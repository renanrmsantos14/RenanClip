use serde::{Deserialize, Serialize};

/// Preferências persistidas como JSON na tabela `settings` (chave "settings").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub device_name: String,
    /// Dias de histórico (fixados não expiram).
    pub retention_days: i64,
    pub paused: bool,
    /// Nomes de apps (trecho, sem maiúsculas) cujas cópias são ignoradas.
    pub ignored_apps: Vec<String>,
    pub sync_images: bool,
    pub sync_files: bool,
    pub max_blob_mb: i64,
    /// Teto do espaço de imagens e arquivos no histórico (MB).
    pub store_cap_mb: i64,
    // Preferências só da interface (guardadas aqui para viajarem com o perfil).
    pub theme: String, // auto | claro | escuro
    pub hotkey: String,
    pub auto_paste: bool,
    pub autostart: bool,
    pub auto_update: bool,
    /// Procurar aparelhos também pela tailnet do Tailscale (se instalado).
    pub tailscale: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            retention_days: 7,
            paused: false,
            ignored_apps: vec!["keepass".into(), "bitwarden".into(), "1password".into(), "lastpass".into(), "dashlane".into(), "proton pass".into(), "enpass".into()],
            sync_images: true,
            sync_files: true,
            max_blob_mb: 10,
            store_cap_mb: 500,
            theme: "auto".into(),
            hotkey: "Ctrl+Alt+V".into(),
            auto_paste: true,
            autostart: true,
            auto_update: true,
            tailscale: true,
        }
    }
}

impl Settings {
    pub fn is_ignored(&self, app: &str) -> bool {
        let a = app.to_lowercase();
        !a.is_empty() && self.ignored_apps.iter().any(|x| !x.trim().is_empty() && a.contains(&x.trim().to_lowercase()))
    }
    pub fn clamp(mut self) -> Self {
        self.retention_days = self.retention_days.clamp(1, 3650);
        self.max_blob_mb = self.max_blob_mb.clamp(1, 100);
        self.store_cap_mb = self.store_cap_mb.clamp(10, 10_000);
        self
    }
}
