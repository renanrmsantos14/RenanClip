//! Área de transferência do sistema: observa mudanças, lê (texto, imagem, arquivo) e aplica.
//! Linux (X11): XFixes avisa a troca de dono da seleção, sem polling; Windows: número de sequência.
use arboard::{Clipboard, ImageData};
use renanclip_core::{Engine, LocalCopy, Payload};
use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};

static CB: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();

fn with_cb<T>(f: impl FnOnce(&mut Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
    let m = CB.get_or_init(|| Mutex::new(None));
    let mut g = m.lock().map_err(|_| "área de transferência travada".to_string())?;
    if g.is_none() {
        *g = Some(Clipboard::new().map_err(|e| e.to_string())?);
    }
    f(g.as_mut().unwrap()).map_err(|e| e.to_string())
}

fn encode_png(img: &ImageData) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width as u32, img.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut w = enc.write_header().ok()?;
    w.write_image_data(&img.bytes).ok()?;
    w.finish().ok()?;
    Some(out)
}

fn decode_png(bytes: &[u8]) -> Option<ImageData<'static>> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(ImageData { width: info.width as usize, height: info.height as usize, bytes: Cow::Owned(rgba) })
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().map(|e| e.to_lowercase()).as_deref() {
        Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("zip") => "application/zip",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn safe_name(n: &str) -> String {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("arquivo");
    let s: String = base.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    if s.trim_matches('.').is_empty() { "arquivo".into() } else { s }
}

/// Lê o que está na área de transferência agora (já filtrado por tipo e tamanho).
fn read_current(kind: Kind, cap_bytes: u64) -> Option<LocalCopy> {
    match kind {
        Kind::Text => {
            let t = with_cb(|c| c.get_text()).ok()?;
            Some(LocalCopy::text(t))
        }
        Kind::Image => {
            let img = with_cb(|c| c.get_image()).ok()?;
            if (img.bytes.len() as u64) > cap_bytes * 8 {
                return None; // RGBA enorme: nem tenta codificar
            }
            Some(LocalCopy::image(encode_png(&img)?))
        }
        Kind::Files => {
            let list = with_cb(|c| c.get().file_list()).ok()?;
            let path = list.into_iter().find(|p| p.is_file())?;
            if std::fs::metadata(&path).ok()?.len() > cap_bytes {
                return None;
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(LocalCopy::file(name.clone(), mime_of(&name), std::fs::read(&path).ok()?))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Text,
    Image,
    Files,
}

/// Coloca o conteúdo na área de transferência do sistema.
pub fn apply(p: &Payload, cache_dir: &std::path::Path) -> Result<(), String> {
    match p {
        Payload::Text(t) => with_cb(|c| c.set_text(t.clone())),
        Payload::Image { png } => {
            let img = decode_png(png).ok_or("imagem inválida")?;
            with_cb(|c| c.set_image(img))
        }
        Payload::File { name, bytes, .. } => {
            let dir = cache_dir.join(renanclip_core::crypto::hex(&renanclip_core::crypto::sha256(&[bytes])[..6]));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(safe_name(name));
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            with_cb(|c| c.set().file_list(&[path]))
        }
    }
}

/// Apaga arquivos recebidos com mais de `days` dias (cache de itens aplicados).
pub fn clean_cache(cache_dir: &std::path::Path, days: u64) {
    let limit = std::time::Duration::from_secs(days * 86_400);
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > limit) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

fn handle_change(engine: &Engine, kind: Kind, app: Option<String>, sensitive: bool) {
    let s = engine.settings();
    if s.paused || sensitive || app.as_deref().is_some_and(|a| s.is_ignored(a)) {
        return;
    }
    let cap = (s.max_blob_mb.max(1) as u64) * 1024 * 1024;
    if let Some(mut c) = read_current(kind, cap) {
        c.source_app = app;
        engine.local_copy(c);
    }
}

// ───────────────────────────── Linux (X11) ─────────────────────────────
#[cfg(target_os = "linux")]
mod sys {
    use super::*;
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;
    use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

    fn atom(c: &RustConnection, name: &str) -> Option<u32> {
        Some(c.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
    }

    fn proc_name(pid: u32) -> Option<String> {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?.trim().to_string();
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).ok().and_then(|b| b.split(|&x| x == 0).next().map(|s| String::from_utf8_lossy(s).into_owned())).unwrap_or_default();
        let base = cmd.rsplit('/').next().unwrap_or("").to_string();
        Some(format!("{comm} {base}"))
    }

    /// Lista os formatos (TARGETS) oferecidos pelo dono atual da seleção.
    fn targets(c: &RustConnection, win: u32, clipboard: u32) -> Vec<String> {
        let (Some(t), Some(prop)) = (atom(c, "TARGETS"), atom(c, "RENANCLIP_TARGETS")) else { return vec![] };
        if c.convert_selection(win, clipboard, t, prop, CURRENT_TIME).is_err() || c.flush().is_err() {
            return vec![];
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(600) {
            match c.poll_for_event() {
                Ok(Some(Event::SelectionNotify(n))) if n.requestor == win => {
                    if n.property == 0 {
                        return vec![];
                    }
                    let Ok(Ok(r)) = c.get_property(true, win, prop, AtomEnum::ATOM, 0, 1024).map(|x| x.reply()) else { return vec![] };
                    return r
                        .value32()
                        .map(|it| it.filter_map(|a| c.get_atom_name(a).ok()?.reply().ok().map(|n| String::from_utf8_lossy(&n.name).into_owned())).collect())
                        .unwrap_or_default();
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(Duration::from_millis(8)),
                Err(_) => return vec![],
            }
        }
        vec![]
    }

    pub fn watch(engine: Engine) {
        let _ = std::thread::Builder::new().name("clip-watch".into()).spawn(move || {
            if let Err(e) = run(engine) {
                eprintln!("renanclip: observador da área de transferência parou: {e}");
            }
        });
    }

    fn run(engine: Engine) -> Result<(), Box<dyn std::error::Error>> {
        let (c, n) = RustConnection::connect(None)?;
        let root = c.setup().roots[n].root;
        let win = c.generate_id()?;
        c.create_window(COPY_DEPTH_FROM_PARENT, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE))?;
        let clipboard = atom(&c, "CLIPBOARD").ok_or("sem atom CLIPBOARD")?;
        let pid_atom = atom(&c, "_NET_WM_PID");
        c.xfixes_query_version(5, 0)?.reply()?;
        c.xfixes_select_selection_input(win, clipboard, xfixes::SelectionEventMask::SET_SELECTION_OWNER)?;
        c.flush()?;
        let me = std::process::id();
        loop {
            let Event::XfixesSelectionNotify(ev) = c.wait_for_event()? else { continue };
            if ev.selection != clipboard || ev.owner == 0 {
                continue;
            }
            // junta rajadas (gerenciadores de área de transferência reafirmam a posse)
            std::thread::sleep(Duration::from_millis(120));
            let mut owner = ev.owner;
            while let Some(e) = c.poll_for_event()? {
                if let Event::XfixesSelectionNotify(e2) = e {
                    owner = e2.owner;
                }
            }
            if owner == 0 {
                continue;
            }
            let pid = pid_atom.and_then(|a| c.get_property(false, owner, a, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?.value32()?.next());
            if pid == Some(me) {
                continue; // fomos nós que aplicamos
            }
            let app = pid.and_then(proc_name);
            let t = targets(&c, win, clipboard);
            let sensitive = t.iter().any(|x| x == "x-kde-passwordManagerHint");
            let has = |names: &[&str]| t.iter().any(|x| names.contains(&x.as_str()));
            let kind = if has(&["text/uri-list", "x-special/gnome-copied-files"]) {
                Kind::Files
            } else if has(&["image/png"]) {
                Kind::Image
            } else {
                Kind::Text
            };
            handle_change(&engine, kind, app, sensitive);
        }
    }

    /// Ctrl+V sintético (XTEST) na janela com foco.
    pub fn paste() {
        let _ = (|| -> Result<(), Box<dyn std::error::Error>> {
            let (c, n) = RustConnection::connect(None)?;
            let root = c.setup().roots[n].root;
            let (min, max) = (c.setup().min_keycode, c.setup().max_keycode);
            let map = c.get_keyboard_mapping(min, max - min + 1)?.reply()?;
            let per = map.keysyms_per_keycode as usize;
            let find = |sym: u32| map.keysyms.chunks(per).position(|k| k.contains(&sym)).map(|i| min + i as u8);
            let (ctrl, v) = (find(0xffe3).ok_or("sem Control_L")?, find(0x76).ok_or("sem v")?);
            for (ty, key) in [(xproto::KEY_PRESS_EVENT, ctrl), (xproto::KEY_PRESS_EVENT, v), (xproto::KEY_RELEASE_EVENT, v), (xproto::KEY_RELEASE_EVENT, ctrl)] {
                c.xtest_fake_input(ty, key, 0, root, 0, 0, 0)?;
            }
            c.flush()?;
            c.get_input_focus()?.reply()?;
            let _ = PropMode::REPLACE;
            Ok(())
        })();
    }
}

// ───────────────────────────── Windows ─────────────────────────────
#[cfg(windows)]
mod sys {
    use super::*;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, HWND};
    use windows_sys::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber, GetClipboardFormatNameW, IsClipboardFormatAvailable, OpenClipboard, CloseClipboard, EnumClipboardFormats, RegisterClipboardFormatW};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    const CF_HDROP: u32 = 15;
    const CF_DIB: u32 = 8;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn owner_exe() -> Option<String> {
        unsafe {
            let hwnd: HWND = GetClipboardOwner();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().unwrap_or("").to_string())
        }
    }

    /// Formatos padrão que gerenciadores de senha usam para pedir que a cópia não seja guardada.
    fn sensitive() -> bool {
        unsafe {
            for n in ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                let f = RegisterClipboardFormatW(wide(n).as_ptr());
                if f != 0 && IsClipboardFormatAvailable(f) != 0 && n.starts_with("Exclude") {
                    return true;
                }
            }
            false
        }
    }

    fn has_bitmap() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_DIB) != 0 }
    }
    fn has_files() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_HDROP) != 0 }
    }
    #[allow(dead_code)]
    fn names() -> Vec<String> {
        let mut v = vec![];
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let mut f = EnumClipboardFormats(0);
                while f != 0 {
                    let mut b = [0u16; 128];
                    let n = GetClipboardFormatNameW(f, b.as_mut_ptr(), 128);
                    if n > 0 {
                        v.push(String::from_utf16_lossy(&b[..n as usize]));
                    }
                    f = EnumClipboardFormats(f);
                }
                CloseClipboard();
            }
        }
        v
    }

    pub fn watch(engine: Engine) {
        let _ = std::thread::Builder::new().name("clip-watch".into()).spawn(move || {
            let mut last = unsafe { GetClipboardSequenceNumber() };
            loop {
                std::thread::sleep(Duration::from_millis(350));
                let now = unsafe { GetClipboardSequenceNumber() };
                if now == last {
                    continue;
                }
                last = now;
                std::thread::sleep(Duration::from_millis(80));
                let kind = if has_files() { Kind::Files } else if has_bitmap() { Kind::Image } else { Kind::Text };
                handle_change(&engine, kind, owner_exe(), sensitive());
            }
        });
    }

    pub fn paste() {
        unsafe {
            let key = |vk: u16, up: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } } };
            let seq = [key(VK_CONTROL, false), key(0x56, false), key(0x56, true), key(VK_CONTROL, true)];
            SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }
}

pub use sys::{paste, watch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_ida_e_volta() {
        let img = ImageData { width: 2, height: 1, bytes: Cow::Owned(vec![255, 0, 0, 255, 0, 255, 0, 128]) };
        let png = encode_png(&img).unwrap();
        let back = decode_png(&png).unwrap();
        assert_eq!((back.width, back.height), (2, 1));
        assert_eq!(&back.bytes[..], &img.bytes[..]);
        assert!(decode_png(b"nao e png").is_none());
    }

    #[test]
    fn nome_de_arquivo_seguro() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a<b>.txt"), "a_b_.txt");
        assert_eq!(safe_name(".."), "arquivo");
        assert_eq!(mime_of("x.PDF"), "application/pdf");
    }
}
