# Pendências, bugs e problemas para revisão

> Registro vivo das pendências conhecidas. Atualizado em **2026-11-07**
> (após a Fase 8 / v0.13.0). Convenção: **[BUG]** comportamento errado
> confirmado · **[LIM]** limitação consciente do recorte · **[DIV]** débito
> técnico · **[AMB]** problema de ambiente (não de código) · **[PROC]**
> processo/infraestrutura do repositório.
>
> Antes de marcar algo como resolvido, o gate do workspace precisa estar
> verde (testes, fmt, clippy) no estado entregue.

## 1. Bugs conhecidos (não corrigidos)

| # | Sev | Onde | Descrição |
|---|---|---|---|
| B1 | média | `phpide-ui/src/main.rs` (`poll_lsp`, bloco de inlay hints) | O gatilho de inlay hints foi estendido para admitir arquivos FE, mas o bloco inteiro vive dentro de `poll_lsp`, que só roda com o servidor PHP ativo. Em projeto **sem** intelephense (só TS), hints nunca são solicitados — FE-002-02 fica parcialmente morto nesse cenário. Mover o gatilho de hints para `poll_fe` ou para um poll neutro. |
| B2 | baixa | `phpide-db/src/drivers/sqlite.rs` (`DbSession::cancel`) | `cancel()` é no-op: o cancelamento real acontece pela destruição da sessão (`CancelHandle` fecha a conexão). Funciona, mas viola a expectativa do contrato — implementar `get_interrupt_handle` (rusqlite) guardado no `SqliteSession`. |
| B3 | média | `phpide-db/src/console.rs` (`run_script`) | Script com N statements aplica sequencialmente sem transação implícita: um erro no statement k deixa os anteriores **persistidos**. O aceite DB-006-05 pede erro parcial controlado — rodar scripts multi-statement dentro de uma transação implícita (ou documentar semanticamente). |
| B4 | baixa | `phpide-db/src/grid.rs` (`paginate`) | `GridPage.total` é sempre `None` (nenhum COUNT paralelo) — a grade não sabe o total real de linhas, então paginação "página x de y" não existe. Adicionar `SELECT COUNT(*)` com o mesmo WHERE quando `filter` está ativo. |
| B5 | média | `phpide-db/src/edit.rs` + drivers | Concorrência otimista usa só contagem de linhas afetadas; o **valor antigo** (`CellChange.old`) não entra no WHERE. Duas sessões editando a mesma célula sobrescrevem sem detecção. Aceite parcial de DB-006-05 — incluir `old` na condição quando não-NULL. |
| B6 | baixa | `phpide-db/src/secrets.rs` (`open_secret_store`) | Se `FileKeyring::open()` falhar (permissão/disco), cai num keyring em memória que **perde segredos silenciosamente**. Deve sinalizar à UI que segredos não persistirão nesta sessão. |
| B7 | baixa | `phpide-ui` — markers Git da scrollbar | O cache de markers Git (`git_markers`) só recarrega com buffer **limpo**; durante edição, os markers ficam stale **sem sinalização** (o blame gutter marca "stale", os markers não). Reaproveitar a mesma sinalização. |
| B8 | baixa | `phpide-ui/src/main.rs` (lens preview) | O popup do lens preview ancora num ponto fixo (`Align2::RIGHT_TOP, (-24,80)`) em vez de seguir o ponteiro — preview "por região" aproximado. Usar `painter` em coordenada de tela do hover. |
| B9 | média | `phpide-lsp` (edits.rs) + `phpide-ui` (`WsStore::lsp_version`) | Conflitos de versão de WorkspaceEdit **nunca são reportados**: `WsStore::lsp_version` devolve `None` sempre, e os buffers não rastreiam versão LSP por arquivo. A infra existe (`DocumentEdits.version` → `conflicts`), falta o rastreamento. |
| B10 | baixa | `phpide-dbgp/src/engine.rs` (documentado no baseline) | Janela residual: Detach/Stop no instante exato em que um `run` desbloqueou mas a fase ainda não virou `Stopped` degrada para `ConnectionLost` em vez de `Detached/Stopped`. Impacto pequeno (o RunManager mata o processo no Stop de qualquer forma). |

## 2. Limitações de recorte (por fase)

