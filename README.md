# RenanClip

App pessoal leve da família Renan* (RenanTalk, RenanSkip): área de transferência compartilhada entre PCs (Linux/Windows) e Android, pela rede local, cifrada, sem servidor.

- `core/`: protocolo, cripto, histórico SQLite e motor de sync (Rust).
- `desktop/`: app de PC (Tauri 2): motor, bandeja, atalho global, clipboard, autostart, autoupdate e a janela.
- `docs/protocolo.md`, `docs/decisoes.md`.

Testes: `cargo test -p renanclip-core -p renanclip-desktop`. Versão: `version.json` (`node scripts/bump-version.mjs`).

## Usar no PC
Atalho global **Ctrl+Alt+V** abre o seletor (Enter copia, fecha e cola; Esc fecha). Pela bandeja abre a janela completa. Ctrl+1/2/3 trocam histórico, aparelhos e ajustes; Alt+1…9 escolhe o item.

Variáveis de ambiente (testes e uso avançado): `RENANCLIP_DATA_DIR`, `RENANCLIP_PORT`, `RENANCLIP_TAILSCALE` (binário do tailscale), `RENANCLIP_TAILNET_PORT`, `RENANCLIP_NO_AUTOSTART`, `RENANCLIP_DEVICE_NAME`.

## Dois processos: RAM parada baixa
O **motor** (sync, bandeja, atalho, clipboard) roda sempre, sem webview: ~19 MB anônimos (~40 MB PSS contando bibliotecas compartilhadas). A **janela** é um processo filho (`renanclip --ui`) que nasce ao abrir e morre ao fechar, falando com o motor por linhas JSON no pipe. Assim a RAM do WebKit volta ao sistema ao fechar.
