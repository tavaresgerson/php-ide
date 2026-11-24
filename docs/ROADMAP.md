# Roadmap

PHP IDE is reworking its UI layer and editor. The back-end crates (LSP
client, DBGp, PHPUnit, Git, process execution, core) are kept; the editor and
the shell are rebuilt. Each phase closes only with a screenshot, an
interaction test and a measurement — a ticked box without that evidence does
not count.

## Decisions

- **Toolkit:** stay on Rust + egui/eframe; upgrade egui to the current stable
  release at the start of Phase 1.
- **Editor:** a custom widget (`phpide-editor` crate) replaces `egui::TextEdit`:
  rope as source of truth, only visible lines rendered, grouped undo,
  multi-cursor, incremental tree-sitter highlighting.
- **Scope until v1.0:** PHP editing, intelligence, run/debug/test, Git.
  Database Tools, frontend language servers, Services, Remote Host,
  Deployment, frameworks, HTTP client, Docker and AI are frozen behind the
  `experimental` Cargo feature. The code is kept, not deleted.
- **Design system first:** tokens, bundled fonts (Inter and JetBrains Mono,
  both OFL 1.1), [Tabler Icons](https://tabler.io/icons) (MIT) and shared
  components before any new screen.
- **UI language:** English. No internationalization work before v1.0.
- **Product name:** PHP IDE. Crate and binary names keep the `phpide` prefix.
- **No UI-thread blocking:** Git, filesystem, processes and language servers
  run in background jobs.

## Phases

Size: S (days) · M (1–2 weeks) · L (3+ weeks).

### Phase 0 — Honest baseline and cleanup (S) — in progress

- [x] `experimental` feature hides frozen tool windows, status bar items and
      Welcome pages; frontend language servers do not start by default.
- [x] No automatic `untitled.php` tab; closing the last tab leaves the editor empty.
- [x] Welcome screen: no requirements text, English strings, no fake keymap picker.
- [x] Starting without a project no longer scans, watches or starts language
      servers for the current directory.
- [x] "Close tab" in the main menu asks before discarding unsaved changes
      (it used to close dirty tabs silently).
- [x] Status bar: fake items removed (Processes, Column mode, Deploy).
- [x] `scripts/baseline.sh` runs every gate and benchmark and writes a report.
- [x] `editor_pipeline` benchmark measures the real per-frame editor cost.
- [x] New documentation set (this file, ARCHITECTURE, CONTRIBUTING, README).
- [x] Run `scripts/baseline.sh` and record the results below.
- [ ] Screenshots of every screen at 1280×800 and 1920×1080 ("before").
- [ ] Remove the old documents (`REQUISITOS.md`, `docs/TAREFAS.md`, …).

**Done when:** baseline recorded, old docs removed, frozen surfaces invisible.

### Phase 1 — Application foundation (M) — done

- [x] egui/eframe 0.31 → 0.36 (new `App::ui` entry point, unified `Panel`,
      menus). The default renderer is now wgpu.
- [x] Every UI string in English, including errors from the other crates
      (frozen Database code excepted).
- [x] Single command registry (`commands.rs`) feeding the main menu, Find
      Action and keyboard shortcuts. Shortcuts are consumed from the most to
      the least specific: egui ignores extra Shift/Alt, so before this
      Ctrl+Shift+Z ran Undo, Ctrl+Shift+N created a file, Ctrl+Alt+S saved,
      Ctrl+Alt+B / Ctrl+Shift+B / Ctrl+Alt+Shift+N fell into Ctrl+B / Ctrl+N,
      and Shift+F2 / Shift+F8 ran F2 / F8. Tests cover the conflicts and press
      the keys for real.
- [x] `main.rs` from 4,817 to under 500 lines: methods moved, unchanged, into
      `app_project`, `app_lsp`, `app_actions`, `app_panels` and `app_editor`;
      the gate tests into their own files. I/O owners live in `services/`.
      Not done: a separate `AppState` — `PhpIdeApp` still holds ~115
      fields. That split happens with the new editor (Phase 3) and shell
      (Phase 4), which replace most of `app_editor` and `app_panels`.
- [x] Git off the UI thread (`services/git.rs`): branch, line markers and
      blame run as jobs. Before, the header and status bar each ran
      `git rev-parse` on **every frame**. A test now fails if render code
      calls Git or scans folders directly.
- [x] Lazy project tree (`services/files.rs`, `project_tree.rs`): folders are
      read when expanded, only visible rows are laid out, no depth limit
      (was a silent 6), `vendor/` visible and collapsed. The search index is
      built in the background ("Indexing…" in the status bar).
- [x] Welcome: `git clone` runs in the background with Cancel; folder
      listing and recent-project checks no longer hit the disk every frame.
- [x] File watcher: access events (reads) no longer count as changes — they
      caused a reload loop every 300 ms — and reloading skips files whose
      content did not change.
- [x] UI tests no longer write to the user's real settings file.
- [x] Interaction tests with `egui_kittest` (`src/ui_tests.rs`): tree
      expand/open, close-with-unsaved-changes from the menu, no frozen
      status items, Welcome start does no project work.
- [x] Integration gates report skips: a missing tool prints `GATE SKIP`
      (counted as "not verified" by `scripts/baseline.sh`), and
      `PHPIDE_REQUIRE_INTEGRATIONS=1` turns skips into failures.

Still synchronous on the UI thread, accepted for now: opening and saving a
file, reading `composer.json` after it changes, the usages preview.

**Done when:** no `git`/filesystem/process call on the UI thread, `main.rs`
under 800 lines, menus and shortcuts come from the registry. ✔

### Phase 2 — Design system (M) — done

- [x] `design/tokens.rs`: semantic colors (shell, Git, debug, coverage, diff,
      search, editor, file types), spacing, radii, type scale, motion and the
      Comfortable/Compact density table from [DESIGN_TOKENS.md](DESIGN_TOKENS.md).
      Tests check WCAG contrast for text, status colors and every syntax role.
- [x] Dark, Light and High Contrast with their own palettes and syntax colors.
      The seven old theme ids (Islands, Darcula, classic) map to these three.
      Resting widgets no longer get the accent color on every border.
- [x] Syntax highlighting by semantic role (`phpide_php::SyntaxRole`): one
      syntect pass, colors from the active theme; LSP semantic tokens use the
      same roles. Editor font size is a setting, independent of IDE zoom.
- [x] Bundled Inter (UI) and JetBrains Mono (code, ligatures removed), egui's
      fonts as fallback; a test checks accents, Greek, Cyrillic, symbols and
      box drawing in every family.
- [x] Tabler Icons 3.2 in `phpide-icons` under semantic names, with a full
      SVG path parser (arcs, curves, compact numbers) and file-type icons.
      `scripts/update-icons.sh` regenerates them from npm.
- [x] Components: Button (primary/secondary/ghost/danger), IconButton,
      TextInput/search field, Select, Checkbox, tooltip, menu items, Tab,
      TreeRow, ListRow, panel frame and header, splitter, StatusItem, Dialog,
      Toasts, EmptyState, ProgressBar and Spinner. All take colors from the
      theme and have a visible focus ring.
- [x] Gallery (`phpide --gallery`) with every state × theme × density.
      `design::gallery::tests::gallery_snapshots` renders it on the CPU (no
      GPU needed) and compares with `crates/phpide-ui/tests/snapshots/`;
      `UPDATE_SNAPSHOTS=1` rewrites them.
- [x] Guard test: no color literal outside `design/tokens.rs`. All 50 literals
      were migrated (gutter, breakpoints, coverage, scrollbar markers, test
      tree, problems list, status bar, Welcome).
- [x] First uses in the shell: tool window rails and headers with icons,
      project tree with file icons and Git-ready rows, editor tabs with file
      icons, quiet header/status bar buttons, frameless editor.

**Done when:** gallery approved; no color literal outside `tokens.rs`
(checked in CI). ✔ Gallery approved on 2026-11-24.

### Phase 3 — New editor `phpide-editor` (L) — done, awaiting review

- [x] Model: rope with invertible change sets (`phpide_core::text`), a change
      log per document (`Document::changes_since`), undo grouped by time and
      edit kind with caret restore (`phpide_core::history`), monotonic
      revision.
- [x] View: only visible lines are laid out, per-line layout cache, vertical
      and horizontal scrolling, current line, indent guides, scrollbars with
      diagnostic/Git/find markers.
- [x] Input: grapheme/word/line/page motions, smart Home, mouse selection
      (word on double click, line on triple click), Alt+click carets,
      Alt+Shift drag column selection, clipboard (whole-line copy and paste),
      IME preedit, auto-indent, bracket pairs and type-over, line/block
      comment toggle, duplicate, delete and move lines, next/all occurrences.
- [x] Highlighting: tree-sitter for PHP, parsed in the background for large
      files and reparsed incrementally per edit; syntect per line for other
      languages; LSP semantic tokens painted on top.
- [x] Gutter and decorations as editor layers: line numbers, breakpoints,
      bookmarks, Git markers, folding chevrons and placeholders, sticky lines,
      diagnostics squiggles, inlay hints, blame, coverage, execution line.
- [x] Find/replace in file: regex, case, whole word, replace one/all.
- [x] Split: two views of one buffer with their own carets, scroll and folds;
      edits in one pane move the other's carets.
- [x] `TextEdit` editor and its projection code removed (`editor_surface`,
      `editor_view`, `folding`, `geometry`, `groups`).
- [x] Interaction tests: `phpide-editor/tests/interaction.rs` (typing,
      grouped undo, multi-cursor, clipboard, folding, split, find/replace,
      read-only) and app-level tests in `phpide-ui` (focus, shortcuts,
      split); snapshots `editor-dark`/`editor-light`.

Shortcut changes: Run moved from Ctrl+R to **Shift+F10** and Debug from
Ctrl+D to **Shift+F9**, so Ctrl+R is Replace and Ctrl+D is Duplicate, as in
IntelliJ.

**Done when:** a 50k-line file opens in under 300 ms, scrolls at 60 fps and a
keystroke costs under 16 ms (measured with `editor_pipeline`); interaction
tests cover typing, grouped undo, multi-cursor, folding and split.
✔ Measured below ("Phase 3 results").

### Phase 4 — Polished shell (M) — done (2026-11-24)

Scope and status per requirement in [REQUIREMENTS.md](REQUIREMENTS.md)
(PhpStorm reference list, MVP items first).

Done (2026-11-24):
- [x] File types: name patterns, `.dist`/`.example` suffixes, shebang and
      `<?php` detection, user patterns and per-file overrides (status bar,
      tree, Settings › File Types), notice for unknown files; `.env`, INI,
      TOML, Dockerfile and ignore files highlighted by a built-in
      line-based engine; TypeScript, SCSS, Twig, Vue, NEON mapped to grammars.
- [x] Project view: follows the active tab (or Select Opened File), keyboard
      navigation, speed search, multi-select, Expand/Collapse All, sorting,
      folders on top, excluded files; New (file, folder, PHP class with
      PSR-4 namespace), Cut/Copy/Paste, Rename, Delete to Trash, Copy Path,
      Open In (file manager, terminal), Open in Right Split.
- [x] Tabs: open next to the active tab, one scrolling row with a list of
      all tabs, parent folder for duplicate names, `*` when modified, path
      tooltip, middle click, Close All/Unpinned, Reopen Closed Tab, copy path,
      double click to keep a preview tab or maximize the editor, oldest
      unmodified tab closes over the limit.
- [x] Home: recent projects with search, remove, missing marker and context
      menu; native folder chooser with a built-in fallback; drop a folder to
      open it; New Project and Get from VCS dialogs; second project opens in
      a new window, this window or asks; Close Project; reopen the last
      project on startup.
- [x] PhpStorm shortcuts: Ctrl+Alt+←/→ back/forward, Alt+←/→ tabs,
      Ctrl+Shift+Backspace last edit, Alt+F1 Select In, F12 last tool window,
      Ctrl+E twice for edited files; editor shortcuts only act when the
      editor (or nothing else) has focus.

- [x] Project windows: Window menu, Ctrl+Alt+] / [ between windows, Close
      Other Projects, every window reopened at startup (one process each,
      registry under `$XDG_RUNTIME_DIR/phpide/windows`); opening a project
      that is already open focuses its window. Attach a project as an
      extra root of the Project view and Go to File.
- [x] Search Everywhere with All / Classes / Files / Symbols / Actions tabs,
      Settings pages as results, recent files first; Go to Class (Ctrl+N).
- [x] Project view: dependency nodes for `vendor/` and `node_modules/`, New
      popup filtered by typing, Copy Reference (`path:line`) and repository
      URL, Select In popup (Project View, Files, Terminal).
- [x] Color Scheme page: Language Defaults inherited per language;
      foreground, background, bold/italic, underline/strikethrough per
      element; live preview. Font page: any monospaced system font, line
      height.
- [x] Tool windows move to left, right, bottom or float; tabs in several
      rows; pinned tabs first; clickable navigation bar.
- [x] New UI header (main menu, Project, Git and Run widgets, Search,
      Notifications, Settings, problems), quiet status bar, Event Log with
      toasts for errors and warnings, Settings rebuilt with the components
      and searchable pages.

Left for later phases: attached projects get no language server of their
own; font ligatures (the text engine does not shape them).

**Done when:** every MVP item of REQUIREMENTS.md is ✅; "after" screenshots
approved in all three themes; open project → edit → save → close works with
the keyboard only.

### Phase 5 — PHP intelligence on the new editor (M) — done (2026-11-24)

Incremental `didChange`, completion with docs and snippets, hover, signature
help, inline diagnostics, definition/implementation, usages, rename, code
actions, formatting, Structure, visible server state with restart.

**Done when:** a real gate with intelephense and phpactor on a small Laravel
fixture passes through UI interaction tests.

Done (2026-11-24):
- [x] Ranged `didChange` built from the editor's change log (a mirror of
      what the server has; full text only when the log no longer reaches
      back or the server asks for full sync). Every request flushes the
      sync first, so answers are about the text on screen.
