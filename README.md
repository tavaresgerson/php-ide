# phpide

IDE de desenvolvimento em **Rust**, com foco em **PHP**, para Linux (X11/Wayland).
Construído do zero sobre código aberto — sem nenhum componente proprietário.
Arquitetura inspirada nos princípios do guia `../docs/como-construir-um-ide.md`.

## Executar

```bash
cd phpide
cargo run --release -- /caminho/do/projeto-php [arquivos...]
```

(sem argumento, abre a **Welcome Screen**; arquivos extras são abertos em abas).
Atalhos: `Ctrl+S` salva, `Ctrl+Z`/`Ctrl+Shift+Z` desfaz/refaz.

## Estrutura (workspace)

| Crate | Responsabilidade | Equivalente clássico |
|---|---|---|
| `phpide-core` | `Document` (rope), `Buffer` (aba + undo), `Workspace`, árvore de arquivos, diagnósticos, offsets (byte/char/UTF-16), revisões, jobs, settings versionados, erros/logs | Document/FileEditorManager/ProblemsView |
| `phpide-php` | Linguagem: metadados PHP, highlight (syntect), tipos de símbolo | Language/ParserDefinition |
| `phpide-lsp` | Cliente LSP: framing JSON-RPC, threads, sync de documentos, completion | LanguageServerManager |
| `phpide-syntax` | Análise estrutural tree-sitter: blocos, erros de sintaxe offline | LocalFileSystem/Folding (offline) |
| `phpide-dbgp` | Protocolo DBGp (Xdebug): breakpoints, run, pilha, variáveis | XDebugger/DBGp driver |
| `phpide-exec` | Processos externos: descoberta configurável de executáveis, spawn padronizado, timeout, Stop e kill de grupo, stdout/stderr em streaming | Execution infrastructure |
| `phpide-run` | Run Configurations versionadas, sessões (process list), PHPUnit (escopos + árvore TeamCity) e cobertura Clover | RunManager/PhpUnit runner |
| `phpide-vcs` | Git como provedor do editor: detecção de repo, diff, stage/commit, log, line markers e blame (wrap do `git` CLI) | GitIntegration/VcsLog/Annotate |
| `phpide-ui` | Shell gráfica egui/eframe: abas, painéis, editor, execução, plugins | IdeFrame |

## Roadmap por estágios

- [x] **Estágio 0 — Fundação**: workspace Cargo, 3 crates, build verde no Linux.
- [x] **Estágio 1 — Editor MVP**: rope com edições mínimas (diff prefixo/sufixo), abas com
      dirty/undo/redo, árvore de arquivos com colapso (ignora `.git/vendor/node_modules`),
      highlight PHP via syntect, salvar com Ctrl+S, barra de status com Ln/Col.
- [x] **Estágio 2 — Shell de IDE**: command palette com fuzzy match (Ctrl+P), busca em
      arquivos em thread separada (Ctrl+Shift+F), painel de problemas (Ctrl+Shift+M),
      temas claro/escuro (hoje: Quick Switch Scheme), zoom (Ctrl+= / Ctrl+- / Ctrl+0), watcher de arquivos
      (recarrega buffers sem edição local, atualiza a árvore).
- [x] **Estágio 3 — Inteligência PHP**: crate `phpide-lsp` (cliente LSP próprio, JSON-RPC
      sobre stdio, sem runtime async) rodando **intelephense** ou **phpactor** como processo
      externo: diagnostics no painel de problemas, ir para definição (F12), outline
      (documentSymbol) no painel Estrutura, sync didOpen/didChange/didSave/didClose,
      indicador de status na barra.
