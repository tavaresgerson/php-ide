# Requisitos PHPForge IDE — Paridade com PhpStorm 2026.2

> Documento construído a partir da **documentação oficial da JetBrains**
> (jetbrains.com/help/phpstorm, versão 2026.2). É a especificação de referência
> para o desenvolvimento do PHPForge IDE. A interface do usuário deve ser
> **idêntica** à descrita aqui (nomes de elementos, atalhos, layout, fluxos).
> Ver também: `docs/roadmap-paridade-phpstorm.md` (mapeamento requisito → módulo).

---

# PARTE 1 — Interface do usuário (identidade visual obrigatória)

## 1.1 Janela principal

```
┌────────────────────────────────────────────────────────────────┐
│ WINDOW HEADER (New UI)                                         │
│  [Main menu] [Projeto ▾] [Git: branch ▾] [Run ▾]               │
├──────────┬───────────────────────────────────────────┬─────────┤
│ TOOL     │  EDITOR (abas, gutter, breadcrumbs)       │ TOOL    │
│ WINDOW   │                                           │ WINDOW  │
│ BAR      ├───────────────────────────────────────────┤ BAR     │
│ (esq.)   │  (tool windows horizontais: Run, Debug,   │ (dir.)  │
│          │   Terminal, Version Control, Problems...) │         │
├──────────┴───────────────────────────────────────────┴─────────┤
│ STATUS BAR: [mensagens/notificações] ... [caret] [LF] [UTF-8]  │
│   [indent] [branch] [PHP debug listen ☎] [memória]             │
│   [Navigation Bar — New UI: fica NA STATUS BAR (bottom)]       │
└────────────────────────────────────────────────────────────────┘
```

