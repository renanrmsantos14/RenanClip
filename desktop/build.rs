fn main() {
    // Versão única do app (version.json na raiz), embutida no binário.
    let raw = std::fs::read_to_string("../version.json").unwrap_or_default();
    let field = |k: &str| -> String {
        let key = format!("\"{k}\"");
        raw.find(&key)
            .and_then(|i| raw[i + key.len()..].split('"').nth(1).map(|s| s.to_string()))
            .unwrap_or_default()
    };
    println!("cargo:rustc-env=RC_VERSION={}", field("version"));
    println!("cargo:rustc-env=RC_BUILT={}", field("built"));
    println!("cargo:rerun-if-changed=../version.json");
    tauri_build::build();
}
