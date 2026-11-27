# RenanClip para Android

App nativo em Kotlin (sem Compose, sem bibliotecas além do SDK; criptografia pela JCA). APK debug ≈ 0,9 MB.

## Como construir

Requisitos: JDK 17, Android SDK (plataforma 35) e `android/local.properties` com `sdk.dir=...` (ignorado pelo git).

```bash
cd android
export JAVA_HOME=/caminho/do/jdk17
./gradlew testDebugUnitTest    # núcleo: vetores de docs/vetores.json + pareamento/sessão em loopback
./gradlew assembleDebug        # app/build/outputs/apk/debug/app-debug.apk (keystore debug padrão)
```

`version.json` (raiz do repositório) define `versionName`; o app mostra `v<versão> · dd/mm/aaaa HH:mm` (America/Sao_Paulo, hora do build) num canto de todas as telas. Não há keystore de release no repositório.

## Estrutura

- `core/` — Kotlin puro (testado na JVM): `Crypto`, `Frame`, `Pairing` (SAS de 6 dígitos), `Proto`, `Store` (regras + `MemoryStore`), `Engine` (rede, sessão, sync inv/want/clip/chunks, lápides).
- `SqliteStore` — histórico em SQLite; imagem/arquivo em `filesDir/blobs/<id>`; busca sem acento por coluna pré-dobrada.
- `Nsd` — registro e descoberta `_renanclip._tcp.` (TXT `id`, `n`, `v`).
- `ClipService` — serviço em primeiro plano (`connectedDevice`), notificação discreta, reconexão ao mudar a rede.
- Telas: `MainActivity` (histórico, busca, fixar, apagar), `PeersActivity` (aparelhos, pareamento, endereço manual/Tailscale), `SettingsActivity`.
- `SendTileService` + `SendClipboardActivity` (tile "Enviar área de transferência"), `ShareActivity` (`ACTION_SEND`/`SEND_MULTIPLE`).
- `ClipFiles` — provider só leitura para entregar imagem/arquivo recebido à área de transferência.

## Tailscale / fora de casa

O servidor escuta na porta 47823 (outra automática se ocupada). Em Aparelhos, "Adicionar por nome ou IP" aceita nome MagicDNS ou IP 100.x (porta opcional) e guarda o endereço como último endereço do par. Reconexão: NSD/último endereço bom, depois o endereço guardado. Sem rede ou sem Tailscale o app apenas fica offline.

## Provado (nesta máquina)

- `./gradlew testDebugUnitTest`: vetores de teste (ECDH, pareamento, sessão, ids de conteúdo), sync completo entre duas instâncias Kotlin em loopback (pareamento com SAS, inv/want/clip, lápides, blob em chunks).
- `./gradlew assembleDebug` compila e empacota (944 KB).

## Não provado (sem dispositivo nem emulador disponíveis)

Tudo que depende do sistema Android: NSD real, serviço em primeiro plano e sobrevivência com tela apagada/Doze, tile, share, leitura/escrita da área de transferência (incl. flag sensível), notificação, permissões em runtime, tema, layouts. Interoperabilidade com o PC (Rust) só pelos vetores; falta um teste ponta a ponta com o aplicativo do PC.

## Limitações conhecidas

- O protocolo não leva o marcador "sensível": cópias locais marcadas são ignoradas e é possível copiar um item recebido como sensível, mas o outro lado não é avisado.
- Android 10+ não permite ler a área de transferência em segundo plano: o envio é por ação explícita (tile, compartilhar, botão).
- Android 15 limita serviços `dataSync` a 6 h; por isso o tipo é `connectedDevice`.
