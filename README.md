# PHP IDE

A PHP-focused IDE written in Rust for Linux (X11/Wayland). Free and open
source under MIT OR Apache-2.0, with no proprietary components.

> **Status: pre-1.0, usable for daily PHP work.** The code editor was
> rewritten (multi-cursor, incremental highlighting, folding, split,
> find/replace; smooth on 50k-line files), the shell follows PhpStorm's
> navigation, and PHP intelligence runs on intelephense or phpactor
> (completion, documentation, parameter info, rename, quick fixes,
> formatting). Run/debug/test tool windows and Git are reworked next.
> Expect rough edges. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Run

```bash
cargo run --release -p phpide-ui -- /path/to/php-project [files...]
```

Without a project argument the Welcome screen opens. Extra arguments open
files in tabs.

Features frozen until v1.0 (Database Tools, frontend language servers,
Services, Remote Host, Deployment) are hidden by default. To try them:

```bash
cargo run --release -p phpide-ui --features experimental -- /path/to/project
```

To see every UI component in every theme, density and state:

```bash
cargo run -p phpide-ui -- --gallery
```

## PHP language server

PHP IDE talks to an external language server. It looks for one in this order:

1. `PHPIDE_LSP` environment variable, e.g. `PHPIDE_LSP="phpactor language-server"`
2. `third_party/node_modules/.bin/intelephense` (install with `npm ci --prefix third_party`)
3. `intelephense` on `PATH`
4. `phpactor` on `PATH`

Without a server, syntax errors, folding and structure still work offline
through tree-sitter.

## Main shortcuts

| Keys | Action |
|---|---|
| Shift Shift | Search Everywhere |
| Ctrl+N / Ctrl+Shift+N / Ctrl+Alt+Shift+N | Go to Class / File / Symbol |
| Ctrl+E (twice: edited only) / Ctrl+Shift+A | Recent Files / Find Action |
| Ctrl+S / Ctrl+Z / Ctrl+Shift+Z | Save / Undo / Redo |
| Ctrl+F4 / Ctrl+Shift+T | Close tab / Reopen closed tab |
| Alt+←/→ / Ctrl+Tab | Previous/next tab / Switcher |
| Ctrl+Alt+←/→ / Ctrl+Shift+Backspace | Back / Forward / Last edit location |
| Alt+F1 | Select In: Project view, file manager, terminal |
| Ctrl+Shift+C / Ctrl+Alt+Shift+C | Copy path / Copy reference (`path:line`) |
| Ctrl+Alt+] / Ctrl+Alt+[ | Next / previous project window |
| Ctrl+Space | Completion (also opens while typing; Tab walks snippet placeholders) |
| Ctrl+Q / Ctrl+P | Quick Documentation / Parameter Info |
| Alt+Enter / Shift+F6 / Ctrl+Alt+L | Context actions / Rename symbol / Reformat code |
| Ctrl+F12 | File Structure |
| Ctrl+B | Go to definition |
| Ctrl+F / Ctrl+R / F3 | Find / Replace / Find next in the editor |
| Ctrl+Shift+F | Find in files |
| F2 / Shift+F2 | Next / previous problem |
| Ctrl+/ / Ctrl+D / Ctrl+Y | Comment line / Duplicate / Delete line |
| Alt+Shift+↑/↓ | Move line up / down |
| Alt+J / Ctrl+Alt+Shift+J / Alt+click | Next occurrence / All occurrences / Add caret |
| Ctrl+Shift+− / Ctrl+Shift+= | Collapse / Expand fold |
| Shift+F10 / Shift+F9 / Ctrl+F2 | Run / Debug the selected configuration / Stop |
| Ctrl+Shift+F10 / Ctrl+Shift+F9 / Ctrl+F5 | Run / Debug the test or script at the caret / Rerun |
| Alt+F8 / Ctrl+Shift+F8 | Evaluate expression / View breakpoints (conditions) |
| F9 | Toggle breakpoint (Resume while debugging) |
| F7 / F8 / Shift+F8 | Step into / over / out |
| Alt+1…0, Alt+F12, F12 | Tool windows, Terminal (keeps the keys while focused), last tool window |
| Alt+Insert / Shift+F6 / Delete | New / Rename / Delete in the Project view (typing searches) |
| Ctrl+Shift+F12 / Shift+Esc | Hide all tool windows (maximize editor) / Hide active |
| Ctrl+Alt+S | Settings |

## Documentation

- [docs/ROADMAP.md](docs/ROADMAP.md) — plan and phase status
- [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) — PhpStorm reference requirements and their status
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, threading and data flow
- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates and baseline

## License

MIT OR Apache-2.0 for the project's own code. Dependencies keep their own
licenses (see `cargo tree`). Bundled assets: Tabler Icons (MIT), Inter and
JetBrains Mono (SIL OFL 1.1) — see `crates/phpide-icons/assets/manifest.json`
and `crates/phpide-ui/assets/fonts/README.md`. Intelephense is freeware and is installed
locally through npm, never redistributed.