- [x] **Estágio 3.1 — Completamento e estrutura**: popup de completamento LSP
      (Ctrl+Espaço; gatilhos `$`, `->`, `\`) com campo de filtro próprio; crate
      `phpide-syntax` (tree-sitter-php) com blocos estruturais, saltos Ctrl+[ / Ctrl+]
      e diagnósticos de sintaxe offline quando o LSP está fora.
- [x] **Estágio 4 — Executar & depurar**: painel Execução (thread + canal) para
      `php arquivo.php` (Ctrl+R), scripts do `composer.json`, PHPUnit; crate
      `phpide-dbgp` com o protocolo DBGp do Xdebug (escuta porta 9003, breakpoints F9,
      run, pilha e variáveis ao parar, detach) — validado com um "xdebug falso" em socket.
- [x] **Estágio 5 — Extensões**: plugins por script (`plugin.json` em
      `.phpide/plugins/` ou `~/.config/phpide/plugins/`) com comandos na palette;
      tema e zoom persistentes em `settings.json`. WASM fica como evolução documentada.
- [x] **Estágio 6 — Shell de IDE (paridade visual New UI, REQUISITOS Parte 1)**:
      window header unificado (main menu ☰, widget de Projeto, widget VCS com
      branch git, widget Run); tool windows com nomes/atalhos idênticos ao
      PhpStorm (Project `Alt+1`, Find `Alt+3`, Run `Alt+4`, Debug `Alt+5`,
      Problems `Alt+6`, Structure `Alt+7`, Hide All `Ctrl+Shift+F12`) + tool
      window bars laterais e base; status bar com widgets (PHP Language Level,
      Ln:Col → Go to Line, LF/CRLF, UTF-8, indent, branch VCS, ☎ debug listen,
      read-only, RSS); Search Everywhere (`Shift×2`), Go to File (`Ctrl+Shift+N`),
      Recent Files (`Ctrl+E`), Find Action (`Ctrl+Shift+A`, ou `>` no campo);
      Quick Switch Scheme (``Ctrl+` ``) com temas Islands Dark/Light/Darcula/Light;
      Welcome Screen (Projects/Customize, projetos recentes, browser de pastas,
      New Project com esqueleto composer.json); editor tabs com preview (itálico),
      pin, Close Others, Closing Policy de 10; breadcrumbs estruturais clicáveis
      (tree-sitter, offline); F2/Shift+F2 entre problemas; Alt+←/→ no histórico
      de navegação; debug zero-config (listener DBGp + `XDEBUG_CONFIG` automático);
      Stop `Ctrl+F2`. A especificação de referência está em `REQUISITOS.md`.
- [x] **Estágio 6.1 — Paridade visual da shell (REQUISITOS Parte 1)**:
      catálogo completo de tool windows e atalhos (`Alt+1…0`, `Alt+F12`,
      `Ctrl+Alt+3`), More tool windows, Hide Active `Shift+Escape`, painéis
      Bookmarks/Services/Git/Commit/Terminal/Database/Notifications/Remote Host;
      Settings `Ctrl+Alt+S` com a árvore oficial e páginas funcionais de aparência,
      keymap, editor e PHP; Navigation Bar inferior/topo; status bar com Processes,
      Column Selection Mode e deployment; Welcome Screen com Projects/Customize/
      Plugins/Learn/Remote Development; sete temas oficiais; Compact Mode e
      importação de configurações. O Terminal executa comandos no shell do projeto.
- [x] **Estágio 6.2 — Superfície do editor (REQUISITOS 1.4)**:
      gutter com números de linha e **breakpoints sobre os números** (New UI),
      marcadores de bookmark (F11) na borda do gutter, destaque da linha do
      cursor e clique no gutter para alternar breakpoint; **sticky lines**
      (máx. 5 declarações estruturais fixadas no topo ao rolar, clicáveis,
      indentadas por profundidade — namespace → class → método → bloco);
      **faixas de inspeção na scrollbar** (erros/avisos/info do arquivo ativo
      pintados na borda direita na altura da linha). Implementado sobre a
      geometria exata da galley do `TextEdit` (`TextEditOutput.galley_pos` +
      `Row.rect`), sem widgets irmãos nem drift de rolagem. Hook de demo:
      `PHPIDE_SCROLL_TO_LINE=<n>` pré-seleciona o scroll para capturas.
      Folding/blame/coverage, inlay hints/code vision, lens mode e split/detach
      de editor seguem pendentes; database neste estágio é a shell — conexões e
      query console pertencem ao RF-DB.
