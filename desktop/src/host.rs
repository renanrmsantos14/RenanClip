//! Lado do motor (processo sempre ligado): despacha os comandos da interface e gerencia o
//! processo-filho da janela. A janela vive em OUTRO processo (`renanclip --ui`) que morre ao
//! fechar: assim a RAM do WebKit volta ao sistema e o motor fica parado em poucos MB.
//! Conversa por pipe (uma linha JSON por mensagem): filho→motor `{id,cmd,args}`; motor→filho
//! `{id,ok|err}` (resposta) ou `{ev,data}` (evento).
use crate::{autostart, clip, eng, update, APP, BUILT, CACHE, VERSION};
use base64::Engine as _;
use renanclip_core::{Payload, Settings};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

struct Ui {
    child: Child,
    stdin: ChildStdin,
}
static UI: Mutex<Option<Ui>> = Mutex::new(None);
static PENDING_PROMPT: Mutex<Option<Value>> = Mutex::new(None);
static PENDING_UPDATE: Mutex<Option<update::Info>> = Mutex::new(None);

fn write_line(v: &Value) -> bool {
    let mut g = UI.lock().unwrap();
    let Some(u) = g.as_mut() else { return false };
    let mut s = v.to_string();
    s.push('\n');
    if u.stdin.write_all(s.as_bytes()).and_then(|_| u.stdin.flush()).is_err() {
        *g = None;
        return false;
    }
    true
}

/// Manda um evento à janela (se estiver aberta).
pub fn emit(name: &str, data: Value) {
    write_line(&json!({ "ev": name, "data": data }));
}

fn alive() -> bool {
    UI.lock().unwrap().as_mut().is_some_and(|u| matches!(u.child.try_wait(), Ok(None)))
}

/// Abre a janela (ou traz a que já está aberta para a aba pedida).
pub fn open(tab: &str, popup: bool) {
    static SPAWN: Mutex<()> = Mutex::new(()); // hotkey, bandeja e pareamento podem chamar juntos
    let _one_at_a_time = SPAWN.lock().unwrap();
    if alive() {
        emit("goto", json!(tab));
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.args(["--ui", "--tab", tab]);
    if popup {
        cmd.arg("--popup");
    }
    cmd.env_remove("GDK_DEBUG").stdin(Stdio::piped()).stdout(Stdio::piped());
    match cmd.spawn() {
        Ok(mut child) => {
            let (Some(stdin), Some(out)) = (child.stdin.take(), child.stdout.take()) else { return };
            let pid = child.id();
            *UI.lock().unwrap() = Some(Ui { child, stdin });
            let _ = std::thread::Builder::new().name("ui-reader".into()).spawn(move || reader(out, pid));
        }
        Err(e) => eprintln!("renanclip: não consegui abrir a janela: {e}"),
    }
}

/// Atalho global: abre o seletor; se já estiver aberto, fecha.
pub fn toggle_popup() {
    if alive() {
        emit("close", Value::Null);
    } else {
        open("historico", true);
    }
}

fn reader(out: ChildStdout, pid: u32) {
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let _ = std::thread::Builder::new().name("ui-call".into()).spawn(move || {
            let id = req["id"].clone();
            let reply = match call(req["cmd"].as_str().unwrap_or(""), &req["args"]) {
                Ok(v) => json!({ "id": id, "ok": v }),
                Err(e) => json!({ "id": id, "err": e }),
            };
            write_line(&reply);
        });
    }
    // o filho fechou: libera o slot (se ainda for o mesmo processo) e colhe o zumbi
    let finished = {
        let mut g = UI.lock().unwrap();
        if g.as_ref().is_some_and(|u| u.child.id() == pid) { g.take() } else { None }
    };
    if let Some(mut u) = finished {
        let _ = u.child.wait();
    }
}

pub fn set_pending_prompt(v: Option<Value>) {
    *PENDING_PROMPT.lock().unwrap() = v;
}
pub fn set_pending_update(i: Option<update::Info>) {
    *PENDING_UPDATE.lock().unwrap() = i;
}

fn s(a: &Value, k: &str) -> String {
    a[k].as_str().unwrap_or("").to_string()
}
fn b(a: &Value, k: &str) -> bool {
    a[k].as_bool().unwrap_or(false)
}

