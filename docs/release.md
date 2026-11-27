# Como publicar uma versão

O release só roda quando você dispara: **Actions → Release → Run workflow**. Não há gatilho de push nem de PR.

## Secrets (Settings → Secrets and variables → Actions)
| Secret | O que é |
|---|---|
| `UPDATER_PRIVATE_KEY` | Chave privada Ed25519 (PEM) do autoupdate. A pública está em `desktop/updater.pub`; o workflow confere que as duas combinam antes de assinar. |
| `ANDROID_KEYSTORE_B64` | Keystore de release do Android em base64 (`base64 -w0 release.keystore`). |
| `ANDROID_KEYSTORE_PASSWORD` | Senha do keystore (a chave usa a mesma senha). |
| `ANDROID_KEY_ALIAS` | Alias da chave. |

Nada disso entra no repositório. Gerar a chave do updater: `openssl genpkey -algorithm ed25519 -out up.pem`; a pública crua (32 bytes) é `openssl pkey -in up.pem -pubout -outform DER | tail -c 32 > desktop/updater.pub`. Trocar a chave exige um novo build instalado à mão: apps antigos só aceitam updates assinados pela chave antiga.

## O que o workflow faz
1. `bump`: incrementa o PATCH em `version.json` (e arquivos ligados), confere que a tag não existe, faz commit na branch e envia.
2. `desktop`: testes e build release em Linux e Windows → `renanclip-linux-x86_64`, `renanclip-windows-x86_64.exe` (os nomes que o autoupdate procura).
3. `android` (opcional): testes, `assembleRelease`, zipalign e apksigner com o keystore do secret → `renanclip-android.apk`.
4. `publish`: gera `SHA256SUMS`, assina com Ed25519 (`SHA256SUMS.sig`, 64 bytes crus) e cria o release `vX.Y.Z`. Por padrão como **rascunho**: confira os arquivos e publique na tela do GitHub.

O autoupdate só instala se `SHA256SUMS.sig` valer para a chave embutida e o hash do binário bater.

## Ainda não provado
O workflow nunca rodou (de propósito). O primeiro disparo é o teste: use rascunho e confira os assets.
