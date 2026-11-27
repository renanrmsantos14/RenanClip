---
name: verificar-sync
description: Verifica o RenanClip de ponta a ponta (núcleo, pareamento, cifra, sincronização, Tailscale, app de PC com duas instâncias, RAM parada, versão). Use antes de commit ou PR que mexa em core/, desktop/ ou no protocolo, ou quando pedirem "prova que o sync funciona".
---

# Verificar sync (RenanClip)

Rode em ordem; pare no primeiro vermelho e diga qual camada falhou. Nunca imprima chaves; nunca use os aparelhos reais da tailnet para testar.

## 1. Núcleo (rápido, sem janela)
```
cargo test -p renanclip-core -j2 2>&1 | rg "test result|FAILED|panicked"
```
Esperado: 0 falhas. Cobre pareamento (mesmo código, recusa, janela fechada), cifra (par errado não conecta), texto/imagem/arquivo, dedupe, inventário ao reconectar, revogação, retenção, sensível/pausa/app ignorado, Tailscale simulado (`fake_tailscale`), `resolve` por nome/IP/porta e vetores determinísticos (`core/tests/vetores.rs`, mesmos de `docs/vetores.json` para o Android).

## 2. App de PC (unidade)
```
cargo test -p renanclip-desktop -j2 2>&1 | rg "test result|FAILED"
node scripts/bump-version.mjs --check
```
Esperado: testes de PNG/nome seguro/versão/assinatura do updater passando; versões de `version.json`, `package.json`, `tauri.conf.json` e Cargos iguais.

## 3. Duas instâncias reais (Linux, Xvfb)
O app tem dois processos: o **motor** (sync, bandeja, atalho, clipboard; sem webview) e a **janela** (`renanclip --ui`, filho que morre ao fechar). Cada instância em um display próprio (`Xvfb :101` e `:102`, 640x700), com `RENANCLIP_DATA_DIR` distinto, `RENANCLIP_NO_AUTOSTART=1`, `RENANCLIP_TAILSCALE=/bin/false` (não toca a tailnet real) e `RENANCLIP_DEVICE_NAME` fictício. Em Xvfb a janela precisa de `WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1` (isso carrega libLLVM no motor e infla o PSS: meça a RAM sem elas).
1. Aba Aparelhos (Ctrl+2) das duas; clicar em Parear no item "Rede local"; o código de 6 dígitos tem de ser igual; confirmar nas duas.
2. `xclip -selection clipboard -i` em A (texto, `-t image/png`, `-t text/uri-list`) → conferir com `xclip -o` em B, e o inverso. Imagem: `compare -fuzz 1% -metric AE` = 0.
3. Matar o motor de A e subir de novo: reconecta sem parear; histórico continua.
4. Janela normal (aberta pelo início/bandeja): Enter copia e a janela **continua aberta**. Seletor (Ctrl+Alt+V): Enter copia, fecha e cola (com `metacity --display=:101` e `xev` focado, o `xev` recebe Ctrl+V).
5. Digitar na busca e apertar Delete: apaga caractere, não o item.
Portas 47901/47902 podem estar ocupadas por outros projetos: o app cai para porta automática (isso é o comportamento certo). Prints só com nomes fictícios; mascare IPs (LAN e 100.x aparecem em "Por perto"/"online ·").

## 4. RAM parada (release)
Subir com `--background` (sem janela, sem as variáveis de render por software) e esperar 15 s:
```
awk '/^Pss:/{p+=$2} /^Anonymous:/{n+=$2} END{print p/1024" MB PSS, "n/1024" MB anon"}' /proc/<pid>/smaps
```
Esperado: PSS < 50 MB (medido: ~40 MB, anon ~19 MB). Abrir e fechar a janela 3 vezes (Ctrl+Alt+V, Esc): o filho tem de sumir (`pgrep -P <pid>` = 0) e o anon do motor não crescer.

## 5. Windows e Android
Windows: `cargo check --target x86_64-pc-windows-msvc -p renanclip-desktop` falha aqui por falta de `lib.exe` (MSVC) ao compilar `ring`; registrar "não provado" e provar no notebook Windows. Android: `./gradlew testDebugUnitTest` em `android/`; runtime só com aparelho (`adb devices`).

## Armadilhas já vistas
- Janela em branco no Xvfb = falta das variáveis WEBKIT acima.
- "failed to fill whole buffer" ao parear = o outro lado não estava com a aba Aparelhos aberta (pareamento fechado).
- Teste `core/tests/sync.rs` falhou 1 vez em 20 s com disco a 99%; não reproduziu em 8 rodadas, inclusive com CPU saturada. Se voltar, capture o nome do teste.
- `target/` enche o disco (cada build release ~2 GB): apague `target/debug` entre rodadas.
- Comparar PNG com `convert x rgba:-` falha por 8 vs 16 bits: use `compare -fuzz 1% -metric AE`.
