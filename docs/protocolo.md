# Protocolo RenanClip (v1)

Rede local, TCP, sem servidor. Descoberta por mDNS (`_renanclip._tcp.local.`, TXT `id`, `n`, `v`). Toda conexão começa com o preâmbulo `RCLP` + versão (1) + modo (1 = pareamento, 2 = sessão). Quadros: `u32 BE tamanho` + corpo (máx. 8 MiB).

## Pareamento (modo 1)
Comparação numérica de 6 dígitos com compromisso (commit-reveal), exibida nos dois aparelhos; o usuário confere e aceita nos dois.

1. `p1` (iniciador): nome, id, compromisso `SHA-256("RCLP-commit" ‖ pubI ‖ nI)`.
2. `p2` (respondente): nome, id, chave pública R, nonce R.
3. `p3` (iniciador): revela chave pública I e nonce I; o respondente confere o compromisso.
4. Os dois derivam `T = SHA-256("RCLP-pair" ‖ idI ‖ idR ‖ pubI ‖ pubR ‖ nI ‖ nR)`, o código `sas = u32 BE(SHA-256("RCLP-sas" ‖ T)[0..4]) mod 10^6` (6 dígitos) e a chave do par `HKDF-SHA256(salt = T, ikm = ECDH, info = "RCLP pair key")`.
5. Depois de aceitar, `p4`/`p5` trocam `HMAC(chave, "RCLP-confirm" ‖ papel ‖ T)`; só então a chave é gravada.

Um MITM obtém chaves diferentes em cada lado, logo códigos diferentes; o compromisso impede escolher a chave depois de ver a do outro. Recusa de qualquer lado falha os dois. O respondente só aceita pareamento com a janela aberta (`open_pairing`).

## Sessão (modo 2)
- `S1{id, n}` (cliente → servidor, claro): id do cliente e nonce de 16 B. O servidor ignora ids sem chave.
- `S2{id, n}` (servidor → cliente): id e nonce.
- Chaves: `HKDF-SHA256(chave do par, salt = nC‖nS, info = "RCLP session v1")` → uma chave AES-256-GCM por direção.
- Quadros seguintes: AES-256-GCM, nonce = 4 bytes zero + contador BE de 64 bits (por direção, implícito), AAD = `RCLP` + direção. Repetição, reordenação, adulteração ou chave errada → conexão encerrada.
- Corpo decifrado: `0x01` + JSON (`Msg`) ou `0x02` + id(16) + offset(u32 BE) + dados (chunk de até 256 KiB).

## Mensagens
`inv {items:[[id,ts]]}` · `want {ids}` · `clip {id,ts,kind,mime,name,size,live,text?}` · `ping` · `pong` · `bye {revoked}`.

- Id do item = `SHA-256(kind ‖ name ‖ conteúdo)[0..16]` em hex. O receptor recalcula e descarta o que não bate.
- Ao conectar, cada lado envia `inv` (últimos 200 itens vivos); o outro pede com `want` o que lhe falta ou está mais novo.
- `live=true`: cópia nova, o receptor aplica na área de transferência. `live=false`: reposição de histórico, só guarda.
- Imagem/arquivo: `clip` com `size`, seguido dos chunks; o receptor monta e confere o hash.
- Apagar deixa lápide (`deleted_at`) para o item não ser ressincronizado. Duas conexões simultâneas: fica a aberta pelo aparelho de menor id.
- Conteúdo sensível, apps ignorados, pausa e limites de tamanho são filtrados na origem e no destino.
