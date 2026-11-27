# Estado: app-completo

Nível 2 (feature grande). Branch `feature/app-completo`. PRs empilhados: 1 núcleo, 2 PC, 3 Android, 4 release.

## Etapa atual
PR1 (núcleo/protocolo): pronto para PR.

## Provas
- `cargo test -p renanclip-core`: 22 testes unitários + 8 de integração (duas Engines em loopback) passando: pareamento com mesmo código, recusa, janela fechada, texto/imagem (3 chunks)/arquivo, dedupe, inventário ao reconectar, revogação, par sem chave não conecta, sensível/pausa/app ignorado, retenção com fixados.

## Falta
PR2 PC (Tauri, clipboard, UI, bandeja, autostart, autoupdate, RAM) · PR3 Android · PR4 release.
