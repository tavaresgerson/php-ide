# PHP IDE

A PHP-focused IDE written in Rust for Linux (X11/Wayland). Free and open
source under MIT OR Apache-2.0, with no proprietary components.

> **Status: release candidate for 1.0.** The code editor (multi-cursor,
> incremental highlighting, folding, split, find/replace; smooth on
> 50k-line files), PhpStorm's navigation and shortcuts, PHP intelligence on
> the bundled PHPantom language server (completion, documentation,
> parameter info, rename, quick fixes, formatting, hierarchies, code
> lenses; intelephense and phpactor also work), run configurations, the Xdebug
> debugger, PHPUnit with coverage, a real terminal and Git (commit with
> hunk staging, side-by-side diff, branches, push/pull, log) are in. Unsaved
> work survives a crash, every control is named for screen readers, and
> there are .deb, AppImage and Flatpak packages. See
> [docs/ROADMAP.md](docs/ROADMAP.md) and [CHANGELOG.md](CHANGELOG.md).

## Install

Packages are attached to each GitHub release (built by CI):

```bash
sudo apt install ./phpide_<version>_amd64.deb      # Debian, Ubuntu
chmod +x PHP_IDE-<version>-x86_64.AppImage && ./PHP_IDE-<version>-x86_64.AppImage
flatpak install --user PHP_IDE.flatpak              # see the note below
```

The IDE uses the PHP, Composer, Git and Node.js of your system. Inside the
Flatpak sandbox it only sees the sandbox's tools (Git, no PHP) for now, so
prefer the .deb or the AppImage for PHP work. To build the packages
yourself, see [CONTRIBUTING.md](CONTRIBUTING.md#packages).

Logs are in `~/.local/state/phpide/logs/` (Help › Show Log); a crash leaves
a report in `~/.local/state/phpide/crashes/`, shown on the next start and
never sent anywhere.

## Run from source

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

PHP IDE ships [PHPantom](https://github.com/PHPantom-dev/phpantom_lsp)
(MIT), a PHP language server in Rust, and runs it as a separate process:
the packages install it as `<prefix>/lib/phpide/phpantom_lsp`. In a source
checkout, `bash scripts/fetch-phpantom.sh` downloads the release pinned in
`third_party/phpantom.lock` (its SHA-256 is checked) into
`third_party/phpantom/`.

Settings › PHP › Language Server chooses the engine — Automatic, PHPantom,
intelephense, phpactor or a command of your own — for every project or for
one project (stored in `.phpide/project.json`). It shows the server in use,
restarts it, opens its log and opens the project's `.phpantom.toml`
(created by `phpantom_lsp init --yes` only when you ask). Automatic looks,
in this order, for:

1. the `PHPIDE_LSP` environment variable (a whole command line; it wins
   over the setting), e.g. `PHPIDE_LSP="phpactor language-server"`
2. PHPantom installed with the IDE, then `third_party/phpantom/`, then
   `phpantom_lsp` on `PATH`
3. `third_party/node_modules/.bin/intelephense` (`npm ci --prefix third_party`),
   then `intelephense` on `PATH`
4. `phpactor` on `PATH`

PHPantom gets the folders marked Excluded and the extensions File Types
maps to PHP, and attached projects as workspace folders. If the server
crashes it is started again, waiting longer each time (three times in five
minutes at most).

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
| Ctrl+B / Ctrl+Alt+B / Ctrl+U | Go to definition / implementation / super method |
| Ctrl+H / Ctrl+Alt+H | Type Hierarchy / Call Hierarchy |
| Ctrl+W / Ctrl+Shift+W | Extend / Shrink Selection |
| Ctrl+click | Go to definition, or follow a link (`require` paths) |
| Ctrl+F / Ctrl+R / F3 | Find / Replace / Find next in the editor |
| Ctrl+Shift+F / Ctrl+Shift+R | Find in files / Replace in files (folder scope, file mask) |
| Ctrl+Shift+E (twice: changed only) | Recent Locations |
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
| Ctrl+K / Ctrl+Shift+K / Ctrl+T | Commit / Push / Update Project (Git) |
| Ctrl+Shift+` / Ctrl+Alt+Z | Git branches / Rollback |
| F7 / Shift+F7 (in a diff) | Next / previous change |
| Alt+1…0, Alt+F12, F12 | Tool windows, Terminal (keeps the keys while focused), last tool window |
| Alt+Insert / Shift+F6 / Delete | New / Rename / Delete in the Project view (typing searches) |
| Ctrl+Shift+F12 / Shift+Esc | Hide all tool windows (maximize editor) / Hide active |
| Space (Project view) | Quick Preview |
| Alt+Enter (in Find Action) | Assign a shortcut to the action |
| Ctrl+Alt+S | Settings (Keymap: change any shortcut) |

## Documentation

- [docs/ROADMAP.md](docs/ROADMAP.md) — plan and phase status
- [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) — PhpStorm reference requirements and their status
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, threading and data flow
- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates and baseline

## License

MIT OR Apache-2.0 for the project's own code. Dependencies keep their own
licenses (see `cargo tree`). Bundled assets: Tabler Icons (MIT), Inter and
JetBrains Mono (SIL OFL 1.1) — see `crates/phpide-icons/assets/manifest.json`
and `crates/phpide-ui/assets/fonts/README.md`. The packages include PHPantom
(MIT), which embeds phpstorm-stubs (Apache-2.0): see
`packaging/third-party/`. Intelephense is freeware and is installed
locally through npm, never redistributed.