- [x] **Consolidação transversal NFR (marco 1 do plano — TAREFAS.md §3)**:
      infraestrutura de concorrência, documentos, processos, settings, erros
      e confiabilidade que os recortes ativos consomem. Entregas:
      - **Jobs** (`phpide-core::jobs`, NFR-001): `JobSystem`/`JobHandle` com
        ID, estados (running/completed/cancelled/failed), retorno por canal,
        cancelamento cooperativo, descarte de resultados obsoletos por chave
        (`JobRegistry`) e `shutdown` ao trocar de projeto ou fechar a IDE
        (busca em arquivos já roda sobre isso).
      - **Posições e revisões** (NFR-002): conversões centralizadas
        byte/char/linha/UTF-16 com negociação de encoding (`offsets.rs`,
        CRLF e astrais testados) e invalidação por revisão (`Revisioned`,
        `PerFileRevision`); diagnósticos só valem para a revisão em que
        foram calculados.
      - **Processos externos** (crate `phpide-exec`, NFR-003): descoberta
        configurável (env/settings antes do PATH), spawn por vetor de
        args com cwd/env, stdout/stderr/exit separados, timeout, Stop e
        encerramento do grupo de processos; erros distintos e acionáveis
        (ausente/permissão/versão/path com espaços) com matriz de falhas em
        testes de integração.
      - **Settings versionados** (NFR-004): `settings_version` com migrações
        incrementais (v1 legado migrado), escrita atômica com `.bak` e
        recuperação de JSON inválido (original preservado como `.invalid`).
      - **Erros e logs** (NFR-005): `IdeError` com componente/operação/ação,
        logger com correlação por sessão/job/documento e redaction de
        segredos (tokens, senhas, URIs, Bearer) testada com casos adversariais.
      - **Confiabilidade** (NFR-007): save atômico (tmp+fsync+rename, buffer
        preservado em erro) e confirmação de buffers sujos ao fechar aba,
        trocar de projeto ou sair da IDE.
      - **Baseline de performance** (NFR-006): fixtures determinísticos
        (10k linhas, projeto grande) e benchmarks reproduzíveis com
        resultados em `docs/baseline/T-NFR-006-02-perf-baseline.md`.
      Baselines de fmt/clippy do código legado registrados em
      `docs/baseline/` (sem formatação em massa fora do escopo).
- [x] **Baseline e geometria do editor (UI-ED-000, T-UI-ED-000-01…04)**:
      mapa centralizado documento → linha visual → coordenada em
      `phpide-ui::geometry` (puro, sem egui; `EditorGeometry` é wrapper no
      espaço de tela) — gutter, viewport, clamp/máximo de scroll, hit tests,
      `scroll_to_line` com margem e coluna visual de tabs; cobertura UT de
      arquivo vazio, Unicode, tabs, linhas longas, zoom e largura de gutter
      por dígitos; baseline de scroll medido no fixture de 10 mil linhas
      (`benches/viewport.rs`: ~7.5 ns/frame de geometria) registrado em
      `docs/baseline/T-UI-ED-000-01-editor-baseline.md`. Verificação do modo
      offline (highlight syntect + diagnósticos tree-sitter sem LSP) e gate
      de fluxo abrir→editar→desfazer→salvar→fechar cobertos por testes
      (`phpide-core/tests/editor_flow.rs`, `gate_tests` no binário).
- [x] **Tabs e zoom consolidados (T-UI-ED-003-01, T-UI-ED-010-01)**:
      Closing Policy configurável (`settings.tab_limit`, default 10, clamp
      1–99) aplicada ao `Workspace`; auditoria de preview/pin/Close
      Others/dirty confirmation/tabs acessíveis em
      `docs/baseline/T-UI-ED-003-01-tabs-audit.md`; `clamp_zoom` central
      (0.5–3.0, NaN → 1.0) com UT e roundtrip de persistência.
- [x] **Estágio 7.x — Projeto Composer e inteligência PHP (T-PHP-030/001/002/003/004/005)**:
      descoberta declarativa de `composer.json` (`phpide-php::composer`:
      constraint PHP → language level, autoload PSR-4/PSR-0 → source/test
      roots, vendor ausente vira aviso acionável — sem install automático;
      re-descoberta pelo watcher por mtime); cliente LSP com capabilities
      negociadas no `initialize` (recursos habilitados por capability, nunca
      supostos), estados Starting/Running/Failed com crash real vs. parada
      voluntária, restart pela palette, sincronização planejada por frame
      (didOpen→didChange→didClose, versões monotônicas) e descarte de
      respostas de sessão/revisão antigas; completion com cancelamento
      (`$/cancelRequest`), aceite via `textEdit`/`insertText` em ranges
      UTF-16 pelas conversões centralizadas (Unicode/CRLF testados) e
      documentação no popup; navegação definition/implementation/
      typeDefinition por capability, Go to Symbol (workspace/symbol) e Find
      Usages (references → painel Find com preview); semantic tokens
      (delta encoding decodificado, cores por tipo sobre o highlight
      sintático, cache por revisão) e ocorrências com fallback sintático
      offline; Problems com escopo projeto/arquivo e resumo por ferramenta.
      **Gate real**: `crates/phpide-php/tests/composer_flow.rs` sobe um
      projeto Composer em disco com intelephense real — descobrir, abrir,
      editar, diagnosticar, completar (`greet`), navegar (definition em
      `Greeter.php`) e salvar, sem derrubar a sessão.
