# PHP IDE — Diagnóstico e plano de implementação

> Data: 2026-11-23 (atualizado em 2026-11-24: Fase 8 concluída — candidata a v1.0; registro do que ficou para depois na seção 8) · Base: inspeção de `README.md`, `REQUISITOS.md`, `docs/*`, `docs/baseline/*`, capturas em `docs/screenshots/` e código de todos os crates (`phpide-ui` em detalhe).
> No repositório, a versão em inglês deste plano está em `docs/ROADMAP.md` (com o detalhe de cada fase concluída e da lista pós-1.0) e o status de cada requisito de referência do PhpStorm em `docs/REQUIREMENTS.md`.
> Este documento substitui `REQUISITOS.md`, `docs/TAREFAS.md`, `docs/REVISAO_TECNICA_E_TAREFAS.md`, `docs/DESIGN_SYSTEM_UI_UX.md`, `docs/PENDENCIAS.md` e `docs/baseline/*` como referência de trabalho.

---

## 1. Diagnóstico

### 1.1 O problema central

A documentação marcava quase tudo como `[x]` e citava 372 testes verdes, mas os testes provavam **modelos e protocolos**, não a experiência de uso. O projeto cresceu em largura (11 crates, Database Tools com 3 drivers, DBGp, PHPUnit, cobertura, frontend TS) enquanto o **editor — o coração de uma IDE — continuava sendo um `egui::TextEdit` adaptado**. *(Resolvido na Fase 3.)*

### 1.2 Editor (causa raiz técnica) — resolvido na Fase 3

| Achado | Consequência |
|---|---|
| `TextEdit::multiline` sobre uma `String` copiada da rope a cada frame | O(n) por frame |
| Highlight syntect do arquivo inteiro a cada frame, sem cache | Lentidão em arquivos médios |
| Edição detectada por comparação do texto inteiro | Custo O(n) e edições ambíguas |
| Undo por snapshot, sem agrupamento | Ctrl+Z desfazia caractere por caractere |
| Folding projetando texto e reconciliando o `TextEdit` | Frágil por construção |
| Split secundário era outro `TextEdit` cru | Split inacabado |
| Sem multi-cursor, seleção em coluna, bracket matching | Lacunas básicas |

### 1.3 Thread de UI bloqueada *(resolvido na Fase 1)*

Blame, marcadores Git, branch, `git clone` e `scan()` síncronos na thread de UI.

### 1.4 Shell e acabamento visual

- Ícones de tool windows eram letras. *(Fase 2: Tabler.)*
- Status bar com itens falsos e botões com contorno. *(Fases 0, 2 e 4.)*
- Cores literais espalhadas. *(Fase 2: tokens.)*
- Strings em português e inglês misturadas. *(Fase 1.)*
- Navegação e highlight por tipo de arquivo abaixo de uma IDE. *(Fase 4.)*
- Inteligência PHP sem hover, parameter info, rename, quick fixes nem formatação; completion roubava o foco do editor; `didChange` mandava o texto inteiro a cada tecla. *(Fase 5.)*
- Terminal falso (`sh -lc` por comando, sem PTY). *(Fase 6.)* Painel Commit mínimo. *(Fase 7: Commit, diff, branches e log.)*

### 1.5 Arquitetura e processo

- `main.rs` com ~5 000 linhas. *(Fase 1: < 500 linhas.)*
- Atalhos, menus e palette sem registro de comandos comum. *(Fase 1.)*

### 1.6 O que vale preservar

`phpide-core`, `phpide-lsp`, `phpide-dbgp`, `phpide-exec`, `phpide-run`, `phpide-vcs`, `phpide-syntax`, `phpide-icons` e `phpide-db` têm testes reais. **A reforma é da camada de UI e do editor, não uma reescrita do projeto.**

---

## 2. Decisões estratégicas

1. **Manter Rust + egui/eframe** (egui 0.36).
2. **Editor próprio** (`phpide-editor`): rope como fonte de verdade, só linhas visíveis renderizadas, undo agrupado, multi-cursor, highlight incremental com tree-sitter.
3. **Escopo congelado até a v1.0**: Database Tools, frontend TS, Remote Host, Deployment, Services, Docker, frameworks, IA e HTTP Client atrás da feature `experimental`.
4. **Design system antes de tela nova**: tokens, Inter e JetBrains Mono empacotadas, Tabler Icons, componentes-base.
5. **Nada bloqueia a thread de UI.**
6. **Verdade nos gates**: captura + teste de interação + medição.
7. **Idioma da UI**: inglês, sem internacionalização antes da v1.0.
8. **Nome do produto**: PHP IDE; crates e binário com prefixo `phpide`.

