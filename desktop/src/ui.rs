//! Processo da janela (`renanclip --ui`): só a webview e uma ponte para o motor pelo pipe
//! (stdout = pedidos, stdin = respostas e eventos). Termina ao fechar a janela.
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::http::Response;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

type Reply = Result<Value, String>;
static WAIT: OnceLock<Mutex<HashMap<u64, Sender<Reply>>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
static OUT: Mutex<()> = Mutex::new(());
static GOT_FOCUS: AtomicBool = AtomicBool::new(false);

fn waiters() -> &'static Mutex<HashMap<u64, Sender<Reply>>> {
    WAIT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn call(cmd: &str, args: Value) -> Reply {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = channel();
    waiters().lock().unwrap().insert(id, tx);
    {
        let _g = OUT.lock().unwrap();
        let mut so = std::io::stdout().lock();
        writeln!(so, "{}", json!({ "id": id, "cmd": cmd, "args": args })).and_then(|_| so.flush()).map_err(|e| e.to_string())?;
    }
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(r) => r,
        Err(_) => {
            waiters().lock().unwrap().remove(&id);
            Err("o RenanClip em segundo plano não respondeu".into())
        }
    }
}

#[tauri::command]
async fn rpc(cmd: String, args: Option<Value>) -> Reply {
    match tauri::async_runtime::spawn_blocking(move || call(&cmd, args.unwrap_or(Value::Null))).await {
        Ok(r) => r,
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn quit_ui(app: AppHandle) {
    app.exit(0);
}

fn pump(h: AppHandle) {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(id) = v["id"].as_u64() {
            let r = match v.get("err") {
                Some(e) => Err(e.as_str().unwrap_or("erro").to_string()),
                None => Ok(v["ok"].clone()),
            };
            if let Some(tx) = waiters().lock().unwrap().remove(&id) {
                let _ = tx.send(r);
            }
        } else if let Some(ev) = v["ev"].as_str() {
            match ev {
                "close" => h.exit(0),
                "goto" => {
                    if let Some(w) = h.get_webview_window("main") {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                    let _ = h.emit("goto", v["data"].clone());
                }
                _ => {
                    let _ = h.emit(ev, v["data"].clone());
                }
            }
        }
    }
    h.exit(0); // o motor sumiu: não deixa janela órfã
}

pub fn run(tab: String, popup: bool) {
    let app = tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("rc", |_ctx, req, responder| {
            // rc://localhost/<id>: imagem do histórico
            let id = req.uri().path().trim_matches('/').rsplit('/').next().unwrap_or("").to_string();
            std::thread::spawn(move || {
                let resp = match call("image", json!({ "id": id })) {
                    Ok(Value::String(b64)) => match base64::engine::general_purpose::STANDARD.decode(b64) {
                        Ok(png) => Response::builder().header("content-type", "image/png").header("cache-control", "max-age=3600").body(png).unwrap(),
                        Err(_) => Response::builder().status(404).body(Vec::new()).unwrap(),
                    },
                    _ => Response::builder().status(404).body(Vec::new()).unwrap(),
                };
                responder.respond(resp);
            });
        })
        .invoke_handler(tauri::generate_handler![rpc, quit_ui])
        .on_window_event(move |w, ev| {
            if let WindowEvent::Focused(f) = ev {
                if *f {
                    GOT_FOCUS.store(true, Ordering::SeqCst);
                } else if popup && GOT_FOCUS.load(Ordering::SeqCst) {
                    w.app_handle().exit(0); // seletor: perdeu o foco, fecha
                }
            }
        })
        .setup(move |app| {
            let url = format!("index.html?tab={tab}&popup={}", popup as u8);
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App(url.into()))
                .title("RenanClip")
                .inner_size(560.0, 640.0)
                .min_inner_size(420.0, 420.0)
                .center()
                .focused(true)
                .build()?;
            let h = app.handle().clone();
            std::thread::Builder::new().name("pump".into()).spawn(move || pump(h)).ok();
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("falha ao montar a janela");
    app.run(|_, _| {});
}