- [x] **Estágio 8.x — Execução, depuração, testes e cobertura (T-RUN-001/002,
      RUN-DBG-001/002/003, RUN-TST-001/002/003)**:
      - **Run Configurations** (`phpide-run`): modelo versionado com
        validação de campos (erros bloqueiam com ação, avisos não), tipos
        PHP Script / Composer Script / PHPUnit; sessões no painel Run com
        **process list**, **Rerun**, **Stop** (kill de grupo),
        **Pause Output** e **Pin Tab**; env com **redaction obrigatória**
        nos logs; allow multiple instances.
      - **Xdebug/DBGp** (`phpide-dbgp`): listener 9003 com **IDE key**
        (conexão errada é recusada com detach), `XDEBUG_CONFIG` montado
        **mesclando o env do usuário** (nada de apagar `mode=` etc.),
        **path mappings** local↔remoto com associação da conexão ao projeto
        (Foreign exige confirmação), breakpoints com estados
        (Pending/Verified/Invalid/Disabled) sincronizados com o backend,
        **Resume F9, Step Into F7, Over F8, Out Shift+F8, Show Execution
        Point Alt+F10**, **Watches e Evaluate por frame** (invalidados após
        Resume), e **Stop/Detach confiáveis** durante o run (socket clonado
        é fechado na hora; processo PHP morto em grupo; porta liberada).
      - **PHPUnit** (`phpide-run::phpunit`): runner resolvido
        (vendor/bin → phar → PATH), consolidação de `phpunit.xml`/bootstrap
        (explícito vence sempre), execução por **diretório, classe, método
        e data set**, árvore de resultados com **duração, output e falhas
        navegáveis** (arquivo:linha) via parser TeamCity.
      - **Cobertura** (`phpide-run::coverage`): modelo por arquivo/linha
        executável, importador Clover (`--coverage-clover` do PHPUnit com
        Xdebug/PCOV), percentual por arquivo e faixa no gutter do editor —
        linha fora do relatório nunca aparece como coberta.
      - **Gate real** (`phpide-run/tests/gate_run_debug.rs`): executa um
        script PHP real, depura com processo PHP real + engine DBGp — gate
        duplo: peer protocolar e **Xdebug 3.2.0 real** (extensão carregada
        sem root, `.so` de `.deb` Debian — bugs protocolares achados e
        corrigidos, ver `docs/baseline/T-RUN-ESTAGIO8-run-debug-test.md`) —,
        testa com PHPUnit 11 real e valida Stop/Detach confiáveis.
- [x] **Estágio 10.0 — Git como provedor do editor (GIT-001/002/003/004)**:
      - **Detecção** (`phpide-vcs::repo`): worktree (root resolvido), branch
        atual, **HEAD desanexada** (header mostra `(detached <sha>)`) e
        repositório inválido/bare como erro recuperável com ação (nada
        bloqueia a UI).
      - **Diff e commit** (`phpide-vcs::diff`/`ops`): diff de working tree,
        staged e vs HEAD (parser unified; arquivo untracked vira diff
        sintético 100% adicionado); stage/unstage por arquivo com status
        porcelain real pós-operação; commit que **não executa com mensagem
        vazia**, preserva o **output de hooks** no erro e mantém o **índice
        intacto** quando um hook recusa.
      - **Log** (`phpide-vcs::log`): commits com arquivos (`--name-status`),
        filtros de quantidade, path e revisão.
      - **Mudanças no editor** (`phpide-vcs::markers`/`blame`): line markers
        Added/Modified/Removed derivados do diff (pares del+add viram
        Modified; stage mantém, commit limpa, sem cache a invalidar) e
        blame por revisão com navegação ao commit (`log(rev=sha)`) —
        prontos para o gutter (renderização em UI-ED-005-06/006-01/007-01).
      - **Gate real** (`phpide-vcs/tests/gate_git.rs`): repositório
        temporário com o `git` do sistema — diff, stage seletivo, commit,
        falha de hook, markers e blame de ponta a ponta.