---

## 3. Plano passo a passo

Tamanho: **P** (dias) · **M** (1–2 semanas) · **G** (3+ semanas).

### Fase 0 — Baseline honesto e limpeza (P) — concluída

Feature `experimental`, superfícies falsas removidas, `scripts/baseline.sh` e benchmark `editor_pipeline`, docs novos.

### Fase 1 — Fundação da aplicação (M) — concluída

egui 0.36, UI em inglês, command registry único, Git/scan/clone em segundo plano, árvore lazy, `main.rs` < 500 linhas, testes de interação com `egui_kittest`.

### Fase 2 — Design system (M) — concluída

Tokens com teste de contraste WCAG; temas Dark, Light e High Contrast; highlight por papel semântico; fontes empacotadas; Tabler Icons 3.2; componentes; galeria com snapshots; teste que proíbe cor literal fora de `tokens.rs`.

### Fase 3 — Editor novo `phpide-editor` (G) — concluída

Modelo com `ChangeSet` invertível e log de mudanças; multi-cursor; highlight tree-sitter incremental; find/replace; widget virtualizado com gutter em camadas, sticky lines, inlay hints, squiggles, IME; split real. Medição (54k linhas): abrir 9,4 ms; rolagem 0,8 ms/frame; tecla 4,7 ms (antes 1,76 s por frame). Atalhos IntelliJ: Shift+F10 Run, Shift+F9 Debug, Ctrl+R Replace, Ctrl+D Duplicate.

### Fase 4 — Shell polida (M) — concluída em 2026-11-24

Guiada pelo documento de requisitos de referência do PhpStorm; todos os itens MVP de `docs/REQUIREMENTS.md` em ✅. Tipos de arquivo e highlight por tipo; árvore do projeto completa (teclado, speed search, New filtrado, dependências, Copy Reference/URL do repositório); abas (várias linhas, fixadas primeiro, reabrir fechada); Home; janelas de projeto (menu Window, Ctrl+Alt+]/[, reabrir sessão, attach); Search Everywhere com abas e páginas de Settings; Go to Class (Ctrl+N); Color Scheme com herança por linguagem e fonte do sistema; tool windows móveis/flutuantes; header New UI; status bar discreta; Event Log com toasts. Gate de teclado (abrir → editar → salvar → fechar).

### Fase 5 — Inteligência PHP no editor novo (M) — concluída em 2026-11-24

- **Sincronização**: `didChange` com intervalos, gerado do log de mudanças do editor (texto inteiro só quando o log não alcança ou o servidor pede). Toda requisição envia a sincronização antes.
- **Completion**: o teclado continua no editor e a lista filtra enquanto se digita; abre em `->`, `::`, `$`, nos gatilhos do servidor e ao digitar identificadores; selo de letra por tipo; documentação resolvida sob demanda; snippets com Tab/Shift+Tab; imports num único passo de undo.
- **Quick Documentation** ao parar o mouse e com **Ctrl+Q**, com os diagnósticos sob o ponteiro e Markdown. **Diagnósticos inline** no fim da linha.
- **Parameter Info** (**Ctrl+P**, e automático em `(` e `,`).
- **Rename Symbol** (**Shift+F6**), **ações de contexto** (**Alt+Enter**), **Reformat Code** (**Ctrl+Alt+L**), **File Structure** (**Ctrl+F12**), `workspace/applyEdit` e `window/showMessage`.
- **Estado do servidor** na status bar, com Restart e log.
- **Gates**: `gate_lsp.rs` contra o servidor de teste embutido (`phpide_lsp::fixture`); `real_servers_on_laravel_fixture` com intelephense ou phpactor em `tests/fixtures/laravel-mini` quando instalados.
- **Atalhos alterados**: Ctrl+P = Parameter Info (Search Everywhere no Shift Shift); Ctrl+F12 = File Structure (Structure continua no Alt+7).

### Fase 6 — Executar, depurar, testar (M) — concluída em 2026-11-24

