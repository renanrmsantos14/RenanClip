# Estado: app-completo

Nível 2 (feature grande). Branch `feature/app-completo`. PRs empilhados: 1 núcleo (#1), 2 PC (`feature/app-pc`), 3 Android (`feature/app-android`), 4 release.

## Etapa atual
PR2 (PC): pronto para PR como rascunho (Windows e autoupdate real não provados).

## Provas
- Núcleo: 24 unitários + 11 de integração + vetores; desktop: 4 testes.
- Linux (Xvfb, duas instâncias): pareamento real com mesmo código, texto/imagem/arquivo nos dois sentidos, reconexão, atalho global, colagem automática, busca sem acento, tema escuro, versão em todas as telas.
- RAM parada do motor: ~19 MB anônimos / ~40 MB PSS (< 50 MB), sem crescer após abrir/fechar a janela.

## Não provado
Windows (sem MSVC aqui) · autoupdate ponta a ponta (precisa de release assinado) · bandeja em ambiente real · Android em aparelho.

## Falta
PR3 Android · PR4 release (`workflow_dispatch`, secrets de assinatura fora do repo).