## Atalhos

| Tecla | Ação |
|---|---|
| Shift×2 / Ctrl+P | Search Everywhere |
| Ctrl+Shift+N / Ctrl+E / Ctrl+Shift+A | Go to File / Recent Files / Find Action |
| Ctrl+S / Ctrl+Z / Ctrl+Shift+Z | salvar / desfazer / refazer |
| Ctrl+N | novo arquivo |
| Ctrl+F4 | fechar aba |
| Alt+1…0 | Project, Bookmarks, Find, Run, Debug, Problems, Structure, Services, Version Control, Commit |
| Alt+F12 / Ctrl+Alt+3 | Terminal / Database |
| Shift+Escape / Ctrl+Shift+F12 | Hide Active / Hide All Tool Windows |
| Ctrl+Alt+S | Settings |
| F11 | Toggle Bookmark |
| Alt+Shift+. / Alt+Shift+, | aumentar / reduzir fonte da IDE |
| Ctrl+Shift+F | busca em arquivos (tool window Find) |
| F2 / Shift+F2 | próximo / anterior problema |
| Alt+← / Alt+→ | navegação back/forward |
| Ctrl+Espaço | completamento (e gatilhos `$`, `->`, `\`) |
| F12 / Ctrl+B | ir para definição |
| Ctrl+[ / Ctrl+] | início/fim do bloco estrutural |
| Ctrl+R / Ctrl+D | executar arquivo atual / debug com Xdebug |
| Ctrl+F2 | parar execução/debug (Stop confiável) |
| F9 | breakpoint na linha do cursor; **Resume** com sessão de debug ativa¹ |
| F7 / F8 / Shift+F8 | Step Into / Step Over / Step Out (com debug pausado) |
| Alt+F10 / Alt+F8 | Show Execution Point / Evaluate por frame (painel Debug) |

¹ Atalhos de debug são **contextuais**: com sessão DBGp ativa, F9/F7/F8/
Shift+F8/Alt+F10 controlam o debugger; fora de sessão, F9 mantém o toggle
de breakpoint do baseline. A migração global da keymap (conflitos em
`REQUISITOS.md` 6.7) é um requisito próprio.
| Ctrl+` | Quick Switch Scheme |
| Ctrl+= / Ctrl+- / Ctrl+0 | zoom |

## Servidor de linguagem (LSP)

A IDE procura o servidor nesta ordem (`crates/phpide-lsp`):

1. Variável de ambiente `PHPIDE_LSP` (ex.: `PHPIDE_LSP="phpactor language-server"`)
2. `third_party/node_modules/.bin/intelephense` — instale localmente com:
   `npm --prefix third_party install intelephense`
3. `intelephense` no PATH
4. `phpactor` no PATH (open source, MIT — alternativa ao intelephense)

Verificação rápida do stack LSP (sobe intelephense real e espera diagnósticos):

```bash
cargo test -p phpide-lsp
```

Gate do workspace (com baselines legado registrados em `docs/baseline/`):

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
cargo bench -p phpide-core --bench doc_ops      # baseline NFR-006
cargo bench -p phpide-ui  --bench search_large
```

## Decisões técnicas do Estágio 1

- **Rope** (`ropey`): edições e medição de linha em O(log n); o `TextEdit` do egui devolve
  o texto inteiro, então reduzimos a diferença a `(offset, removidos, inserido)` e aplicamos
  na rope — a String é só o adaptador do widget.
- **Undo por snapshots** com teto de 128 estados (simples e correto; otimizar para
  operações invertíveis no Estágio 2).
- **Highlight** com syntect (lexer declarativo estilo Sublime). No Estágio 3, análise
  semântica (LSP) sobrepõe cores por uso, como nos IDEs clássicos.
- **Sem threads ainda**: tudo síncrono e pequeno. LSP e `notify` introduzem async no Estágio 2/3.

## Licença

MIT OR Apache-2.0 (código próprio). Dependências sob suas próprias licenças (ver `cargo tree`); intelephense é freeware (instalado localmente via npm, não redistribuído).