- [x] Completion keeps the keyboard in the editor and filters as you type
      (prefix, word starts, subsequence; `sortText` breaks ties); opens on
      `->`, `::`, `$`, server trigger characters and identifiers; letter
      badges per kind; documentation panel resolved lazily
      (`completionItem/resolve`); snippets with Tab / Shift+Tab stops;
      `additionalTextEdits` (imports) in one undo step.
- [x] Quick Documentation on mouse rest (500 ms) and Ctrl+Q, with the
      diagnostics under the pointer and Markdown (code blocks, inline code,
      bold, lists). Inline diagnostics: the worst message of a line at its
      end.
- [x] Parameter Info (Ctrl+P, and on `(` / `,`) with the active parameter
      highlighted, following the caret.
- [x] Rename Symbol (Shift+F6, with `prepareRename`), context actions
      (Alt+Enter: quick fixes with their diagnostics, commands, resolve),
      Reformat Code (Ctrl+Alt+L), File Structure popup (Ctrl+F12),
      `workspace/applyEdit` and `window/showMessage` from the server.
- [x] Server state in the status bar with Restart and the server log.
- [x] Gates: `gate_lsp.rs` drives every feature through the UI against a
      built-in fixture server (real LSP over pipes, `phpide_lsp::fixture`);
      `real_servers_on_laravel_fixture` runs the same flow with intelephense
      or phpactor on `tests/fixtures/laravel-mini` when one is installed
      (skipped otherwise — run it locally).