### Estágio 8 — Execução/depuração/testes (RUN-*, RUN-DBG-*, RUN-TST-*) — v0.8.0
- **[LIM]** `eval` avalia no topo da pilha (restrição do protocolo DBGp; contornado com `property_get -d` para variáveis por frame).
- **[LIM]** T-RUN-001-04 (before-launch tasks), T-RUN-DBG-003-02/03 (conditional/exception/logpoint completos, grupos/mute/triggers), T-RUN-TST-001-03 (Rerun Automatically), RUN-DBG-004 (sessões web/remotas, proxy SSH): fora do recorte planejado em TAREFAS.md.
- **[LIM]** Migração global do keymap (conflitos REQUISITOS 6.7) é requisito próprio — hoje F9 é contextual (Resume com sessão ativa / breakpoint fora).
- **[AMB]** Xdebug carregado via `.so` extraído de `.deb` (sem root). Em máquinas com a extensão instalada, tudo funciona igual; sem nenhum dos dois, os testes `*_xdebug_real` se autopulam.

### Fase 5 — Git (GIT-001..004) — v0.10.0
- **[LIM]** GIT-001-02 (multi-root/refresh incremental), GIT-002-04/05/06 (stage por chunk, changelists/amend, commit inline), GIT-003-02/03/04 (branches popup, fetch/pull/push, force-with-lease), GIT-005 (merge/rebase/stash), GIT-006 (GitHub/GitLab): pendentes conforme TAREFAS.md.
- **[LIM]** Renderização dos markers/blame no editor é das tarefas UI-ED-* (a entrega da Fase 5 foi o provedor + wiring do header).

### Fase 6 — Superfície do editor (UI-ICON, UI-ED-001..007) — v0.11.0
- **[LIM]** Split Right/Down: o segundo painel renderiza **empilhado**; o eixo fica registrado no modelo `EditorGroups` para o layout lado a lado (UI-ED-003-04 parcial).
- **[LIM]** UI-ICON-001-06/07 (migração de menus/tool windows/status bar por lotes e validação offline de contraste) pendentes.
- **[LIM]** UI-ED-003-05/06/07 (drag/drop de tabs, avaliação de multi-janela, detach Shift+F4), UI-ED-008 (caret/occurrences/guides/separators), UI-ED-009 (quick-fix/intentions) pendentes.
- **[DIV]** egui não rasteriza SVG: cada ícone tem um pintor vetorial espelhando o SVG canônico em `icons_ui.rs`. Adicionar ícone novo exige editar dois lugares (UT `every_icon_id_has_prims` protege).

### Fase 7 — Frontend (FE-001/002) — v0.12.0
- **[BUG]** B1 acima (hints FE sem servidor PHP).
- **[LIM]** FE-002-02 "Project Errors": diagnósticos agregados no Problems vêm dos arquivos **abertos** (publish model); o pull workspace-wide (`workspace/diagnostic`) fica pendente de servidor que publique projeto inteiro.
- **[LIM]** FE-002-03 (TypeScript-Go), FE-003 (React/Angular/Vue/Tailwind), FE-004 (debug Node/Chromium), FE-005 (CSS avançado): pendentes.
- **[AMB]** Servidores (`typescript-language-server`, `vscode-langservers-extracted`) declarados em `third_party/package.json`, mas o `node_modules` é gitignored — máquina nova precisa `npm install` em `third_party` (documentado na tag v0.12.0). npm 11 emite warning de `install-scripts` (postinstall bloqueado) — funciona, mas revisar se algum servidor passar a precisar de postinstall.
- **[AMB]** `tsserver` resolvido por `--tsserver-path` com caminho relativo frágil (`parent().join("../lib/tsserver.js")` em `fe_servers.rs`) — funciona com a instalação atual do npm; quebra se a layout do pacote mudar. Derivar de `require.resolve` via `node -p`.

### Fase 8 — Banco de dados (DB-001..006) — v0.13.0
- **[AMB]** **PostgreSQL/MySQL sem suporte anunciado**: contratos prontos, mas nenhum servidor instalável sem root neste ambiente (clientes `psql`/`mysql` presentes, servidores ausentes). Drivers falham com diagnóstico claro; o gate "nos três bancos" fica **parcialmente bloqueado por ambiente**. Quando houver servidor: os ITs existentes passam sem tocar na UI.
- **[LIM]** UI toolwindow (render egui de explorer/grade/console) **não implementada** — modelo e comportamentos cobertos por testes; a renderização é a próxima camada (DB-003-02/03, DB-005-02 em nível de UI).
- **[LIM]** DB-005-05 (resultsets grandes nos três drivers) validado apenas em SQLite/fake.
- **[DIV]** Erros do domínio DB usam `Component::Core` (não existe `Component::Db` no catálogo NFR-005) — avaliar extensão do enum com teste.
- **[DIV]** Onda 3 do roadmap DB (diagramas, compare, execution plans, dumps, mais SGBDs) não planejada neste documento.

## 3. Débitos técnicos transversais