- **Run/Debug Configurations** (PHP Script, PHPUnit, Composer Script) com validação, salvas em `.phpide/run-configurations.json`; seletor no header com Current File, configurações temporárias (até cinco, Save Configuration) e Edit Configurations….
- **Executar o que está no cursor**: método de teste, classe de teste ou script (**Ctrl+Shift+F10** / **Ctrl+Shift+F9**); **Rerun** (**Ctrl+F5**); Run with Coverage.
- **Run tool window**: uma aba por sessão, saída com stderr em vermelho e links `arquivo:linha`, árvore PHPUnit ao vivo, detalhes da falha com stack trace clicável, **Rerun Failed Tests**.
- **Cobertura** (Clover) no gutter e como "NN% lines covered" na árvore do projeto.
- **Debug tool window**: Resume, Stop, Detach, passos, frames, árvore de variáveis com filhos carregados ao expandir, watches, **Evaluate** (**Alt+F8**), breakpoints com condição e contagem (**Ctrl+Shift+F8**).
- **Terminal real**: crate novo `phpide-term`, com PTY escrito sobre libc e emulador compatível com xterm (true color, tela alternativa, scrollback de 10 000 linhas, bracketed paste). Sessões em abas, seleção e cópia, Ctrl+clique em `arquivo:linha`; com foco, o terminal fica com as teclas (só atalhos de tool window e Settings passam).
- **Gate** `gate_run.rs`: configurações, árvore de testes, Rerun Failed, cobertura, execução no cursor, links da saída e sessão de debug pela UI. Sem PHPUnit nem Xdebug no ambiente do agente, o gate usa um script PHP que fala o formato TeamCity e grava Clover, e um par DBGp que responde como o Xdebug.
- **Correções no caminho**: filtro de classe do PHPUnit agora escapado como regex e ancorado em `::` (namespaces com `\U`, como `App\Tests\Unit`, quebravam); Shift Shift conta só toques isolados de Shift (dois Ctrl+Shift rápidos abriam a busca); textos de status restantes em português passaram para inglês.
- **Atalhos alterados**: Shift+F10 / Shift+F9 executam a configuração selecionada (Current File por padrão, então o comportamento antigo continua).
- **Fica para depois**: terminal no Windows (ConPTY) e repasse de mouse para programas no terminal; terminal em janela própria (item 1.x).

### Fase 7 — Git (M) — concluída em 2026-11-24