Shortcut changes: Ctrl+P is Parameter Info (Search Everywhere is Shift
Shift), Ctrl+F12 is File Structure (the Structure tool window stays on
Alt+7).

### Phase 6 — Run, debug, test (M) — done (2026-11-24)

Run Configurations UI, Debug tool window (frames, lazy variable tree,
watches, evaluate, conditional breakpoints), PHPUnit tree with rerun of
failed tests, coverage in the gutter, real terminal with a PTY.

Done (2026-11-24):
- [x] Run/Debug Configurations dialog (PHP Script, PHPUnit, Composer
      Script) with validation, saved to `.phpide/run-configurations.json`;
      configuration selector in the header with Current File, temporary
      configurations (up to five, Save Configuration) and Edit
      Configurations….
- [x] Run the test method, test class or script at the caret
      (Ctrl+Shift+F10 / Ctrl+Shift+F9) as a temporary configuration;
      Rerun (Ctrl+F5); Run with Coverage.
- [x] Run tool window: one tab per session (pin, close), output with
      stderr in red and clickable `path:line` links, live PHPUnit tree
      (TeamCity) with Show Passed, failure details and stack-trace links,
      Rerun Failed Tests (a `--filter` of the failed methods).
- [x] Coverage (Clover) in the editor gutter and as "NN% lines covered" in
      the Project view.
