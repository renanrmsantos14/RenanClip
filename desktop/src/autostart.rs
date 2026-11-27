//! Iniciar com o sistema: .desktop em ~/.config/autostart (Linux) ou chave Run do usuário (Windows).
use std::path::PathBuf;

/// `RENANCLIP_NO_AUTOSTART=1` desliga (testes e perfis de desenvolvimento).
pub fn set(enabled: bool) -> Result<(), String> {
    if std::env::var_os("RENANCLIP_NO_AUTOSTART").is_some() {
        return Ok(());
    }
    let exe: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME ausente".to_string())?;
        let dir = PathBuf::from(std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"))).join("autostart");
        let file = dir.join("renanclip.desktop");
        if enabled {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let body = format!(
                "[Desktop Entry]\nType=Application\nName=RenanClip\nComment=Área de transferência compartilhada\nExec=\"{}\" --background\nIcon=renanclip\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                exe.display()
            );
            std::fs::write(&file, body).map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let mut c = std::process::Command::new("reg");
        if enabled {
            c.args(["add", key, "/v", "RenanClip", "/t", "REG_SZ", "/d", &format!("\"{}\" --background", exe.display()), "/f"]);
        } else {
            c.args(["delete", key, "/v", "RenanClip", "/f"]);
        }
        let ok = c.creation_flags(0x0800_0000).output().map(|o| o.status.success()).unwrap_or(false);
        if ok || !enabled { Ok(()) } else { Err("não consegui gravar a chave de inicialização".into()) }
    }
}
