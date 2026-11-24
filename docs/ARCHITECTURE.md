# Architecture

## Workspace

| Crate | Responsibility |
|---|---|
| `phpide-core` | `Document` (rope, change sets, change log), `Buffer` (tab + grouped undo history), `Workspace`, file tree, diagnostics, byte/char/UTF-16 offsets, revisions, `JobSystem`, versioned settings, `IdeError`, logger with secret redaction, atomic save |
| `phpide-php` | PHP metadata, Composer discovery (language level, PSR-4/PSR-0 roots), syntect highlighting by semantic role (`SyntaxRole`), per line for non-PHP files (`LineHighlighter`) |
| `phpide-syntax` | tree-sitter-php: structural blocks, folding ranges, offline syntax errors |
| `phpide-lsp` | LSP client: JSON-RPC over stdio on threads (no async runtime), capabilities, document sync, completion, workspace edits |
| `phpide-dbgp` | DBGp (Xdebug): listener, breakpoints, stepping, stack, variables, path mappings |
| `phpide-exec` | External processes: executable discovery, spawn, timeout, Stop with process-group kill, streamed stdout/stderr |
| `phpide-run` | Run configurations, sessions, PHPUnit (scopes + TeamCity tree), Clover coverage |
| `phpide-vcs` | Git through the `git` CLI: repo detection, porcelain `-z` status, diff, stage/commit, log, line markers, blame |
| `phpide-icons` | Tabler Icons 3.2 (outline) under semantic names, SVG → polyline parser (all path commands), file-type mapping |
| `phpide-db` | Database Tools (frozen until v1.0): SQLite/PostgreSQL/MariaDB drivers, secrets, console, grid, optimistic edits |
| `phpide-editor` | The code editor: multi-cursor selections, motions and edit commands, incremental tree-sitter highlighting, folds, find/replace, and the egui widget (virtualized painting, input, gutter layers) |
| `phpide-ui` | egui/eframe application (binary `phpide-ui`) |

Dependency direction: `phpide-ui` depends on every crate; domain crates
depend only on `phpide-core` (and `phpide-exec` where they spawn processes).
`phpide-editor` depends on `phpide-core`, `phpide-php`, `phpide-syntax` and
egui; it knows nothing about the app (themes, LSP, debugger): the app passes
an `EditorStyle` built from its theme and a `Decorations` value per frame.

## Threading model

- egui runs the UI on the main thread and repaints on input or on request.
- Language servers: one process per server, a writer and a reader thread;
  responses arrive on `mpsc` channels the UI polls every frame. Requests are
  tagged by origin, operation, document and revision (`routing.rs`) so stale
  answers are dropped.
- `phpide-core::jobs::JobSystem` runs cancellable background jobs; project
  switch and exit shut it down.
- `phpide-exec` streams process output through channels; Stop kills the
  whole process group.
- DBGp listens on port 9003 in its own thread.

- `phpide-ui/src/services/` owns UI-side I/O: `git.rs` (branch, line
  markers, blame) and `files.rs` (search index, lazily loaded tree). Each
  runs its work as jobs and exposes cached state; a test fails if render
  code calls Git or scans folders directly.

Still synchronous on the UI thread: opening and saving a file, reading
`composer.json` after it changes, the usages preview.

## `phpide-ui` layout

| Module | Role |
|---|---|
| `main.rs` | `PhpIdeApp` state, `eframe::App` frame loop, startup |
| `commands.rs` | Command registry: menu, Find Action and shortcuts |
| `app_project.rs` | Open/switch project, settings, theme, watcher, services polling |
| `app_lsp.rs` | Language server lifecycle, requests, completion, symbols |
| `app_actions.rs` | Command dispatcher, shortcuts, close/save, run/debug actions |
| `app_panels.rs` | Tool window panels and modal dialogs |
| `app_editor.rs` | Editor panes: decorations from app state, breadcrumbs, split layout, editor output (breakpoints, Ctrl+click, completion triggers) |
| `editor_area.rs` | Per-buffer `Syntax` and `EditorView`, the split pane, focus |
| `services/` | Background I/O owners (`git`, `files`) |
| `design/` | Design system: `tokens.rs` (the only place with color literals), `theme.rs` (Dark/Light/High Contrast, density, applied to `egui::Style`; `design::current(ctx)` returns the active theme), `fonts.rs`, `components/`, `gallery.rs`, `snapshot.rs` (CPU renderer for snapshot tests) |
| `project_tree.rs`, `header.rs`, `tabs.rs`, `statusbar.rs`, `welcome.rs`, … | Individual shell surfaces |
| `ui_tests.rs`, `gate_*.rs` | Interaction tests (`egui_kittest`) and gates |

## Editor

- **Text model** (`phpide-core`): every edit is a `ChangeSet` (sorted,
  non-overlapping `Change`s in char offsets) applied by `Document::apply`,
  which bumps the revision and keeps the last 512 applied changes, with
  tree-sitter byte edits, in a log. `Buffer::edit` records the change and its
  inverse in `History`; typing and deleting merge into one undo group while
  they continue within 1.5 s from where the last edit left the carets.
- **View** (`phpide-editor::view::EditorView`): selection, scroll, folds and
  find state of one pane. Before each frame it replays
  `Document::changes_since` to map its carets and folds through edits made
  elsewhere (other pane, LSP workspace edit, reload). Two panes of one buffer
  are two views.
- **Commands** (`edit.rs`, `movement.rs`) are pure functions from rope +
  selection to an `Edit` (change set + new selection + kind), so they are
  tested without egui.
- **Syntax** (`syntax.rs`): one per buffer. PHP uses tree-sitter: the first
  parse of files over 128 KB runs on a worker thread, later edits reparse
  incrementally on the UI thread (~3 ms at 54k lines), and highlight queries
  run only over the visible lines. Folds and blocks are computed in the
  background. Other languages use syntect with a per-line state checkpoint.
- **Widget** (`view/widget.rs`): lays out only visible rows, caches each
  line's galley until its text or style changes, and paints text, current
  line, selections, find matches, indent guides, fold placeholders,
  squiggles, inlay hints, carets and IME preedit, then the gutter (numbers,
  breakpoints, bookmarks, Git, folding, coverage, blame), sticky lines and
  scrollbars with markers. `benches/editor_pipeline.rs` measures it.

## Scope flag

`phpide-ui/src/scope.rs` exposes `EXPERIMENTAL`, set by the `experimental`
Cargo feature. Frozen tool windows return `false` from
`ToolWindow::is_available`, so `ToolWindows::set_visible` refuses to open
them from shortcuts, menus or rails. Frozen code stays compiled and is
checked in CI with `--features phpide-ui/experimental`.

## Settings and data

- User settings: `$XDG_CONFIG_HOME/phpide/settings.json` (versioned, atomic
  write with `.bak`, invalid JSON kept as `.invalid`).
- Project data: `.phpide/` inside the project (plugins, data sources).
- Logs go to stderr with session/job/document correlation and secrets
  redacted.