- [x] Debug tool window: Resume, Stop, Detach, Show Execution Point, steps,
      Evaluate (Alt+F8), Mute and View Breakpoints (Ctrl+Shift+F8: enabled,
      condition, hit count, go to source); frames; variable tree with lazy
      children (`property_get` on expand) and watches.
- [x] Terminal: new crate `phpide-term` — PTY on Unix (openpty, own
      session and controlling terminal, resize, working directory of the
      foreground process) and an xterm-compatible emulator (colors up to
      true color, alternate screen, scroll regions, bracketed paste,
      application cursor keys, 10 000 lines of scrollback). Tabs of
      sessions; selection and copy; Ctrl+click opens `path:line` links;
      while it has focus, the terminal keeps the keys (only tool-window
      shortcuts and Settings pass through).
- [x] Gates: `gate_run.rs` drives configurations, the test tree, Rerun
      Failed Tests, coverage, run at the caret, output links and a debug
      session through the UI. This environment has neither PHPUnit nor
      Xdebug, so the gate uses a PHP script that speaks PHPUnit's TeamCity
      output and writes Clover, and a DBGp peer that answers like Xdebug;
      the real-PHPUnit tests in `phpide-run` run where PHPUnit is installed.

Shortcut changes: Shift+F10 / Shift+F9 run and debug the selected
configuration (Current File by default, so the old behaviour stays);
Search Everywhere's double Shift now counts only lone Shift taps.

