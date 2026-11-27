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

## App de PC: Tauri 2 com janela sob demanda
Parado, o app é só o motor (Rust) e a bandeja; a janela (webview) é criada ao abrir e destruída ao fechar. Isso é o que mantém a RAM parada baixa (meta < 50 MB): uma webview aberta sozinha passa disso. Trade-off: abrir leva ~200 ms a mais que uma janela escondida.

## Área de transferência no Linux: X11 por XFixes, sem polling
O dono da seleção `CLIPBOARD` avisa a troca por evento (XFixes); zero CPU parado. Ignora o que o próprio app aplicou (pelo PID do dono). Wayland puro não é suportado nesta versão (o Cinnamon do Renan é X11). Windows usa o número de sequência da área de transferência a cada 350 ms.

## Tailscale só como rota
Fora de casa os aparelhos se acham pela tailnet, mas a segurança continua sendo a do RenanClip (ECDH + comparação de código + AES-GCM por par). Confiar só no Tailscale deixaria qualquer máquina da tailnet ler o histórico.

## Atualização automática com assinatura própria
`SHA256SUMS` assinado com Ed25519 (chave pública de 32 bytes embutida em `desktop/updater.pub`; a privada fica fora do repo). Sem assinatura válida o app não instala nada, mesmo que alguém publique um release falso.

## Seletor de colagem fecha ao perder o foco
Aberta pelo atalho global, a janela fecha ao perder o foco e ao colar; aberta pela bandeja, fica até você fechar.

## Janela em processo filho
Um webview residente custava ~150 MB e não devolvia RAM ao fechar a janela. O motor fica sem webview e a UI vira processo filho que morre ao fechar; o RPC é JSON por linha no pipe. Custo: ~300 ms para abrir a janela. O seletor (atalho global) usa o mesmo caminho.