/// `close`: veio do seletor (atalho), que fecha e cola; a janela normal só copia e continua aberta.
fn copy_item(id: &str, paste: bool, close: bool) -> Result<(), String> {
    let e = eng();
    let p = e.payload(id).ok_or("item não encontrado")?;
    e.mark_applied(id);
    clip::apply(&p, CACHE.get().unwrap())?;
    e.touch(id);
    if !close {
        return Ok(());
    }
    emit("close", Value::Null);
    if paste && e.settings().auto_paste {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(260));
            clip::paste();
        });
    }
    Ok(())
}

fn save_settings(new: Settings) -> Result<Settings, String> {
    let e = eng();
    let old = e.settings();
    let mut new = e.update_settings(new);
    let mut warn = None;
    if new.hotkey != old.hotkey {
        if let Some(app) = APP.get() {
            if let Err(w) = crate::register_hotkey(app, &old.hotkey, &new.hotkey) {
                // atalho inválido ou ocupado: não guarda; o antigo segue funcionando
                new.hotkey = old.hotkey.clone();
                new = e.update_settings(new);
                warn = Some(w);
            }
        }
    }
    if new.autostart != old.autostart {
        let _ = autostart::set(new.autostart);
    }
    emit("settings", Value::Null);
    match warn {
        Some(w) => Err(w),
        None => Ok(new),
    }
}

/// Todos os comandos da interface. Argumentos em JSON, nomes em snake_case.
pub fn call(cmd: &str, a: &Value) -> Result<Value, String> {
    let e = eng();
    Ok(match cmd {
        "info" => json!({ "version": VERSION, "built": BUILT, "id": e.my_id(), "port": e.port(), "os": std::env::consts::OS }),
        "history" => {
            let kind = a["kind"].as_str().filter(|k| *k != "all");
            json!(e.history(&s(a, "query"), kind, 300))
        }
        "copy_item" => {
            copy_item(&s(a, "id"), b(a, "paste"), b(a, "close"))?;
            Value::Null
        }
        "pin" => {
            e.pin(&s(a, "id"), b(a, "pinned"));
            Value::Null
        }
        "delete_item" => {
            e.delete(&s(a, "id"));
            Value::Null
        }
        "clear_history" => json!(e.clear()),
        "peers" => json!(e.peers()),
        "nearby" => json!(e.nearby()),
        "tailnet" => json!(e.tailnet()),
        "rename_peer" => {
            e.rename_peer(&s(a, "id"), &s(a, "alias"));
            Value::Null
        }
        "revoke_peer" => {
            e.revoke_peer(&s(a, "id"));
            Value::Null
        }
        "open_pairing" => {
            e.open_pairing(a["secs"].as_i64().unwrap_or(120));
            Value::Null
        }
        "pair_decide" => {
            set_pending_prompt(None);
            e.pair_decide(a["pair_id"].as_u64().unwrap_or(0), b(a, "accept"));
            Value::Null
        }
        // por "nome", "nome:porta" ou IP (rede local ou tailnet)
        "pair_with" => {
            let host = s(a, "host");
            let addr = e.resolve(host.trim()).map_err(|er| format!("não achei \"{}\": {er}", host.trim()))?;
            json!(e.pair_with(addr))
        }
        "get_settings" => json!(e.settings()),
        "save_settings" => {
            let st: Settings = serde_json::from_value(a["s"].clone()).map_err(|er| er.to_string())?;
            json!(save_settings(st)?)
        }
        "check_update" => {
            let r = update::check(VERSION)?;
            set_pending_update(r.clone());
            json!(r)
        }
        "install_update" => {
            let info = PENDING_UPDATE.lock().unwrap().clone().ok_or("nenhuma atualização pendente")?;
            update::install(&info)?;
            e.shutdown();
            crate::lock::release();
            if let Some(exe) = crate::EXE.get() {
                let _ = Command::new(exe).arg("--background").spawn();
            }
            std::process::exit(0)
        }
        "image" => match e.payload(&s(a, "id")) {
            Some(Payload::Image { png }) => json!(base64::engine::general_purpose::STANDARD.encode(png)),
            _ => Value::Null,
        },
        // o que aconteceu enquanto a janela não existia (código de pareamento, atualização)
        "pending" => json!({ "prompt": *PENDING_PROMPT.lock().unwrap(), "update": *PENDING_UPDATE.lock().unwrap() }),
        _ => return Err(format!("comando desconhecido: {cmd}")),
    })
}
