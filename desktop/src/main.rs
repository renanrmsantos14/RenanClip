//! RenanClip (PC). Dois modos no mesmo executável:
//! - motor (padrão): sincronização, bandeja, atalho global e clipboard, sem webview (RAM parada baixa);
//! - `--ui`: a janela, em processo filho que morre ao fechar (ver `host.rs` e `ui.rs`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
mod clip;
mod host;
mod lock;
mod ui;
mod update;

use renanclip_core::{Config, Engine, Event};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, RunEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const VERSION: &str = env!("RC_VERSION");
pub const BUILT: &str = env!("RC_BUILT");

static ENGINE: OnceLock<Engine> = OnceLock::new();
pub static APP: OnceLock<AppHandle> = OnceLock::new();
pub static CACHE: OnceLock<PathBuf> = OnceLock::new();
/// Caminho do executável lido ao iniciar (depois de uma atualização o arquivo em uso é outro).
pub static EXE: OnceLock<PathBuf> = OnceLock::new();

pub fn eng() -> &'static Engine {
    ENGINE.get().expect("motor não iniciado")
}

fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RENANCLIP_DATA_DIR") {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    base.unwrap_or_else(std::env::temp_dir).join("RenanClip")
}

fn on_event(e: Event) {
    use serde_json::json;
    match e {
        Event::History => host::emit("history", serde_json::Value::Null),
        Event::Peers => host::emit("peers", serde_json::Value::Null),
        Event::Apply { id, payload } => {
            // pode chegar antes de `Engine::start` devolver o handle
            for _ in 0..200 {
                if ENGINE.get().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // marca antes: o observador não pode reenviar o que acabamos de receber
            eng().mark_applied(&id);
            if let Err(err) = clip::apply(&payload, CACHE.get().unwrap()) {
                eprintln!("renanclip: não consegui aplicar {id}: {err}");
            }
        }
        Event::PairPrompt { pair_id, peer_name, sas, inbound } => {
            let v = json!({ "pair_id": pair_id, "peer_name": peer_name, "sas": sas, "inbound": inbound });
            host::set_pending_prompt(Some(v.clone()));
            host::open("aparelhos", false);
            host::emit("pair-prompt", v);
        }
        Event::PairDone { pair_id, ok, peer_name, error } => {
            host::set_pending_prompt(None);
            host::emit("pair-done", json!({ "pair_id": pair_id, "ok": ok, "peer_name": peer_name, "error": error }));
        }
    }
}

/// Registra o novo atalho e só então solta o antigo: se o novo falhar, o antigo continua valendo.
pub fn register_hotkey(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let (old, new) = (old.trim(), new.trim());
    if !new.is_empty() {
        gs.on_shortcut(new, |_app, _s, ev| {
            if ev.state == ShortcutState::Pressed {
                host::toggle_popup();
            }
        })
        .map_err(|e| format!("atalho \"{new}\" indisponível: {e}"))?;
    }
    if !old.is_empty() && old != new {
        let _ = gs.unregister(old);
    }
    Ok(())
}

// ───────────────────────────── início ─────────────────────────────
fn arg_after(flag: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1).cloned())
}

fn main() {
    if std::env::args().any(|a| a == "--ui") {
        ui::run(arg_after("--tab").unwrap_or_else(|| "historico".into()), std::env::args().any(|a| a == "--popup"));
        return;
    }
    // o motor não desenha nada: sem GL e só X11 (a captura é por XFixes), o que poupa ~20 MB de bibliotecas
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("GDK_DEBUG", "gl-disable");
        std::env::set_var("GDK_BACKEND", "x11");
    }
    EXE.set(std::env::current_exe().unwrap_or_default()).ok();
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if !lock::acquire(&dir) {
        eprintln!("renanclip: já está rodando");
        return;
    }
    update::cleanup();
    let cache = dir.join("recebidos");
    let _ = std::fs::create_dir_all(&cache);
    CACHE.set(cache.clone()).ok();

    let mut cfg = Config::new(dir.clone());
    if let Some(p) = std::env::var("RENANCLIP_PORT").ok().and_then(|p| p.parse().ok()) {
        cfg.port = p;
    }
    cfg.device_name = std::env::var("RENANCLIP_DEVICE_NAME").ok().filter(|n| !n.trim().is_empty());
    if let Ok(t) = std::env::var("RENANCLIP_TAILSCALE") {
        cfg.tailscale_cmd = Some(t);
    }
    if let Some(p) = std::env::var("RENANCLIP_TAILNET_PORT").ok().and_then(|p| p.parse().ok()) {
        cfg.tailnet_port = p;
    }
    let engine = match Engine::start(cfg, on_event) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("renanclip: não consegui iniciar: {e}");
            std::process::exit(1);
        }
    };
    ENGINE.set(engine.clone()).ok();
    clip::clean_cache(&cache, engine.settings().retention_days.max(1) as u64);
    clip::watch(engine.clone());

    let background = std::env::args().any(|a| a == "--background");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let h = app.handle().clone();
            APP.set(h.clone()).ok();
            let s = eng().settings();

            let abrir = MenuItem::with_id(app, "abrir", "Abrir RenanClip", true, None::<&str>)?;
            let pausa = CheckMenuItem::with_id(app, "pausa", "Pausar sincronização", true, s.paused, None::<&str>)?;
            let sair = MenuItem::with_id(app, "sair", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&abrir, &pausa, &PredefinedMenuItem::separator(app)?, &sair])?;
            let pausa2 = pausa.clone();
            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
            TrayIconBuilder::with_id("tray")
                .icon(icon)
                .tooltip(format!("RenanClip {VERSION}"))
                .menu(&menu)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "abrir" => host::open("historico", false),
                    "pausa" => {
                        let mut st = eng().settings();
                        st.paused = !st.paused;
                        let _ = pausa2.set_checked(st.paused);
                        eng().update_settings(st);
                        host::emit("settings", serde_json::Value::Null);
                    }
                    "sair" => {
                        eng().shutdown();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            if let Err(e) = register_hotkey(&h, "", &s.hotkey) {
                eprintln!("renanclip: {e}");
            }
            if std::env::var_os("RENANCLIP_NO_AUTOSTART").is_none() {
                let _ = autostart::set(s.autostart);
            }
            if !background {
                host::open("historico", false);
            }
            // verifica atualização ao iniciar (após 20 s) e a cada 6 h
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(20));
                loop {
                    if eng().settings().auto_update {
                        if let Ok(Some(info)) = update::check(VERSION) {
                            host::set_pending_update(Some(info.clone()));
                            host::emit("update-available", serde_json::json!(info));
                        }
                    }
                    std::thread::sleep(Duration::from_secs(6 * 3600));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("falha ao montar o app");

    app.run(|_app, ev| {
        if let RunEvent::ExitRequested { api, code, .. } = ev {
            if code.is_none() {
                api.prevent_exit(); // fechar a janela não encerra: continua na bandeja
            }
        }
    });
}
