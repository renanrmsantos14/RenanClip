//! Descoberta pela tailnet do Tailscale (opcional). Só lê `tailscale status --json`:
//! sem o programa instalado ou sem login, a lista vem vazia e nada quebra.
//! A cifra e o pareamento são os do RenanClip; o Tailscale é só o caminho de rede.

use serde::Serialize;
use std::net::IpAddr;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub dns: String,
    pub ips: Vec<IpAddr>,
    pub online: bool,
}

impl Node {
    /// IPv4 primeiro (mais compatível com firewalls); depois IPv6.
    pub fn best_ip(&self) -> Option<IpAddr> {
        self.ips.iter().find(|i| i.is_ipv4()).or(self.ips.first()).copied()
    }
}

/// Executa `<cmd> status --json` com prazo e devolve os outros nós da tailnet.
pub fn status(cmd: &str) -> Vec<Node> {
    let (tx, rx) = channel();
    let cmd = cmd.to_string();
    let _ = std::thread::Builder::new().name("tailnet".into()).spawn(move || {
        let mut c = Command::new(&cmd);
        c.args(["status", "--json"]).stdin(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // sem janela de console
        }
        let _ = tx.send(c.output().ok().filter(|o| o.status.success()).map(|o| o.stdout));
    });
    match rx.recv_timeout(Duration::from_secs(4)) {
        Ok(Some(out)) => parse(&out),
        _ => vec![],
    }
}

pub fn parse(json: &[u8]) -> Vec<Node> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(json) else { return vec![] };
    let Some(peers) = v.get("Peer").and_then(|p| p.as_object()) else { return vec![] };
    let mut out: Vec<Node> = peers
        .values()
        .filter_map(|p| {
            let ips: Vec<IpAddr> = p.get("TailscaleIPs")?.as_array()?.iter().filter_map(|i| i.as_str()?.parse().ok()).collect();
            if ips.is_empty() {
                return None;
            }
            Some(Node {
                name: p.get("HostName").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                dns: p.get("DNSName").and_then(|s| s.as_str()).unwrap_or("").trim_end_matches('.').to_string(),
                ips,
                online: p.get("Online").and_then(|b| b.as_bool()).unwrap_or(false),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status() {
        let j = br#"{"Self":{"HostName":"eu","TailscaleIPs":["100.64.0.1"]},"Peer":{"k1":{"HostName":"poco","DNSName":"poco.tail1234.ts.net.","TailscaleIPs":["fd7a::2","100.64.0.2"],"Online":true},"k2":{"HostName":"off","TailscaleIPs":["100.64.0.3"],"Online":false},"k3":{"HostName":"sem-ip","TailscaleIPs":[]}}}"#;
        let n = parse(j);
        assert_eq!(n.len(), 2);
        let p = n.iter().find(|n| n.name == "poco").unwrap();
        assert!(p.online);
        assert_eq!(p.dns, "poco.tail1234.ts.net");
        assert_eq!(p.best_ip().unwrap().to_string(), "100.64.0.2");
        assert!(!n.iter().find(|n| n.name == "off").unwrap().online);
    }

    #[test]
    fn lixo_e_comando_ausente_dao_lista_vazia() {
        assert!(parse(b"nao e json").is_empty());
        assert!(parse(b"{}").is_empty());
        assert!(status("renanclip-comando-que-nao-existe").is_empty());
    }
}
