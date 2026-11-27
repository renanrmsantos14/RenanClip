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

## Android

11. **Foreground service `connectedDevice`.** `dataSync` tem limite de 6 h por dia no Android 15; `connectedDevice` descreve melhor um servidor TCP na LAN. Notificação com importância mínima e botão "Parar".
12. **JSON próprio e só JCA.** Sem `org.json` (a JVM dos testes não o tem) e sem Bouncy Castle: o núcleo roda idêntico em teste JVM e no aparelho, e o APK fica em ~1 MB.
13. **`MemoryStore` e `SqliteStore` compartilham `StoreRules`.** As regras (lápides, ressurreição, busca sem acento) são testadas uma vez; o SQLite só persiste.
14. **Provider próprio (`ClipFiles`) em vez de `FileProvider`.** Evita androidx; serve só leitura a arquivos em `cache/clip/`.
15. **Endereço digitado fica guardado e não é sobrescrito** por endereços descobertos (`touchPeer(id, "")` na sessão). Tailscale entra apenas como endereço manual (nome MagicDNS ou IP 100.x, porta padrão 47823); cifra e pareamento não mudam.
16. **Sem flag "sensível" no protocolo.** O Android ignora cópias locais com `EXTRA_IS_SENSITIVE` e permite copiar um item como sensível; propagar a flag exigiria mudar o protocolo.
17. **Deadlock em `pairDecide` corrigido:** a decisão do usuário passa por `ArrayBlockingQueue(1)` (um `SynchronousQueue.offer` com timeout bloqueava até o consumidor chamar `poll`).