- **Commit tool window** (**Alt+0** / **Ctrl+K**, à esquerda como na New UI), no modelo de staging area: Merge Conflicts, Staged, Changes e Unversioned Files. Stage/unstage por arquivo e por grupo; Rollback com confirmação (não versionados vão para a Lixeira); mensagem com **Amend**; **Commit** (Ctrl+Enter) e **Commit and Push**. A **saída do commit, com a dos hooks**, aparece logo abaixo dos botões. Faixa de merge/rebase em andamento com Continue/Abort; Accept Yours/Theirs nos conflitos.
- **Diff lado a lado** numa aba da barra do editor: os dois lados só leitura com o highlight do editor, linhas alinhadas, a parte alterada da linha marcada, recolher trechos sem mudança, **F7/Shift+F7** entre mudanças, **Stage, Unstage e Revert por hunk** (patch daquele hunk via `git apply`), rolagem horizontal compartilhada, F4/duplo clique abre o arquivo na linha. Abre do Commit, do log e de Git › Compare with HEAD.
- **Popup de branches** no header (e Git › Branches…, Ctrl+Shift+`): Update Project (**Ctrl+T**), Commit, Push (**Ctrl+Shift+K**), Fetch, New Branch, branches locais e remotas com ↓entrada/↑saída e ações (checkout, nova branch a partir de, merge na atual, renomear, apagar), Abort de merge. O header mostra as contagens.
- **Version Control** (**Alt+9**): **Log** com grafo de commits, refs, filtros (texto/hash, autor, branch, caminho via Git › Show History), paginação, detalhes do commit (mensagem e arquivos contra o primeiro pai) e o diff de cada arquivo; **Console** com cada comando que a IDE rodou e a saída, com Stop.
- **Nada de Git na thread de UI**: retrato do repositório atualizado após cada mudança e a cada poucos segundos; operações rápidas em jobs; comandos com saída ao vivo que nunca esperam entrada (`GIT_TERMINAL_PROMPT=0`, SSH em batch mode salvo comando SSH do usuário). Arquivos abertos são salvos antes e recarregados depois.
- **Cores de status VCS** nos nomes na árvore e nas abas (RF-NAV-25, RF-TAB-12), pastas com mudanças destacadas (RF-NAV-26), ignorados apagados.
- **Gates com Git real**: `phpide-vcs/tests/workflow.rs` (hunks, rollback, hooks, remoto bare, conflito de merge, filtros do log) e `gate_git.rs` pela UI (commit de um hunk a partir do diff, saída do hook e commit recusado por hook, push, update de outro clone, branch pelo popup, log e diff de um commit, rollback, conflito resolvido e commitado).
- **Corrigido no caminho**: arquivos de um commit de merge agora aparecem (comparação com o primeiro pai).
- **Fica para depois**: stage linha a linha dentro de um hunk, editar o lado direito do diff, ferramenta de merge de três vias, rebase interativo, stash, cherry-pick pelo log e pedido de credenciais (HTTPS usa o credential helper do usuário; SSH, o agent).

### Fase 8 — Endurecimento e v1.0 (M) — concluída em 2026-11-24

- **Acessibilidade**: todo controle das telas principais, das tool windows, dos diálogos, de cada página de Settings e das telas de Git tem nome acessível (o gate `gate_a11y.rs` percorre a árvore AccessKit e falha num controle sem nome); rótulos ligados aos campos; Tab percorre o diálogo de Settings; um commit inteiro só com o teclado.
- **Log de erros** em `~/.local/state/phpide/logs/phpide.log`, com rotação em 2 MB (três arquivos); Help › Show Log e Show Log in Files.
- **Crash report local**: um panic grava mensagem, backtrace e as últimas 200 linhas do log em `~/.local/state/phpide/crashes/` (dez guardados); a próxima abertura mostra o aviso uma vez, com o caminho. Nada é enviado.
- **Recuperação de buffers**: arquivos não salvos vão para a pasta de recuperação a cada 2 s enquanto mudam; depois de uma saída anormal o projeto oferece Recover / Discard; saída normal esvazia a pasta. Projeto aberto em outra janela não é tocado.
- **Corrigidos**: corrida Detach/Stop do debugger (Stop/Detach logo após Resume não se perde; Stop atrás de outro comando termina como parado); separador de linha por arquivo (detectado ao abrir, convertido pela status bar, mantido ao salvar; arquivos novos seguem a configuração); testes de detach estáveis em paralelo (sem `--test-threads=1`); `kill_tree_reaches_grandchildren_on_unix` conta processo zumbi como encerrado e passa no ambiente do agente.
- **Requisitos do PhpStorm da seção 8.1**: todos feitos (detalhe na seção 8.1). RF-TAB-05 ficou parcial: abas no topo ou nenhuma; embaixo/esquerda/direita depois.
- **Pacotes**: `.deb` (`scripts/package-deb.sh`, conferido aqui com `dpkg-deb`), AppImage (`scripts/package-appimage.sh`) e Flatpak (`packaging/flatpak/`), os três pelo mesmo `scripts/install-files.sh`; desktop entry, metadados AppStream, ícone do app e da janela (app id `io.github.tavaresgerson.PhpIde`); `phpide --version` e `--help`; `LICENSE-MIT`, `LICENSE-APACHE` e `CHANGELOG.md`.
- **CI** (`.github/workflows/ci.yml`): fmt, clippy com e sem `experimental`, todos os testes em paralelo com PHP, Xdebug, Composer e intelephense instalados, testes das features congeladas, diffs de snapshot e capturas de revisão como artefatos, `.deb` e AppImage gerados e o `.deb` instalado; Flatpak e release no GitHub em tags `v*`. Substitui o `ci-experimental.patch`.
- **Gates**: `gate_hardening.rs`, `gate_a11y.rs`, `gate_project_view.rs`, `gate_parity.rs`; capturas nos três temas com `cargo test -p phpide-ui phase8_screenshots -- --ignored`.
- **Não verificado aqui**: AppImage e Flatpak não foram gerados no ambiente do agente (sem appimagetool nem flatpak-builder) — a CI gera. Os gates com servidores TypeScript reais (`gate_fe`, só com `experimental`) não rodaram aqui por falta do `node_modules`.
- **Limitação do Flatpak**: dentro do sandbox a IDE usa as ferramentas do sandbox (Git sim, PHP não); rodar as do sistema via `flatpak-spawn --host` fica para depois. Para PHP, `.deb` ou AppImage.
- **Versão**: continua 0.1.0 até o mantenedor marcar a tag da 1.0.

### Depois da v1.0 (lista do mantenedor, 2026-11-24)

Cada item vira uma fase própria, com o mesmo critério: gate por testes de interação pela UI e capturas nos três temas.

**1.x**

- [ ] **PHPantom como motor de análise de PHP.** O PHPantom (<https://github.com/AJenbo/phpantom_lsp>, MIT, Rust; parser Mago, stubs do PhpStorm embutidos, autoload do Composer, genéricos, anotações PHPStan, Laravel) passa a ser o motor que vem com o PHP IDE: inteligência de código funcionando ao abrir, sem servidor externo nem proprietário. intelephense e phpactor continuam selecionáveis.
  - Decidir a forma de distribuição: binário empacotado falando stdio, ou crate ligado à IDE e servido no próprio processo pelo transporte de streams (`LspClient::connect`, como o servidor de teste). Versão fixada e atualizada de propósito.
  - Settings › Languages › PHP: escolha do motor (PHPantom / intelephense / phpactor / comando próprio), opções (`.phpantom.toml`: versão do PHP, estratégia de indexação, diagnósticos), restart.
  - Mapear o que ele oferece para os recursos da Fase 5 e esconder o que faltar.
  - Projetos anexados (RF-PRJ-08) como workspace folders do motor.
  - **Pronto quando:** `real_servers_on_laravel_fixture` passar com o PHPantom na CI a cada commit, sem nenhum servidor instalado na máquina, com latência de inicialização e de tecla medidas num projeto grande.
- [ ] **Terminal em janela própria.** Parte do terminal com PTY da Fase 6: o terminal pode viver numa janela separada do sistema (e voltar para a tool window), várias sessões em abas, sessões divididas, perfis (shell, pasta, ambiente), integração com o shell (limites de comando, status de saída, caminhos e `arquivo:linha` clicáveis que abrem no editor), busca no histórico, fonte e tema do editor.
  - **Pronto quando:** um comando longo (`php artisan serve`, `npm run dev`) continuar rodando numa janela de terminal destacada enquanto se usa a janela do projeto, com links da saída abrindo arquivos.
- [ ] **Minimap na lateral direita do editor**, como no VS Code: miniatura do arquivo com a área visível destacada; clicar e arrastar para rolar; marcas de diagnósticos, mudanças Git, resultados de busca e seleção; ligar/desligar, largura e "caracteres ou blocos" em Settings › Editor. Custo imperceptível em arquivos de 50 mil linhas (desenhado dos spans de highlight, em cache por revisão).
  - **Pronto quando:** o benchmark do editor em 54k linhas continuar nos números da Fase 3 com o minimap ligado, e houver snapshots dele.

**2.x**

- [ ] **Gestor e modelador completo de banco de dados**, com as funcionalidades e a aparência das ferramentas de banco da JetBrains (<https://www.jetbrains.com/help/webstorm/2026.3/relational-databases.html>). O `phpide-db` já tem drivers de SQLite, PostgreSQL e MySQL atrás da feature `experimental`; este item dá a eles a UI nova e os completa.
  - Data sources: diálogo de conexão por driver (host, porta, usuário, senha no chaveiro do sistema, banco, SSL/TLS, túnel SSH), testar conexão; primeiro MySQL/MariaDB, PostgreSQL, SQLite, Oracle, Redis, Databricks, MongoDB, SQL Server e ClickHouse.
  - Database Explorer: schemas, tabelas, views, colunas, chaves, índices, rotinas, triggers; filtro, refresh, DDL de um objeto.
  - Console de consultas e arquivos SQL ligados a um data source: completion de SQL pelo schema, executar instrução/seleção/arquivo, várias abas de resultado, plano de execução (EXPLAIN) em árvore e diagrama, cancelar.
  - Editor de dados: grade com ordenação, filtro (campos WHERE / ORDER BY), paginação, edição com prévia das mudanças pendentes e Submit/Rollback, editor de valores longos, JSON e binário; modo de transação automático/manual.
  - Importar e exportar: CSV, JSON, INSERTs SQL, tabelas Markdown/HTML; dump e restore com `mysqldump`, `pg_dump`, `pg_restore` e `psql` quando instalados.
  - Modelagem: diagramas do banco (tabelas e relações) gerados de um schema e editáveis; criar e alterar tabelas, colunas, chaves e índices por diálogos que mostram o DDL antes de executar; comparação de schemas e script de migração entre dois data sources.
  - **Pronto quando:** um gate contra SQLite (sempre) e PostgreSQL/MySQL em containers (CI) criar um schema pelo modelador, editar dados na grade, rodar consultas com completion e exportar/importar uma tabela.
- [ ] **HTTP Client no estilo do Bruno** (<https://www.usebruno.com>): coleções guardadas como arquivos de texto no repositório (formatos `.bru` e OpenCollection YAML do Bruno, para as mesmas coleções abrirem nas duas ferramentas), pastas e requisições em abas, ambientes com variáveis e segredos em `.env`, REST e GraphQL primeiro (WebSocket e gRPC depois), autenticação (Basic, Bearer, API key, OAuth 2), scripts antes e depois da requisição e asserções, executar uma pasta inteira, visualizador de resposta (JSON/XML/HTML formatado, cabeçalhos, tempo, cookies), importar do Postman e de cURL, copiar como cURL / código PHP. Offline: sem conta e sem nuvem.
  - **Pronto quando:** um gate executar uma coleção do repositório contra um servidor de teste local, com ambiente, script e asserções, e os mesmos arquivos abrirem sem mudança no Bruno.

**Ainda sem versão**: suporte a frameworks (Laravel/Symfony), frontend TypeScript, Docker/Remote — continuam congelados atrás de `experimental`.

---

## 4. Ordem de execução e uso em projetos reais

```text
Fase 0 → Fase 1 → Fase 2 → Fase 3 → Fase 4 → Fase 5 → Fase 6 → Fase 7 → Fase 8 → 1.x → 2.x
```

- Após a Fase 3: uso experimental em projetos reais.
- Após a Fase 5: uso diário para PHP.
- Após a Fase 6: executar, testar e depurar dentro da IDE, com terminal real.
- Após a Fase 7: fluxo de Git completo dentro da IDE (commit com stage por hunk, diff, branches, push/pull, log).
- **Após a Fase 8 (agora): candidata a v1.0** — pacotes, CI, recuperação de buffers e crash report. Falta o mantenedor revisar e marcar a tag `v1.0.0`.

---

## 5. Migração dos documentos antigos

| Origem | Destino |
|---|---|
| `docs/SETUP.md` | `CONTRIBUTING.md` ✅ |
| `DESIGN_SYSTEM_UI_UX.md` §4 (tokens) | `docs/DESIGN_TOKENS.md` ✅ |
| `PENDENCIAS.md` — B10, B4, TLS/SSH | `docs/ROADMAP.md` › Known issues ✅ |
| Tabela de crates e descoberta do LSP | `docs/ARCHITECTURE.md` e `README.md` ✅ |
| Documentos antigos de planejamento | Apagados ✅ |

---

## 6. Decisões tomadas

1. Interface em inglês, sem internacionalização agora.
2. Nome unificado: **PHP IDE**.
3. Ícones: **Tabler Icons** (MIT).
4. Escopo congelado até a v1.0 (feature Cargo `experimental`).
5. Temas: Dark, Light e High Contrast.
6. Fonte de código sem ligaduras.
7. Atalhos no padrão IntelliJ: Shift+F10 Run, Shift+F9 Debug, Ctrl+Shift+F10 Run no cursor, Ctrl+F5 Rerun, Alt+F8 Evaluate, Ctrl+K Commit, Ctrl+Shift+K Push, Ctrl+T Update Project, Ctrl+Alt+Z Rollback, Ctrl+R Replace, Ctrl+D Duplicate, Ctrl+N Go to Class, Ctrl+P Parameter Info, Ctrl+Q Quick Documentation, Ctrl+F12 File Structure, Alt+Enter, Shift+F6, Ctrl+Alt+L.
8. Requisitos de referência do PhpStorm (documento do usuário, 2026-11-24) guiam a Fase 4.
9. Cada janela de projeto é um processo próprio; elas se encontram por um registro em `$XDG_RUNTIME_DIR`.
10. Terminal sem crates externos de PTY/emulação (`phpide-term`, escrito sobre libc).
11. Git pelo `git` CLI (sem libgit2), com o diff lado a lado próprio em vez do editor em modo só leitura: o alinhamento com linhas em branco exige um widget dedicado; ele reaproveita o highlight e as cores do editor.
12. Os testes de inteligência PHP usam um servidor LSP de teste embutido (`phpide_lsp::fixture`); o gate com intelephense/phpactor reais roda onde eles estiverem instalados.
13. **Motor de análise próprio depois da v1.0: PHPantom** (2026-11-24), por dar inteligência de código livre e embutida. Até lá, intelephense ou phpactor instalados pelo usuário.
14. **Lista pós-1.0** (2026-11-24): 1.x = PHPantom, terminal em janela própria, minimap; 2.x = banco de dados no nível das ferramentas JetBrains, HTTP Client no estilo do Bruno.

---

## 7. Pendências conhecidas

- Revisão das Fases 5 a 8 pelo usuário (na Fase 8: instalar o `.deb` ou o AppImage gerados pela CI, conferir o leitor de tela e a recuperação depois de matar o processo com `kill -9`): rodar `cargo test -p phpide-ui real_servers_on_laravel_fixture -- --nocapture` com intelephense ou phpactor instalado, testar Run/Debug com PHPUnit e Xdebug reais e o fluxo de Git num repositório de verdade (push/pull com o remoto e a autenticação do usuário).
- Projetos anexados ainda não têm language server próprio.
- Rename no intelephense só existe na edição premium; sem ela a IDE mostra a mensagem do servidor.
- Dentro do Flatpak não há PHP (ver Fase 8).

---

## 8. Registro do que ficou para depois

Tudo o que foi adiado nas Fases 0 a 7, num lugar só (2026-11-24). Os códigos RF-* são os de `docs/REQUIREMENTS.md`. Ao concluir um item, marcar aqui e no documento de origem.

### 8.1 Para a Fase 8 (antes da v1.0)

**Correções**
- [x] Debug: corrida entre Detach e Stop — um Stop no instante em que o `run` desbloqueia pode virar `ConnectionLost` (o processo é morto mesmo assim).
- [x] Status bar: o alternador de fim de linha muda uma configuração global aplicada ao salvar, não o arquivo atual. Deve valer para o buffer ativo.
- [x] Testes instáveis: `phpide-dbgp::detach_while_running_closes_socket_immediately` (falha às vezes sob carga) e o gate de detach do `phpide-run`, que exige `--test-threads=1`. A CI da Fase 8 precisa deles estáveis. *(5 rodadas seguidas em paralelo sem falha.)*

**Requisitos de referência do PhpStorm marcados para a Fase 8**
- [x] RF-PRJ-06 — grupos de projetos na Home. *(mover para grupo, criar, renomear, remover, recolher)*
- [x] RF-PRJ-07 — ícone próprio do projeto. *(`.phpide/icon.png` ou `.idea/icon.png`; senão iniciais)*
- [x] RF-PRJ-14 — pasta padrão de projetos. *(Settings › System Settings)*
- [x] RF-NAV-02 — visões por escopo (arquivos abertos, escopos do usuário). *(Project, Open Files, Changed Files, Production, Tests; escopos do usuário depois)*
- [x] RF-NAV-05 — prévia rápida com Espaço.
- [x] RF-NAV-11 — tamanho e data dos arquivos na árvore.
- [x] RF-NAV-12 — aninhamento de arquivos (file nesting). *(regras editáveis)*
- [x] RF-NAV-13 — guias de indentação na árvore.
- [x] RF-NAV-19 — marcar pasta como fonte, teste ou excluída. *(guardado em `.phpide/project.json`)*
- [x] RF-NAV-21 — substituir em arquivos e limitar a busca a uma pasta (Find Usages e Find in Files já existem). *(Replace in Files, Ctrl+Shift+R, com pasta e máscara de arquivos)*
- [x] RF-SEL-05 — filtrar resultados da busca por tipo de arquivo e incluir itens fora do projeto.
- [x] RF-SEL-08 — Recent Locations (Ctrl+Shift+E).
- [x] RF-TAB-04 — qual aba fica ativa ao fechar uma.
- [ ] RF-TAB-05 — posição das abas, inclusive nenhuma. *(parcial: topo ou nenhuma; embaixo/esquerda/direita depois)*
- [x] RF-TAB-11 — posição do botão de fechar; ordem alfabética.
- [x] RF-HL-12 — duplicar, renomear, exportar e importar esquemas de cores. *(também apagar; exportação em JSON)*
- [x] RF-UI-06 — keymap configurável; atribuir atalho pelo Search Everywhere. *(Alt+Enter no Find Action; só o keymap IntelliJ)*

### 8.2 Depois da v1.0, ainda sem versão

Além da lista 1.x/2.x da seção 3, que já tem versão.

**Editor e inteligência**
- [ ] Projetos anexados com language server próprio — resolvido junto com o PHPantom (1.x), que os recebe como workspace folders.
- [ ] Ligaduras na fonte de código: o motor de texto não faz shaping. Hoje a decisão 6 é não usá-las; fica registrado caso se queira oferecer como opção.

**Run, debug, terminal (Fase 6)**
- [ ] Terminal no Windows (ConPTY); hoje o PTY é só Unix.
- [ ] Repassar mouse para programas no terminal (o emulador já reconhece o pedido do programa; a interface não envia os cliques).
- [ ] Terminal em janela própria — já é o item 1.x.

**Git (Fase 7)**
- [ ] Stage linha a linha dentro de um hunk.
- [ ] Editar o lado direito do diff quando ele é o arquivo em disco.
- [ ] Ferramenta de merge de três vias para conflitos (hoje: Accept Yours/Theirs ou editar o arquivo).
- [ ] Rebase interativo.
- [ ] Stash (guardar e aplicar mudanças).
- [ ] Cherry-pick e checkout de revisão pelo log.
- [ ] Pedir credenciais dentro da IDE (hoje HTTPS usa o credential helper do usuário e SSH usa o agent; nada pede senha).

**Banco de dados (coberto pelo item 2.x)**
- [ ] Contagem explícita de linhas na grade (o total desconhecido é proposital por padrão).
- [ ] Conexões com TLS e túnel SSH.

**Requisitos de referência opcionais ou marcados para depois da v1.0**
- [ ] RF-PRJ-09 — projetos anexados resolvidos depois do principal.
- [ ] RF-PRJ-15 — mover a pasta do projeto.
- [ ] RF-NAV-03 — nó de scratches e consoles.
- [ ] RF-NAV-09 — membros de classe como nós da árvore (a janela Structure cobre o uso).
- [ ] RF-NAV-23 — comparar, histórico local, sincronizar, favoritos.
- [ ] RF-NAV-27 — cores de fundo por escopo.
- [ ] RF-SEL-06 — fixar resultados da busca num painel.
- [ ] RF-TAB-12 (parte) — cor da aba pela análise de código (a cor por status Git já existe).
- [ ] RF-TAB-13 — tamanho de fonte das abas (hoje segue o zoom da interface).
- [ ] RF-TAB-18 — destacar uma aba em janela própria.
- [ ] RF-HL-13 — highlight semântico (uma cor por variável).
- [ ] RF-HL-15 — abrir a configuração de cor do símbolo sob o cursor.

**Requisitos parciais, ainda sem fase definida** (o que falta em cada um)
- [ ] RF-TAB-05 — abas embaixo, à esquerda e à direita (topo e nenhuma já existem).
- [ ] RF-TAB-20 — dividir arrastando a aba (pelo menu da aba já funciona).
- [ ] RF-TAB-21 — mover ou abrir no outro split a partir da aba (pela árvore já funciona, Shift+Enter).
- [ ] RF-TAB-22 — mais de um split ao mesmo tempo, alternar, esticar e igualar (hoje: um split e desfazer).
- [ ] RF-HL-03 — lista de tipos de arquivo com adicionar, editar e remover padrões num só lugar.
- [ ] RF-HL-05 — interface para criar tipos de arquivo próprios (o motor já existe).
- [ ] RF-HL-06 — carregar gramáticas TextMate do usuário.
- [ ] RF-HL-07 — padrões de arquivos e pastas ignorados configuráveis (hoje lista fixa: `.git`, `.idea`, `.phpide`).
- [ ] RF-UI-07 — favoritos mnemônicos (Ctrl+F11); hoje só o anônimo (F11).

**Pacotes e outros (Fase 8)**
- [ ] Flatpak rodando PHP, Composer e Git do sistema (`flatpak-spawn --host`); envio ao Flathub (fontes do cargo offline).
- [ ] Recent Locations com highlight nos trechos; importar esquemas `.icls` do IntelliJ.

**Congelados atrás de `experimental`, sem versão:** suporte a frameworks (Laravel/Symfony), frontend TypeScript, Docker/Remote.

**Não planejado:** RF-PRJ-13 — juntar janelas de projeto em abas (só existe no macOS no PhpStorm).

### 8.3 Limitações externas (não são tarefas)

- O Rename do intelephense só existe na edição premium; sem ela a IDE mostra a mensagem do servidor. O PHPantom (1.x) resolve isso.
- Sem `appimagetool` nem `flatpak-builder` no ambiente do agente: o AppImage e o Flatpak só são gerados na CI.
