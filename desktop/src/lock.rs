//! Uma instância por perfil (pasta de dados): trava de arquivo mantida até o processo sair.
use std::fs::File;
use std::path::Path;
use std::sync::Mutex;

static HELD: Mutex<Option<File>> = Mutex::new(None);

/// Solta a trava (antes de reiniciar numa atualização, para o novo processo poder pegá-la).
pub fn release() {
    *HELD.lock().unwrap() = None;
}

/// `false` se outra instância já usa este perfil.
pub fn acquire(dir: &Path) -> bool {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("renanclip.lock");
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let Ok(f) = File::options().create(true).truncate(false).write(true).open(&path) else { return true };
        // SAFETY: fd válido enquanto `f` existir; flock não retém ponteiros.
        let ok = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if ok {
            *HELD.lock().unwrap() = Some(f);
        }
        ok
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        match File::options().create(true).truncate(false).write(true).share_mode(0).open(&path) {
            Ok(f) => {
                *HELD.lock().unwrap() = Some(f);
                true
            }
            Err(_) => false,
        }
    }
}