- **Window header** unificado com widgets: **Main menu** (modos: *Merge with Main Toolbar* / *Hamburger* `Alt+\` / *Show above*), **Project widget**, **VCS widget** (branch + incoming/outgoing), **Run widget**. Customizável.
- **Navigation bar**: na New UI fica na **status bar (parte inferior)**; alternável para topo ou oculta (`View | Appearance | Navigation Bar`). Foco: `Alt+Home`.
- **Tool window bars**: ícones grandes nas laterais/base; vertical (topo) vs horizontal (base); botão **More tool windows** (⋯); splits por arrasto; *Compact Mode* reduz alturas.

## 1.2 Tool windows (nomes exatos + atalhos padrão)

| Tool window | Atalho | | Tool window | Atalho |
|---|---|---|---|---|
| Project | `Alt+1` | | Version Control (Git) | `Alt+9` |
| Bookmarks | `Alt+2` | | Commit | `Alt+0` |
| Find | `Alt+3` | | Terminal | `Alt+F12` |
| Run | `Alt+4` | | Database | `Ctrl+Alt+3` |
| Debug | `Alt+5` | | Notifications | — (clique na status bar) |
| Problems | `Alt+6` | | Services | `Alt+8` |
| Structure | `Alt+7` | | Remote Host | — |

- Gestão: Hide Active `Shift+Escape`; Hide All `Ctrl+Shift+F12` (restaura com mesmo atalho); editor→tool window `F12`; tool window→editor `Escape`; stretch `Ctrl+Alt+Shift+setas`; speed search em árvores.
- Header da tool window: botão Options (⋮) + Hide; ícones no hover (Advanced: *Always show tool window header icons*); vistas por abas ou dropdown (*Group Tabs*).

## 1.3 Status bar (widgets exatos)

Esquerda: mensagens de evento (clique → Notifications) + ícone Processes.
Direita (configurável por clique direito):

| Widget | Comportamento |
|---|---|
| **PHP Language Level** | Seletor PHP 5.3–8.x; sincronização com composer.json |
| **Line:Column** | Posição do caret; clique → ir para linha |
| **Line endings** (`LF`) | Clique para alterar |
| **Encoding** (`UTF-8`) | Clique para alterar |
| **Column Selection Mode** | Toggle `Alt+Shift+Insert` |
| **Read-only/Read-write** (cadeado) | Trava arquivo |
| **VCS branch** | Branch atual → gerenciamento |
| **Indent style** (`4 spaces`) | Estilo de indentação do arquivo |
| **Listen for PHP Debug Connections** (☎) | Liga/desliga escuta Xdebug |
| **Memory indicator** | Heap usado |
| **Default deployment server** | Seleção de servidor padrão |

## 1.4 Editor

- **Gutter**: números de linha, breakpoints (New UI: **sobre** os números), bookmarks, folding (ícones no hover; *Always* configurável), annotations Git blame, ícones de ação (run/debug/quick-fix), color preview CSS, linhas modificadas VCS, coverage, faixas de inspeção na scrollbar + Inspections widget (clique → Problems).
- **Breadcrumbs**: trilha do elemento sob o caret; top ou bottom; configurável por linguagem.
- **Lens mode** (não existe minimap): preview do código ao pairar sobre a scrollbar.
- **Tabs**: limite padrão 10 (Closing Policy); preview tab (itálico); pin/unpin; navegação `Alt+←/→`; Switcher `Ctrl+Tab`; fechar `Ctrl+F4`; split right/down; detach `Shift+F4`; ocultas (▾); tab placement configurável.
- **Sticky lines** (máx. 5): declarações estruturais fixadas no topo ao rolar.
- **Inlay hints**: parameter name hints (PHP: opções *all arguments*, *pass by reference*), Code vision (Inheritors/Usages/Code author), exclude list, toggle global.
- **Fonte/zoom**: `Alt+Shift+.` / `Alt+Shift+,`; zoom com Ctrl+wheel (opcional); line height; ligatures.
- **Ação da lâmpada** (intention/quick-fix) no gutter; `Alt+Enter`; preview automático.
- **Editor | General | Appearance**: caret (blink, block, smooth movement *Snappy/Gliding*), highlight occurrences, method separators, whitespace, indent guides, **PHP class and namespace separators**, HTML tag tree highlighting.

## 1.5 Welcome screen

Abas laterais: **Projects** (New Project / Open / Get from VCS + recentes) · **Customize** (tema, keymap, fontes, import settings) · **Plugins** (Marketplace/Installed) · **Learn** · **Remote Development**.
Primeiro run: **Import Settings dialog** (detecta IDEs instalados).

## 1.6 Settings (Ctrl+Alt+S) — árvore de categorias

- **Appearance & Behavior** — Appearance (temas, Compact Mode, fonte UI **Inter**), Menus and Toolbars, System Settings, File Colors, Scopes, Notifications, Quick Lists, Path Variables
- **Keymap** (Windows/macOS/Emacs/Eclipse/NetBeans/Sublime/Visual Studio…)
- **Plugins**
- **Editor** — General (Appearance, Breadcrumbs, Code Completion, Editor Tabs, Sticky Lines, Smart Keys, Postfix Completion, Auto Import), Code Style (PHP/JS/CSS/HTML/SQL/HTTP Request…), Inspections, Color Scheme, Font, Inlay Hints, File Types, Code Editing, Live Templates
- **PHP** — Language Level, CLI Interpreter, Composer, CLI Interpreters, Debug, Test Frameworks, Quality Tools, Frameworks (Symfony/Laravel/WordPress/Drupal/Joomla), Blade, Servers, Path Mappings
- **Languages & Frameworks** — JavaScript, TypeScript, Node.js, CSS, SQL, Twig
- **Version Control** — Git, GitHub, GitLab, Mercurial, Subversion; Changelists, Commit, Confirmation
- **Build, Execution, Deployment** — Deployment (servidores), Debugger, Coverage, Docker, Vagrant, WSL
- **Tools** — Terminal, Web Browsers and Preview, Actions on Save, File Watchers, SSH Configurations, Tasks, HTTP Client, Database
- **Project Structure | Project** · **Advanced Settings**

## 1.7 Temas e aparência

- Temas UI: **Islands Dark** (padrão), Islands Light, Islands Darcula, Dark, Light, High Contrast, Darcula; *Different tool window background*; **Sync with OS**.
- Color schemes do editor: Dark, Light, High Contrast, Classic Light, Darcula, Darcula Contrast.
- Acessibilidade: Zoom IDE, custom font, screen readers, contraste, ajuste para daltonismo.
- **New UI** é o padrão (Classic UI disponível).

## 1.8 Atalhos essenciais (keymap padrão)

`Shift×2` Search Everywhere · `Ctrl+N`/`Ctrl+Shift+N`/`Ctrl+Alt+Shift+N` Class/File/Symbol · `Ctrl+Shift+A` Find Action · `Alt+Enter` context actions · `F2/Shift+F2` próximo/anterior problema · `Ctrl+E` Recent Files · `Ctrl+W` extend selection · `Ctrl+Space` completion · `Ctrl+Shift+Space` type-matching · `Ctrl+Shift+Enter` statement completion · `Ctrl+/` comentar · `Alt+F7` Find Usages · `Ctrl+B` goto decl · `Ctrl+Alt+B` goto impl · `Ctrl+F12` structure popup · `Alt+Insert` Generate · `Ctrl+Alt+Shift+T` Refactor This · `Alt+`` VCS popup · `Ctrl+Q` quick docs · `Ctrl+H` type hierarchy · ``Ctrl+` `` Quick Switch Scheme · `Ctrl+Alt+S` Settings.

---

# PARTE 2 — Núcleo PHP

## 2.1 Code completion

- **Basic** (`Ctrl+Space`): símbolos no escopo de visibilidade + live templates; 2ª invocação inclui inacessíveis; aceite por `Enter`/`Tab`/`Ctrl+Shift+Enter` (statement-correct); aceite por caracteres contextuais configuráveis; `Ctrl+Space` em diálogos de path.
- **Type-Matching** (`Ctrl+Shift+Space`): só tipos aplicáveis ao contexto.
- **Statement** (`Ctrl+Shift+Enter`): completa construtos (parênteses/chaves/`;`), encapsula expressão como argumento.
- Configurações (Editor | General | Code Completion | Popup): match case, auto-insert single suggestion, sort, show-as-you-type, doc popup delay, auto-parênteses, parameter info (`Ctrl+P`), suggest variable/parameter names.
- **Full Line Completion**: modelo local 100% on-device (PHP bundled; JS/TS/CSS com download); aceite `Tab`/`Ctrl+Right`/`End`; requisito AVX2; settings em Code Completion | Inline; sem envio de código.
- **Hippie completion** (`Alt+/`), **Postfix completion** (`.if` etc.).

## 2.2 Inspeções (PHP — 23 categorias, ~387 inspeções)

Categorias (nomes oficiais): **Undefined symbols (14), Probable bugs (46), Code smell (66), Code style (67), Type compatibility (17), Control flow (14), General (63), Naming conventions (6), Attributes (16), PHPDoc (11), PHPUnit (24), Pest (7), Composer (5), Quality tools (6), Error handling (3), Regular expressions (5), Replaceable assignments (2), PHP strict standards (2), Symfony (2), Behat (1), Psalm (1), Taint analysis (2), Unused symbols (7)**.

Mecânica: severidades configuráveis + customizáveis; profiles (Project Default / per-project); escopos; on-the-fly e batch (*Code | Inspect Code* → Problems); quick-fixes (lâmpada vermelha) e intention actions (lâmpada amarela); *Disable new inspections by default*; prefixos por ferramenta externa (`phpstan:`, `psalm`, `phpcs`…).

## 2.3 Intention actions

`Alt+Enter`; preview automático; gerenciamento em Editor | Intentions; atalhos atribuíveis por intenção. Exemplos: inicializar propriedades no construtor, criar parâmetro para variável indefinida, *Add `#[ArrayShape]`/ObjectShape attribute ou PHPDoc*, *Run composer script*, *Enable inspection*, *Move HTTP Requests*.

## 2.4 Live templates

Abreviação + `Tab`; tipos: simples, parametrizados (`$VAR$`, funções como `phpClassName()`, `phpSuggestVariableName()`, `date()`, `enum()`, `groovyScript()`…), surround (`$SELECTION$`, `Ctrl+Alt+J`); variáveis imutáveis `$END$`/`$SELECTION$`; grupos por linguagem; reformat according to style; compartilhamento por XML; exemplos PHP documentados: `rqr`→require, `pubsf`→public static function, `fore`→foreach, `eco`→echo.

## 2.5 Refatorações PHP (lista completa)

Rename (`Shift+F6`, in-place/modal, comments & strings) · Move (`F6`: arquivo, **Move Namespace** PSR-0/4, Move Class, Move Static Member, Move Function/Constant, **Move Instance Method**) · Copy (`F5`) · Safe Delete (`Alt+Delete`) · Extract Method (`Ctrl+Alt+M`) · Extract/Introduce Variable (`Ctrl+Alt+V`) · Extract Constant (`Ctrl+Alt+C`) · Extract Field (`Ctrl+Alt+F`) · Extract Parameter (`Ctrl+Alt+P`) · Extract Class (com accessors) · Extract Interface (replace references) · Extract Superclass · Extract Include File (PHP-specific) · Inline (`Ctrl+Alt+N`) · **Change Signature (`Ctrl+F6` — PHP e JS; propagação de parâmetros pela hierarquia)** · Make Static · Pull Members Up · Push Members Down.
Geral: `Ctrl+Alt+Shift+T` Refactor This; preview no Find; *Refactor Anyway*; desfazer `Ctrl+Z`; multi-caret para pré-seleção de membros.

## 2.6 PHPDoc e type checking

- **Array shapes / Object shapes** em PHPDoc **e** atributos `#[ArrayShape]`/`#[ObjectShape]`; completion de chaves; quick-fixes de geração.
- **Integer ranges** `int<min, max>`; inspeção *Number ranges mismatch*.
- **Anotações Psalm**: pseudotipos (`numeric`, `scalar`), `@psalm-param/var` (uniões de constantes, wildcards `*`), `@psalm-type` / `@psalm-import-type`.
- **Generics** `@template` (caso simples função→parâmetro).

## 2.7 Composer

- UI dedicada: **init, install, update, Validate, Dump Autoloader, Check for Changes in Dependencies, Clear Cache, List Licenses, Diagnose, Simulate Update, Self-Update**; saída no **Composer Log**.
- Execução: composer global, composer.phar (com download), via interpretador local/remoto/Docker (`composer` image).
- Criação de projeto: *Composer Package Project* (create-project com completion de pacote/versão); Init Composer; arquivo `composer.json` novo por template.
- **Sincronização IDE ↔ composer.json**: vendor como bibliotecas (protegidas, indexadas), PSR-0/4 roots → source/tests roots, PHP language level de `require.php`, check de updates (`update --dry-run`), notificação de vendor ausente, `--ignore-platform-reqs`.
- **Múltiplos composer.json** por subprojeto (cada um com PSR roots, test frameworks, vendor próprios).
- Assistance no composer.json: completion (licenses, stability, namespaces, versões/extensões PHP, pacotes com constraints), quick docs, navegação para pacote/classe/script.
- Gestão de dependências: install/update/uninstall por gutter e quick-fix; repositórios custom.
- **Scripts**: completion (event names, classes, `@refs`), Rename refactoring, execução por gutter/Alt+Enter, run configuration *Composer Script*.

## 2.8 Ferramentas de qualidade (todas como inspeções, on-the-fly + batch)

| Ferramenta | Integração |
|---|---|
| **PHPStan/Larastan** | plugin bundled ativo por default; auto-config via composer scripts; `phpstan.neon(.dist)`; executável `vendor/bin`; prefixos `phpstan`/`phpstan::` |
| **Psalm** | bundled; auto-config via scripts; `psalm.xml(.dist)`; *Init cache* para análise de projeto inteiro |
| **PHP CS Fixer** | ≥2.8; regras preset ou config; quick-fix arquivo inteiro + **External Formatter** (Ctrl+Alt+L, on save, before commit) |
| **Laravel Pint** | baseado em php-cs-fixer; `pint.json` opcional; presets; reformat uncommitted only |
| **PHP_CodeSniffer** | ≥1.5; standards PSR/custom; auto-detecção de standards (Drupal, Joomla, Magento, Symfony, WordPress, Yii, PHPCompatibility…); integra **phpcbf** |
| **PHP Mess Detector** | rulesets built-in + XML; sem correção automática |

Compartilhamento de padrões de time: diretório no projeto + project profile em VCS.

---

# PARTE 3 — Execução, depuração e testes

## 3.1 Run configurations (tipos e campos)

Comuns a todas: Name, Allow multiple instances, Store as project file, Before launch tasks (Launch Web Browser, Run External tool, Run Another Configuration, Run File Watchers, Run npm script, Upload files to Remote Host…), Logs.
Configurações temporárias (Run \<file\> no contexto) salvas via Save Configuration.

| Tipo | Campos |
|---|---|
| **PHP Script** | File, Arguments; Interpreter, Interpreter options, Custom working directory, Environment variables (.env); Docker Compose area; Break at first line |
| **PHP Web Page** | Server (página Servers), Start URL (URL composta dinâmica), Browser |
| **PHP Remote Debug** | Server, Ide key (session id) |
| **PHPUnit** | Test scope (Directory/Class/Method/Defined in config file/Composite), alternative config/bootstrap file, test runner options, **Preferred Coverage engine** (Xdebug/PCOV/phpdbg), ParaTest |
| **Pest** | Idem + Run in Parallel, Run with Mutation |
| **Behat / PHPSpec / Codeception** | Biblioteca (path + Reload), config file (default `behat.yml`/`phpspec.yml`/`codeception.yml`) |
| **PHP HTTP Request** | Script via servidor web |
| **HTTP Request** | Environment, File (.http), Request name |
| **Composer Script** | composer.json path, Script, CLI params |

Ações: Rerun `Shift+F10`, Stop `Ctrl+F2`, Pin Tab, Pause Output, Show Running List.

## 3.2 Depuração (Xdebug / Zend Debugger)

- Config: Settings | PHP | Debug — porta (9003), resolved breakpoints (Xdebug 2.8+), return value debugging (3.2+, Step Into no `return`), data flow analysis (predict conditions, gray-out unreachable), skipped paths.
- **Listen for PHP Debug Connections** (status bar ☎ + Run menu); Incoming Connection dialog → path mappings; validação *Web Server Debug Validation*; extensões de browser (Xdebug Helper).
- **Path mapping**: Settings | PHP | Servers (host, port, debugger, absolute paths, *Shared* → `.idea/php.xml`); importação de deployment configs.
- **Breakpoints**: line (`Ctrl+F8`), method, exception, conditional, logging (logpoint, `Shift+clicar`); propriedades: Enabled, Suspend, Condition, Logging (hit message/stack/evaluate), Remove once hit, Disable until hitting another breakpoint (trigger), Description; mute; grupos; status (Verified/Warning/Invalid/Inactive/Muted).
- **Debug tool window** (`Alt+5`): Frames; Variables (Set Value `F2`, Inspect, Jump to Source/Type, Compare with Clipboard, user Constants); Watches (add/edit/duplicate `Ctrl+D`/pause/evaluate, persistent, separate mode); **Evaluate Expression** (`Alt+F8`, code fragments `Ctrl+Enter`) e Quick Evaluate (`Ctrl+Alt+F8`); Console Input (REPL PHP com completion); value tooltips; **inline values** e inline watches; JSON/XML estruturado.
- Stepping: Into `F7`, Over `F8`, Out `Shift+F8`, Resume `F9`, Show Execution Point `Alt+F10`.
- Cenários: zero-config, CLI, comandos de framework (Symfony console, Artisan, WP-CLI, Drush), sessões simultâneas, multiuser via proxy DBGp, túnel SSH.

## 3.3 Servidor web embutido

- Sempre ativo; porta padrão **63342**; estático direto, PHP via `php -S` automático; `Alt+F2` abrir no browser; **Built-in Preview** (aba no editor com reload); opções: *Can accept external connections*, *Allow unsigned requests*.

## 3.4 Frameworks de teste

- **PHPUnit**: via Composer/phar; Test Frameworks page (autoloader path ou phar + Reload, config/bootstrap); geração **File | New | PHP Test | PHPUnit Test** (template, namespace, métodos); navegação test↔sujeito (`Ctrl+Shift+T`, Go to Test Subject); execução por gutter/pasta/classe/método/data set; UI Test Runner (árvore + output); **Rerun Automatically** (auto-test delay); **ParaTest** paralelo.
- **Pest**: init via `pest --init`; gutter Run/Debug/**Parallel**/**Mutation** (Pest 3+, Xdebug 3+); `debug()` continue button (Pest 4+).
- **Behat 2/3**: Gherkin keywords, anotações `@given/@when/@then`, regex PCRE/Turnip; execução em `.feature`.
- **PHPSpec**: `phpspec.yml`; prefixo `spec`; geração de specs.
- **Codeception ≥2.2**: unit/functional/acceptance; bootstrap `codecept bootstrap`.

## 3.5 HTTP Client

- Arquivos `.http`/`.rest`: highlighting, completion (hosts/headers/OpenAPI), folding, Structure view, Move refactoring, live templates; scratch files.
- Sintaxe: método + URI + versão, headers, corpo; separador `###`; nomes `# @name`; comentários `#`/`//`; `< ./input.json`; multipart (`mptr`); auth Basic/Digest; **HTTP/2**; métodos customizados.
- Conversão: colar cURL → .http automático; export para cURL; import Postman; OpenAPI → request; Guzzle → request.
- Tags: `@no-redirect @no-log @no-cookie-jar @no-auto-encoding @timeout @connection-timeout`.
- Resposta: Services tool window; formatos por content-type; preview JCEF (HTML/PDF/imagem); histórico (50) em `.idea/httpRequests/`; redirect `>>`/`>>!`; cookies jar (300).
- **Variáveis** `{{var}}`: environment (`http-client.env.json` / `http-client.private.env.json` sobrescreve), global, in-place `@var =`, per-request, dinâmicas (`$uuid`, `$timestamp`, `$randomInt`, faker), `$env.ENV_VAR`; coleções iteram (`[*]`); Crypto API.
- **Scripts**: pre-request (`request.variables.set`) e response handler (`> {% client.global.set, client.test, client.assert %}`; `jsonPath`, `xpath`); ECMAScript 2023 + imports.
- **WebSocket** (`WEBSOCKET`, `===`) e **GraphQL** (`GRAPHQL` + JSON variables).
- **PHP Debug de request**: Alt+Enter → PHP Debug (injeta `XDEBUG_SESSION`).

---

# PARTE 4 — Frameworks PHP

## Symfony (plugin open-source)
Completion/navegação de services, routes, forms, events, Doctrine, translations; **Navigate | Symfony Symbol**; gutter icons (template↔controller, rota↔controller); **Symfony Profiler na status bar**; live templates; log highlighting; code style Symfony2; bin/console via Run Anything com completion e debug; Twig language injection + debug de templates.

## Laravel (Laravel Idea bundled + nativo)
Geração de código, completion Eloquent/routes/request fields/validation/gates; **Blade**: completion de diretivas (customizáveis em PHP | Blade), variáveis em `@for/@foreach`, navegação `@extends/@include` + Find Usages, language injection JS/CSS em seções, **debug de Blade** (cache `storage/framework/views`); Artisan via Run Anything; Pint; Pest; log highlighting.

## WordPress
Indexação de hooks: completion em `add_action/add_filter`; gutter icons registro↔invocação (`do_action/apply_filters`); navegação da callback; *Search on WordPress.org*; code style + phpcs WordPress; WP-CLI; template de novo projeto.

## Drupal (6–9)
Include paths automáticos; associação `*.test/install/engine/profile/theme` → PHP; completion/navegação de hooks (`.api.php` docs); code style Drupal + Coder; *Search in Drupal API*; Drush (autodetect via Composer, Run Command, debug); Drupal 8 + Symfony Bridge (yaml services, `t()`); template *Drupal Module*.

## Joomla!
Ativação manual/auto-detect; DocBlocks; code style Joomla; **Import Data Sources** lê `configuration.php` (prefixos `#__`, dialeto SQL); phpcs Joomla.

---

# PARTE 5 — Frontend

## JavaScript/TypeScript
- JS: completion, inspections, refactorings, ES5.1/ES6+/JSX/Flow por escopo; auto-import ES6+CJS; `node_modules` como biblioteca; proposals (`import defer/source`); inlay hints (params, return types, chains).
- TS: Language Service on-the-fly; File/Project Errors na Problems; seleção de versão (bundled/custom); *service-powered type engine*; auto-import com `type` modifier; inlay hints do LS (4.4+); Go to Type Declaration; @types sugeridos; TypeScript-Go preview.
- **Debugger**: client+server side (Chromium); interactive console; testes Jest/Karma/Mocha/Cucumber.js/Protractor; attach Node.js.

## React
create-vite; completion (componentes, props, eventos, propTypes, classes CSS do projeto); quick-fixes *add useState/props*; **colar HTML→JSX** (class→className); 50+ live templates; `use client/server/memo/no memo`; Emmet JSX; Structure view React; ESLint react.

## Angular
Angular CLI; language service; schematics; `ng add` / Angular Dependency; signals (create/computed/effect, contentChild/viewChild); structural directives (rename, parameter info); host listeners/bindings; reactive forms (FormGroup aninhado); `@defer hydrate`; Extract Component; Related Symbol popup (`Ctrl+Alt+Home`); inspections (standalone imports, templateUrl duplicado); Angular Material; `.ws-context`.

## Vue.js
create-vue; SFC por blocos; **Volar** language server; completion (componentes, slots de libs, Vuex); Extract Vue Component; live templates Sarah Drasner; Prettier default; debug Chromium; Nuxt workaround.

## Tailwind CSS
Tailwind Language Server (v3/v4); completion em HTML/`@apply`/strings; preview CSS no hover/doc; `includeLanguages`; `emmetCompletions`.

## CSS/HTML
Sass/SCSS/Less/Stylus/PostCSS; completion (propriedades, variáveis, mixins, classes/ids em HTML/templates/JSX); MDN docs + **Baseline status**; browser compatibility inspection; color picker gutter + conversões HEX/HSL/HWB/lch/oklch; refactorings (Introduce Variable `var()`/`$`, Introduce/Move Ruleset); Show Applied Styles; Emmet.

---

# PARTE 6 — Database Tools and SQL (RF-DB)

> Implementar evoluindo os módulos `ide/db.*` (código-livre) até esta paridade.

- **SGBDs completos**: MySQL/MariaDB, PostgreSQL, SQL Server, Oracle, SQLite, H2, HSQLDB, Derby, IBM Db2, Sybase ASE, ClickHouse, CockroachDB, Exasol, Greenplum, Vertica, Snowflake, BigQuery, Redshift, Azure SQL, Hive, Cassandra, MongoDB, Redis, Couchbase, DocumentDB, DynamoDB + drivers genéricos para ~20 outros.
- **Conexões**: Data Sources and Drivers dialog; `.idea/dataSources.xml`; project/global; duplicar `Ctrl+D`; SSH/SSL; URL-only; test connection; cores; pastas; introspection com seleção de schemas (regex).
- **Query console**: consoles por data source (Scratches), session, default schema, resolve modes Playground/Script; execução `Ctrl+Enter` (statement/seleção/todos; limite 20MB); procedures com parâmetros; Execute to File; In-Editor Results; cancel com aviso; notificação >20s; **Query History** `Ctrl+Alt+E`; Quick Doc DDL; user parameters regex; code style SQL por dialeto; completion resolvido por metadados.
- **Data editor (grade)**: Table/Tree/Text/**Transpose**; Record View; Value Editor `Shift+Enter`; Aggregate View; Geo Viewer; Chart; Update Interval; paginação; Add Row `Alt+Insert`, Delete `Ctrl+Y`, Clone; célula com completion; Set DEFAULT `Ctrl+Alt+D`, NULL `Ctrl+Alt+N`, UUID, LOB file; **Preview DML**; Submit `Ctrl+Enter`/Revert; transações Auto/Manual (commit/rollback, isolamento); Related Rows `F4`; Go to DDL/Row `Ctrl+G`; Full-Text Search; filtros server/local; ordenação; View Query.
- **Diagramas**: schema/table diagrams; pan/zoom/lupa; cores; export (graphml, drawio, .dot, **Mermaid**, **PlantUML**, PNG, clipboard); execution plans visuais.
- **Import/Export**: extractors (TXT, CSV, JSON, XML, Markdown, Excel, SQL INSERT); dump via `mysqldump`/`pg_dump` integrado (local ou Docker, WSL paths); Run SQL Script; CSV import com mapping dialog (charset, header, tipos, DDL editável, preview); Copy Tables; compare data/estruturas.

---

# PARTE 7 — Ferramentas integradas

## Docker
Daemons alternativos (Colima, Rancher); conexões (TCP/Unix/SSH, contexts); Services tool window (images pull/push, containers, Compose scale); run configs (Dockerfile/Image/Compose); **interpretadores PHP e Node.js em containers**; registries; path mappings.

## WSL
Interpretador remoto via WSL (`\\wsl.localhost`); projetos no filesystem WSL; Git do WSL automático.

## Dev Containers
devcontainer.json do zero/da Welcome screen; Dev Container CLI; pré-requisito Docker CLI + Buildx.

## Terminal (`Alt+F12`)
Shell configurável + detecção de sessões; abas (`Ctrl+Shift+T`) + splits; persistência de abas/history; New SSH Session; **command completion** (bash/zsh/pwsh); separadores visuais por comando; busca `Ctrl+F`; links de arquivos clicáveis; drag-and-drop de arquivos.

## Deployment
Servidores FTP/FTPS/SFTP/WebDAV/local; Remote Host tool window; toolbar de arquivo remoto no editor; Upload/Download/Sync/Compare; auto-upload on save/commit; excluded paths; mappings; server groups.

## Local History
Sempre ativo; revisões rotuladas por eventos; Show History com diff; revert total/parcial; restaura arquivos deletados; Put Label; Create Patch; retenção 5 dias úteis; storage em `LocalHistory/`.

## VCS — Git
Clone (shallow), submodules, multi-root, remotes; VCS Operations Popup; **Branches popup** (favoritos, grupos por `/`, incoming/outgoing, compare with current, restore workspace por branch); **Git tool window** (`Alt+9`): Log (branches/commits/files panes), filtros two-dot; Commit tool window (`Alt+0`): changelists, **partial commits por chunk/linha**, commit inline pelo gutter, Amend, Advanced Options (`Ctrl+O`: reformat, optimize imports, cleanup, check TODO, run tests, **check malicious dependencies**), After Commit (upload); Push (`Ctrl+Shift+K`, `--force-with-lease`, multi-repo); staging area opcional; merge 3-way; rebase/reword/edit history; worktrees; stashes; hooks.
**GitHub**: clone/share/fork; Pull Requests tool window (criar com draft/reviewers; timeline; checkout; **Review mode** com sugestões; merge/squash/rebase).
**GitLab**: idem para Merge Requests; GitLab CI highlighting.

---

# PARTE 8 — IA e recursos avançados

- **Full Line Completion** local (ver 2.1).
- **AI Assistant** (plugin): chat context-aware, coding agents (Junie/Claude/Codex/Copilot), ACP, MCP (IDE como servidor MCP), BYOK, modelos locais, cloud completion, next edit suggestions.
- **Agent Sessions** (JetBrains Air): tool window, global prompt, presets, worktree temporário, indicador de janela de contexto, diff review com notas.
- **PhpStorm MCP server**: expor modelo do codebase + inspections para agentes.
- **Qodana**: mesmas inspeções em CI/CD (não-alvo inicial; arquitetar inspeções para rodarem headless).

---

# Apêndice — Matriz de verificação de paridade da UI

| Elemento | Estado exigido |
|---|---|
| Window header com 4 widgets | Idêntico |
| Tool window bars + atalhos Alt+1..0, Alt+F12 | Idêntico |
| Status bar widgets (12 listados) | Idênticos |
| Gutter (breakpoints sobre linha, folding hover, blame) | Idêntico |
| Editor tabs (limite 10, preview, pin, split, detach) | Idêntico |
| Sticky lines (5) + inlay hints + code vision | Idêntico |
| Welcome screen (5 abas) + Import Settings | Idêntico |
| Settings: árvore completa da seção 1.6 | Idêntico |
| Search Everywhere (Shift×2) + Find Action | Idêntico |
| Temas Islands Dark/Light | Idêntico |
| Quick Switch Scheme (``Ctrl+` ``) | Idêntico |
| Keymap padrão (tabela 1.8) | Idêntico |
