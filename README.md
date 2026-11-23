# PHP IDE

A PHP-focused IDE written in Rust for Linux (X11/Wayland). Free and open
source under MIT OR Apache-2.0, with no proprietary components.

> **Status: pre-1.0, under active rework.** The language, debugger, test and
> Git back ends work, but the editor surface and the UI are being rebuilt.
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
| Shift Shift / Ctrl+P | Search Everywhere |
| Ctrl+Shift+N / Ctrl+E / Ctrl+Shift+A | Go to File / Recent Files / Find Action |
| Ctrl+S / Ctrl+Z / Ctrl+Shift+Z | Save / Undo / Redo |
| Ctrl+N / Ctrl+F4 | New file / Close tab |
| Ctrl+Space | Completion |
| F12 / Ctrl+B | Go to definition |
| Ctrl+Shift+F | Find in files |
| F2 / Shift+F2 | Next / previous problem |
| Ctrl+R / Ctrl+D / Ctrl+F2 | Run file / Debug with Xdebug / Stop |
| F9 | Toggle breakpoint (Resume while debugging) |
| F7 / F8 / Shift+F8 | Step into / over / out |
| Alt+1…0, Alt+F12 | Tool windows, Terminal |
| Ctrl+Alt+S | Settings |

## Documentation

- [docs/ROADMAP.md](docs/ROADMAP.md) — plan and phase status
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, threading and data flow
- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates and baseline

## License

MIT OR Apache-2.0 for the project's own code. Dependencies keep their own
licenses (see `cargo tree`). Intelephense is freeware and is installed
locally through npm, never redistributed.
