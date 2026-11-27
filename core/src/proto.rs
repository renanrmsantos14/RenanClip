//! Mensagens da sessão (JSON dentro de quadros cifrados) e tipos públicos de eventos.

use serde::{Deserialize, Serialize};

/// Primeiro byte do quadro decifrado.
pub const T_JSON: u8 = 1;
pub const T_CHUNK: u8 = 2; // id(16) || offset(u32 BE) || dados
pub const CHUNK: usize = 256 * 1024;
pub const MAX_TEXT: usize = 1024 * 1024;
pub const INVENTORY: usize = 200;
/// Porta preferida do servidor (a mesma em todos os aparelhos, para achar pares pela tailnet).
pub const DEFAULT_PORT: u16 = 47823;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClipMeta {
    pub id: String,
    pub ts: i64,
    pub kind: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: i64,
    /// true = acabou de ser copiado (aplicar na área de transferência); false = reposição.
    #[serde(default)]
    pub live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t")]
pub enum Msg {
    #[serde(rename = "inv")]
    Inv { items: Vec<(String, i64)> },
    #[serde(rename = "want")]
    Want { ids: Vec<String> },
    #[serde(rename = "clip")]
    Clip(ClipMeta),
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "pong")]
    Pong {},
    #[serde(rename = "bye")]
    Bye { revoked: bool },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
pub enum Hello {
    #[serde(rename = "s1")]
    S1 { id: String, n: String },
    #[serde(rename = "s2")]
    S2 { id: String, n: String },
}

#[derive(Clone, Debug)]
pub enum Payload {
    Text(String),
    Image { png: Vec<u8> },
    File { name: String, mime: String, bytes: Vec<u8> },
}

#[derive(Clone, Debug)]
pub enum Event {
    History,
    Peers,
    /// Aplicar na área de transferência do sistema.
    Apply { id: String, payload: Payload },
    PairPrompt { pair_id: u64, peer_name: String, sas: String, inbound: bool },
    PairDone { pair_id: u64, ok: bool, peer_name: String, error: String },
}

#[derive(Clone, Debug)]
pub struct LocalCopy {
    pub kind: String, // text | image | file
    pub name: String,
    pub mime: String,
    pub text: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub source_app: Option<String>,
    pub sensitive: bool,
}

impl LocalCopy {
    pub fn text(t: impl Into<String>) -> Self {
        Self { kind: "text".into(), name: String::new(), mime: "text/plain".into(), text: Some(t.into()), bytes: None, source_app: None, sensitive: false }
    }
    pub fn image(png: Vec<u8>) -> Self {
        Self { kind: "image".into(), name: String::new(), mime: "image/png".into(), text: None, bytes: Some(png), source_app: None, sensitive: false }
    }
    pub fn file(name: impl Into<String>, mime: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { kind: "file".into(), name: name.into(), mime: mime.into(), text: None, bytes: Some(bytes), source_app: None, sensitive: false }
    }
}