Left for later: the terminal on Windows (ConPTY) and mouse reporting to
programs; terminal in its own window (1.x item below).

### Phase 7 — Git (M)

Changes view with stage by file and by hunk, side-by-side diff (editor in
read-only mode), commit with visible hook output, branches,
fetch/pull/push in the background, log.

### Phase 8 — Hardening and v1.0 release (M)

Accessibility (AccessKit), keyboard navigation everywhere, error log and
local crash report, recovery of unsaved buffers, AppImage/.deb/Flatpak
packages, CI with interaction tests and visual snapshots.

### After v1.0

The maintainer's post-1.0 list (2026-11-24). Each item is planned as a
phase of its own, with the same rule as before: a gate through UI
interaction tests and screenshots in the three themes.

#### 1.x

- [ ] **PHPantom as the PHP analysis engine.** PHPantom
      (<https://github.com/AJenbo/phpantom_lsp>, MIT, Rust; Mago parser,
      phpstorm-stubs embedded, Composer autoload, generics, PHPStan
      annotations, Laravel) becomes the engine PHP IDE ships with, so code
      intelligence works out of the box with no proprietary or external
      server. intelephense and phpactor stay selectable.
  - Decide how it is shipped: a bundled binary started over stdio (what
    the client already does) or linked as a crate and served in process
    over the stream transport (`LspClient::connect`, as the fixture
    server is). Pin a version; update it deliberately.
  - Settings › Languages › PHP: engine (PHPantom / intelephense /
    phpactor / custom command), its options (`.phpantom.toml`: PHP
    version, indexing strategy, diagnostics), restart.
  - Map what it offers to the features of Phase 5 (completion, hover,
    signature help, definition, references, rename, code actions,
    formatting, semantic tokens, inlay hints); hide what it lacks.
  - Attached projects (RF-PRJ-08) as workspace folders of the engine.
  - **Done when:** `real_servers_on_laravel_fixture` passes with PHPantom
    in CI on every commit, with no server installed on the machine, and
    the startup and keystroke latencies are measured on a large project.
