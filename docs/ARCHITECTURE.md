# Architecture

## Workspace

| Crate | Responsibility |
|---|---|
| `phpide-core` | `Document` (rope), `Buffer` (tab + undo), `Workspace`, file tree, diagnostics, byte/char/UTF-16 offsets, revisions, `JobSystem`, versioned settings, `IdeError`, logger with secret redaction, atomic save |
| `phpide-php` | PHP metadata, Composer discovery (language level, PSR-4/PSR-0 roots), syntect highlighting by semantic role (`SyntaxRole`) |
| `phpide-syntax` | tree-sitter-php: structural blocks, folding ranges, offline syntax errors |
| `phpide-lsp` | LSP client: JSON-RPC over stdio on threads (no async runtime), capabilities, document sync, completion, workspace edits |
| `phpide-dbgp` | DBGp (Xdebug): listener, breakpoints, stepping, stack, variables, path mappings |
| `phpide-exec` | External processes: executable discovery, spawn, timeout, Stop with process-group kill, streamed stdout/stderr |
| `phpide-run` | Run configurations, sessions, PHPUnit (scopes + TeamCity tree), Clover coverage |
| `phpide-vcs` | Git through the `git` CLI: repo detection, porcelain `-z` status, diff, stage/commit, log, line markers, blame |
| `phpide-icons` | Tabler Icons 3.2 (outline) under semantic names, SVG → polyline parser (all path commands), file-type mapping |
| `phpide-db` | Database Tools (frozen until v1.0): SQLite/PostgreSQL/MariaDB drivers, secrets, console, grid, optimistic edits |
| `phpide-ui` | egui/eframe application (binary `phpide-ui`) |

Dependency direction: `phpide-ui` depends on every crate; domain crates
depend only on `phpide-core` (and `phpide-exec` where they spawn processes).
`phpide-php` depends on `eframe` only to return an egui `LayoutJob` from the
highlighter; this goes away with the new editor.

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
| `app_editor.rs` | Editor surface (to be replaced by `phpide-editor` in Phase 3) |
| `services/` | Background I/O owners (`git`, `files`) |
| `design/` | Design system: `tokens.rs` (the only place with color literals), `theme.rs` (Dark/Light/High Contrast, density, applied to `egui::Style`; `design::current(ctx)` returns the active theme), `fonts.rs`, `components/`, `gallery.rs`, `snapshot.rs` (CPU renderer for snapshot tests) |
| `project_tree.rs`, `header.rs`, `tabs.rs`, `statusbar.rs`, `welcome.rs`, … | Individual shell surfaces |
| `ui_tests.rs`, `gate_*.rs` | Interaction tests (`egui_kittest`) and gates |

## Editor today (to be replaced in Phase 3)

`PhpIdeApp::editor` renders an `egui::TextEdit`. On every frame it copies the
rope into a `String`, clones it for the widget, highlights the whole file with
syntect, lays it out and compares the result with the original to derive the
edit. Folding projects a reduced text into the widget and maps edits back.
Gutter, sticky lines, hints and markers are painted on top using the
`TextEdit` galley geometry. `benches/editor_pipeline.rs` measures this cost.

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
