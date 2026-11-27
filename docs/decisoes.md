# Decisões

1. **Rust + Tauri 2 no PC.** Binário pequeno, sem Node em runtime; a janela só existe enquanto aberta (webview destruído ao fechar) para ficar abaixo de 50 MB parado.
2. **Núcleo em crate `core` sem UI.** Protocolo, cripto, store e motor testáveis sem janela; reaproveitável pelo desktop e testável por vetores no Android.
3. **Android em Kotlin nativo, não Tauri mobile.** Tile de Configurações rápidas, serviço em primeiro plano, share target e NSD exigiriam plugins Kotlin de qualquer jeito; Tauri mobile soma WebView e NDK (não há NDK local). Nativo é mais leve e mais rápido de abrir. Custo: segunda implementação do protocolo, coberta por vetores de teste cruzados (`docs/vetores.json`).
4. **Pareamento por comparação numérica (SAS), não por código digitado.** Código curto digitado como PSK permite força bruta offline por quem vê o tráfego; comparação com compromisso não.
5. **Cripto:** ECDH P-256, HKDF-SHA256, AES-256-GCM com nonce de contador. Chave por par, derivada no pareamento; sessões derivam chaves novas por direção.
6. **Itens por hash de conteúdo + lápides.** Sync idempotente e sem relógio confiável; dedupe vira "subir ao topo".
7. **Sem tokio.** Threads simples bastam para poucos pares e mantêm o binário e a RAM menores.
8. **Android 10+ não lê a área de transferência em segundo plano.** Envio por ação explícita (tile, compartilhar); recebimento aplicado pelo serviço.
9. **Autoupdate próprio via GitHub Releases** (SHA256 + assinatura ed25519), no padrão do RenanTalk. Repositório público: nenhuma chave no repo.
10. **Sem GitHub Actions em PR/push** (minutos pagos): `release.yml` só com `workflow_dispatch`.
