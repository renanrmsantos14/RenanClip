//! Autoupdate pelo GitHub Releases. O binário só é trocado se o SHA-256 bater com o
//! `SHA256SUMS`, e esse arquivo precisa ter assinatura ed25519 válida (chave pública embutida).
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use renanclip_core::crypto::{hex, sha256};
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

const REPO: &str = "renanrmsantos14/RenanClip";
const PUBKEY: &[u8; 32] = include_bytes!("../updater.pub");
const MAX_BIN: u64 = 200 * 1024 * 1024;

#[cfg(windows)]
const ASSET: &str = "renanclip-windows-x86_64.exe";
#[cfg(not(windows))]
const ASSET: &str = "renanclip-linux-x86_64";

#[derive(Serialize, Clone, Debug)]
pub struct Info {
    pub version: String,
    pub notes: String,
    #[serde(skip)]
    urls: Vec<(String, String)>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).user_agent("RenanClip-updater").build()
}

pub fn parse_ver(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|x| x.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Versão mais nova que a atual, se houver.
pub fn check(current: &str) -> Result<Option<Info>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let v: serde_json::Value = agent().get(&url).set("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let (Some(new), Some(cur)) = (parse_ver(tag), parse_ver(current)) else { return Ok(None) };
    if new <= cur {
        return Ok(None);
    }
    let urls = v["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["browser_download_url"].as_str()?.to_string()))).collect())
        .unwrap_or_default();
    Ok(Some(Info { version: tag.trim_start_matches('v').to_string(), notes: v["body"].as_str().unwrap_or_default().chars().take(600).collect(), urls }))
}

fn url_of(info: &Info, name: &str) -> Result<String, String> {
    info.urls.iter().find(|(n, _)| n == name).map(|(_, u)| u.clone()).ok_or_else(|| format!("a release não tem {name}"))
}

fn get_small(url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    agent().get(url).call().map_err(|e| e.to_string())?.into_reader().take(max).read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Confere a assinatura do SHA256SUMS e devolve o hash esperado do arquivo `name`.
pub fn expected_hash(sums: &[u8], sig: &[u8], name: &str) -> Result<String, String> {
    let key = VerifyingKey::from_bytes(PUBKEY).map_err(|_| "chave pública inválida")?;
    let sig = Signature::from_slice(sig).map_err(|_| "assinatura inválida")?;
    key.verify(sums, &sig).map_err(|_| "assinatura do SHA256SUMS não confere")?;
    let text = String::from_utf8_lossy(sums);
    text.lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, n)| n.trim().trim_start_matches('*') == name)
        .map(|(h, _)| h.to_lowercase())
        .ok_or_else(|| format!("{name} não está no SHA256SUMS"))
}

/// Baixa, confere e troca o executável atual. O app novo vale a partir do próximo início.
pub fn install(info: &Info) -> Result<(), String> {
    let sums = get_small(&url_of(info, "SHA256SUMS")?, 1 << 20)?;
    let sig = get_small(&url_of(info, "SHA256SUMS.sig")?, 4096)?;
    let want = expected_hash(&sums, &sig, ASSET)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new: PathBuf = exe.with_extension("new");
    let mut bin = Vec::new();
    agent().get(&url_of(info, ASSET)?).call().map_err(|e| e.to_string())?.into_reader().take(MAX_BIN).read_to_end(&mut bin).map_err(|e| e.to_string())?;
    if hex(&sha256(&[&bin])) != want {
        return Err("o download não bate com o SHA-256 assinado; nada foi trocado".into());
    }
    std::fs::write(&new, &bin).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&new, &exe).map_err(|e| e.to_string())
}

/// Remove o resto de uma atualização anterior (Windows não apaga o .exe em uso).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versoes() {
        assert!(parse_ver("v0.0.10") > parse_ver("0.0.9"));
        assert_eq!(parse_ver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_ver("lixo"), None);
    }

    #[test]
    fn assinatura_invalida_e_rejeitada() {
        assert!(expected_hash(b"abc  renanclip-linux-x86_64\n", &[0u8; 64], ASSET).is_err());
        assert!(expected_hash(b"abc", &[0u8; 10], ASSET).is_err());
    }
}