- [ ] **Terminal in its own window.** Builds on the PTY terminal of
      Phase 6: the terminal can live in a separate OS window (and move
      back into the tool window), several sessions as tabs, split
      sessions, profiles (shell, working directory, environment), shell
      integration (command boundaries, exit status, clickable file paths
      and `file:line` links that open in the editor), search in the
      scrollback, the editor's font and theme.
  - **Done when:** a long-running command (`php artisan serve`, `npm run
    dev`) keeps running in a detached terminal window while the project
    window is used, and its output links open files.
- [ ] **Minimap on the right of the editor,** as in VS Code: a scaled
      rendering of the file with the visible area highlighted; click and
      drag to scroll; marks for diagnostics, Git changes, search matches
      and the selection; on/off, width and "render characters vs blocks"
      in Settings › Editor. It must cost nothing noticeable on 50k-line
      files (rendered from the highlight spans, cached per revision).
  - **Done when:** the editor benchmark on 54k lines stays within the
    Phase 3 numbers with the minimap on, and snapshot tests cover it.

#### 2.x

- [ ] **Database tools: a complete manager and modeler,** with the
      features and look of the JetBrains database tools
      (<https://www.jetbrains.com/help/webstorm/2026.3/relational-databases.html>).
      `phpide-db` already has SQLite, PostgreSQL and MySQL drivers behind
      the `experimental` feature; this item gives them the new UI and
      completes them.
  - Data sources: connection dialog per driver (host, port, user,
    password from the system keyring, database, SSL/TLS, SSH tunnel),
    test connection, drivers for MySQL/MariaDB, PostgreSQL, SQLite and
    SQL Server first.
  - Database Explorer tool window: schemas, tables, views, columns,
    keys, indexes, routines, triggers; filter, refresh, DDL of an object.
  - Query console and SQL files attached to a data source: SQL
    completion from the schema, run statement / selection / file,
    several result tabs, execution plan (EXPLAIN) as a tree and a
    diagram, cancel.
  - Data editor: grid with sorting, filtering (WHERE / ORDER BY fields),
    paging, editing with a pending-changes preview and Submit /
    Rollback, value editor for long text, JSON and binary; transaction
    mode (auto / manual).
  - Import and export: CSV, JSON, SQL inserts, Markdown/HTML tables;
    dump and restore through `mysqldump`, `pg_dump`, `pg_restore` and
    `psql` when installed.
  - Modeling: database diagrams (tables and relations) generated from a
    schema and editable; create and alter tables, columns, keys and
    indexes through dialogs that show the DDL before running it; schema
    compare and migration script between two data sources.
  - **Done when:** a gate against SQLite (always) and PostgreSQL/MySQL in
    containers (CI) creates a schema from the modeler, edits data in the
    grid, runs queries with completion and exports/imports a table.
- [ ] **HTTP client like Bruno** (<https://www.usebruno.com>): collections
      stored as plain-text files in the repository (Bruno's `.bru` and
      OpenCollection YAML, so collections open in both tools), folders
      and requests in tabs, environments with variables and `.env`
      secrets, REST and GraphQL first (WebSocket and gRPC later),
      authentication (Basic, Bearer, API key, OAuth 2), pre-request and
      post-response scripts and assertions, running a folder, response
      viewer (pretty JSON/XML/HTML, headers, timing, cookies), import from
      Postman and cURL, copy as cURL / PHP code. Offline: no account and
      no cloud.
  - **Done when:** a gate runs a collection from the repository against
    a local test server, with an environment, a script and assertions,
    and the same files open unchanged in Bruno.

#### Not scheduled yet

Still frozen behind `experimental`, to be ordered later: framework
support (Laravel/Symfony), frontend TypeScript, Docker/Remote.

## Known issues carried over

- Detach/Stop race: Stop during the instant `run` unblocks can degrade to
  `ConnectionLost`; the process is still killed (Phase 8).
- Database grid shows an unknown total by default on purpose; an explicit
  count action is pending (after v1.0).
- TLS/SSH for database connections are not implemented (after v1.0).
- The status bar line-ending toggle changes a global setting applied on save,
  not the current buffer (Phase 4).

## Baseline results

Run `20261123-185315` (Linux 7.0, rustc 1.99.1, release profile for benches).

**Gates:** fmt ok; clippy ok after gating two test-only routing helpers
behind `experimental`; 376 workspace tests passed (102 in `phpide-ui` with
`--features experimental`); Composer + intelephense gate passed; run/debug
gate passed (6 tests). The machine has no Xdebug loaded in the system PHP and
no `phpunit` on `PATH`, so Phase 1 must make the run/debug gates report which
binaries they used instead of a plain "ok".

**Editor pipeline, old `TextEdit` editor** (median; frame budget at 60 fps is 16.6 ms):

| Step | 2k lines | 10k lines | 54k lines |
|---|---:|---:|---:|
| rope → String | 0.00 ms | 0.02 ms | 0.16 ms |
| clone + compare | 0.00 ms | 0.01 ms | 0.13 ms |
| syntect highlight, whole file | **67.7 ms** | **338.8 ms** | **1716.6 ms** |
| layout, cache miss (after edit) | 5.7 ms | 28.3 ms | 165.0 ms |
| layout, cache hit (idle frame) | 1.4 ms | 7.8 ms | 38.7 ms |
| tree-sitter folding (per edit) | 12.5 ms | 67.2 ms | 340.5 ms |
| **Frame without edit** | **69 ms (~14 fps)** | **347 ms (~3 fps)** | **1.76 s** |
| **Frame after a keystroke** | **86 ms** | **434 ms** | **2.22 s** |

Every repaint (typing, mouse move, hover) pays the full syntect cost, so even
a 2k-line file misses the frame budget four times over. Copying the rope and
comparing strings is negligible; the cost is highlighting and laying out the
whole file. Phase 3 targets: highlight and lay out only the visible lines,
reparse incrementally, and stay under 16.6 ms per frame at 54k lines.

Release binary: 28 MB.

## Phase 3 results

`cargo bench -p phpide-editor --bench editor_pipeline` (release, Linux,
real egui frames of the widget at 1280×800 plus tessellation; budget 16.6 ms):

| Step | 2k lines | 10k lines | 54k lines |
|---|---:|---:|---:|
| Open + first frame | 44.8 ms¹ | 2.0 ms | 9.4 ms |
| Background parse + structure | 8.3 ms | 128 ms | 637 ms |
| Scroll frame, median / worst | 0.22 / 0.52 ms | 0.59 / 1.16 ms | 0.79 / 1.13 ms |
| Idle frame | 0.13 ms | 0.14 ms | 0.19 ms |
| Keystroke frame, median / worst | 0.68 / 1.09 ms | 1.23 / 2.76 ms | 4.65 / 7.16 ms |

¹ Includes the font atlas and query compilation, paid once per process.

The first parse of a large file runs on a worker thread; frames before it
arrives show plain text, then the tree catches up by replaying the edits made
meanwhile. Folds and blocks (`Structure`) are recomputed in the background,
at most every 300 ms.