| # | Onde | O que |
|---|---|---|
| D1 | `phpide-ui/src/main.rs` (~4.900 linhas) | O app concentra demais responsabilidades. Extrair: `EditorSurface` (pintura), `LspRouting` (seleção PHP×FE repetida inline em 4 lugares — virou padrão por borrow de campo, candidato a macro ou struct de contexto), `update()` (dispatcher de polls). |
| D2 | `phpide-lsp/src/proto.rs` | Parser DBGp por `attr()` com busca em janela de texto — robusto para o Xdebug 3.2 validado, mas quebraria com namespaces prefixados ou atributos repetidos. XML real (quick_xml) com testes de regressão dos formatos reais capturados. |
| D3 | `phpide-ui/src/folding.rs` (`commit`) | Diff linha-a-linha do texto projetado: edições que tocam várias linhas próximas são mapeadas como hunk único; edições dentro de strings com `\n` literal podem deslocar o mapeamento. Aceitável para digitação; revisar antes de habilitar refatorações sobre arquivo dobrado. |
| D4 | `phpide-vcs` | Wrap do `git` CLI com parse de texto (porcelain). Adoção de `gitoxide`/`git2` avaliada e adiada por consistência com a política de ferramentas reais — mas porcelain é instável entre versões do git; travar a versão mínima suportada nos testes. |
| D5 | `crates/phpide-db` | `update_row` monta SQL por formatação (quoting de identificador, não de valor — valores são bind). Seguro hoje; invariante merece UT de injeção por nome de tabela/coluna vindo de introspection. |
| D6 | Testes | `composer_flow` depende de intelephense local (gitignored) — com ele presente, flakiness de indexação mitigada por poll; sem ele, autopula. Manter os dois modos cobertos no CI. |
| D7 | `Cargo.lock` | Ignorado pelo `.gitignore` desde o baseline do repositório (decisão original). Com 9 crates e deps pesadas (rusqlite bundled, tree-sitter), reconsiderar versionar o lock para builds reproduzíveis — decisão do mantenedor. |

## 4. Ambiente e processo

| # | Tipo | O que |
|---|---|---|
| A1 | **[AMB]** | Sem `gh`/token nesta máquina: releases do GitHub são criadas manualmente a partir da tag (URLs prontas em cada entrega). Instalar `gh` + `gh auth login` remove o passo manual. |
| A2 | **[AMB]** | Capturas de tela exigiram backend X11 + Lavapipe (`env -u WAYLAND_DISPLAY WINIT_UNIX_BACKEND=x11 VK_ICD_FILENAMES=…lvp_icd.json`); no Wayland puro a janela não mapeia para X11 e o DBus de screenshot do GNome está bloqueado. Scriptar `docs/screenshots/regen.sh` se as capturas forem rotineiras. |
| A3 | **[PROC]** | `cargo fmt --all` formata arquivos legados fora do recorte — usar `rustfmt --edition 2021 <arquivos>` apenas nos arquivos do incremento (processo adotado desde o Estágio 8; documentar no CONTRIBUTING se existir). |
| A4 | **[PROC]** | Commits/tags seguem o padrão `Feat: Fase N — …` + tag `v0.N.0` anotada com as notas. Releases GitHub (páginas) não publicadas automaticamente (ver A1). |
| A5 | **[AMB]** | PHPUnit phar e `.so` do Xdebug vivem em `target/tmp/` (gitignored) — documentado no baseline do Estágio 8; um `make setup`-like regeneraria (curl do phar; `apt-get download` + `dpkg-deb -x` do php8.2-xdebug). |
| A6 | **[AMB]** | GNOME Shell ≥ 45 bloqueia screenshot DBus automatizado (`AccessDenied`) — alternativa futura: Xvfb (requer sudo) ou portal com aprovação interativa. |

## 5. Falhas pré-existentes fora do escopo (registro)

Nenhuma no momento: `composer_flow` (Estágio 7) foi estabilizado nesta sessão (poll de indexação); os 4 arquivos legados desalinhados no `fmt` foram formatados na Fase do Estágio 8.x; warnings clippy legados zerados. Se algum teste falhar em máquina nova, registrar aqui com o ambiente.

---

### Como usar este documento

1. Antes de iniciar uma fase, varrer as seções 1–2 por itens da fase alvo.
2. Ao encontrar bug novo durante uma entrega: corrigir se barato e seguro; senão, registrar aqui com severidade e referência de arquivo.
3. Ao fechar uma release: revisar se algum item mudou de estado e atualizar a data do cabeçalho.
4. Severidades sugeridas: **alta** = dado incorreto ou funcionalidade morta em cenário comum · **média** = funcionalidade degradada com contorno · **baixa** = polimento.
