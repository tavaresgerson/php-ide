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
(checked in CI). Guard ✔ — gallery awaiting approval.

### Phase 3 — New editor `phpide-editor` (L)

1. Model: rope, multi-cursor selections, invertible transactions, undo
   grouped by time and edit kind, monotonic revision for LSP.
2. View: visible lines only, per-line layout cache, smooth and horizontal
   scrolling, current line, indent guides.
3. Input: character/word/line/page motions, keyboard and mouse selection,
   column selection, clipboard, IME, auto-indent, bracket pairs and
   matching, comment toggle, duplicate/move line.
4. Incremental highlighting with tree-sitter; LSP semantic tokens on top.
5. Gutter and decorations as editor layers: line numbers, breakpoints,
   bookmarks, Git markers, native folding, sticky lines, diagnostics,
   inlay hints, blame, coverage.
6. Find/replace in file with regex.
7. Real split: two views of one document, independent caret and scroll.
8. Remove the `TextEdit`-based editor and its projection code.

**Done when:** a 50k-line file opens in under 300 ms, scrolls at 60 fps and a
keystroke costs under 16 ms (measured with `editor_pipeline`); interaction
tests cover typing, grouped undo, multi-cursor, folding and split.

### Phase 4 — Polished shell (M)

Window header, single-row tabs, Project tool window (icons, keyboard,
context menu, Git status), icon rails, quiet status bar, breadcrumbs,
palette, Welcome and Settings rebuilt with the components, async clone with
progress, unified notifications.

**Done when:** "after" screenshots approved in all three themes; open
project → edit → save → close works with the keyboard only.

### Phase 5 — PHP intelligence on the new editor (M)

Incremental `didChange`, completion with docs and snippets, hover, signature
help, inline diagnostics, definition/implementation, usages, rename, code
actions, formatting, Structure, visible server state with restart.

**Done when:** a real gate with intelephense and phpactor on a small Laravel
fixture passes through UI interaction tests.

### Phase 6 — Run, debug, test (M)

Run Configurations UI, Debug tool window (frames, lazy variable tree,
watches, evaluate, conditional breakpoints), PHPUnit tree with rerun of
failed tests, coverage in the gutter, real terminal with a PTY
(`portable-pty` + `alacritty_terminal`).

### Phase 7 — Git (M)

Changes view with stage by file and by hunk, side-by-side diff (editor in
read-only mode), commit with visible hook output, branches,
fetch/pull/push in the background, log.

### Phase 8 — Hardening and v1.0 release (M)

Accessibility (AccessKit), keyboard navigation everywhere, error log and
local crash report, recovery of unsaved buffers, AppImage/.deb/Flatpak
packages, CI with interaction tests and visual snapshots.

### After v1.0

Unfreeze in this order: Database Tools (`phpide-db` is solid; it needs the
new UI), frameworks (Laravel/Symfony), HTTP client, frontend TypeScript,
Docker/Remote.

## Known issues carried over

- Detach/Stop race: Stop during the instant `run` unblocks can degrade to
  `ConnectionLost`; the process is still killed (Phase 6).
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

**Editor pipeline** (`cargo bench -p phpide-ui --bench editor_pipeline`,
median; frame budget at 60 fps is 16.6 ms):

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
