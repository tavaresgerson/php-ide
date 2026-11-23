# Architecture

## Workspace

| Crate | Responsibility |
|---|---|
| `phpide-core` | `Document` (rope), `Buffer` (tab + undo), `Workspace`, file tree, diagnostics, byte/char/UTF-16 offsets, revisions, `JobSystem`, versioned settings, `IdeError`, logger with secret redaction, atomic save |
| `phpide-php` | PHP metadata, Composer discovery (language level, PSR-4/PSR-0 roots), syntect highlighting |
| `phpide-syntax` | tree-sitter-php: structural blocks, folding ranges, offline syntax errors |
| `phpide-lsp` | LSP client: JSON-RPC over stdio on threads (no async runtime), capabilities, document sync, completion, workspace edits |
| `phpide-dbgp` | DBGp (Xdebug): listener, breakpoints, stepping, stack, variables, path mappings |
| `phpide-exec` | External processes: executable discovery, spawn, timeout, Stop with process-group kill, streamed stdout/stderr |
| `phpide-run` | Run configurations, sessions, PHPUnit (scopes + TeamCity tree), Clover coverage |
| `phpide-vcs` | Git through the `git` CLI: repo detection, porcelain `-z` status, diff, stage/commit, log, line markers, blame |
| `phpide-icons` | SVG icon pipeline: manifest with licenses, semantic registry, cache, tint |
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

**Known violation (Phase 1):** Git branch/markers/blame, the project tree
scan and `git clone` from the Welcome screen still run on the UI thread.

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
