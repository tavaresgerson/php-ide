# Changelog

All notable changes to PHP IDE. Dates are UTC.

## [Unreleased] — 1.1

Phase 11, PHPantom as the PHP engine (2026-11-25).

### Added

- PHPantom 1.1.0 ships with the IDE as `<prefix>/lib/phpide/phpantom_lsp`
  (separate process; .deb, AppImage and Flatpak), with its license and the
  phpstorm-stubs one. `scripts/fetch-phpantom.sh` downloads the release
  pinned in `third_party/phpantom.lock` and checks its SHA-256; `--update
  VERSION` pins another one.
- Settings › PHP › Language Server: Automatic, PHPantom, intelephense,
  phpactor or a custom command, for every project or for this project only
  (`.phpide/project.json`); the server in use with its version and path;
  Restart, Show Log and Open `.phpantom.toml`.
- Indexing progress in the status bar ("PHPantom: Full index… 42%") and in
  the Event Log.
- Go to Super Method (Ctrl+U), Type Hierarchy (Ctrl+H) and Call Hierarchy
  (Ctrl+Alt+H) in a new Hierarchy tool window, Extend/Shrink Selection
  (Ctrl+W / Ctrl+Shift+W), code lenses after declarations ("2 references",
  "2 implementations"; a click lists them) and document links (Ctrl+click
  on a `require` path opens the file).
- Go to Implementation lists every implementation when there are several.
- The excluded folders and the PHP file types reach PHPantom at startup
  and whenever they change; attached projects are workspace folders.
- A crashed server is started again after 1, 2 and 4 seconds; after three
  restarts in five minutes the IDE stops and says so.

### Fixed

- `initialized` was sent with `null` parameters: PHPantom ignored it and
  never indexed the project.
- Semantic tokens past the first line were placed one line-offset too far
  (and could panic on short files).

Phase 10, icons and menus (2026-11-25).

### Added

- Editor context menu: a right click in the text offers context actions,
  Cut/Copy/Paste, Copy Path/Reference, Go To (Definition, Implementation,
  Type Declaration, Usages), Refactor › Rename, Reformat Code, Run and Debug
  of the file, breakpoint and bookmark, Git (Annotate with Git Blame, Show
  History, Compare with HEAD, Rollback) and Open In. Items that need a
  language server appear only when the server offers them. A right click
  on the gutter toggles a breakpoint or a bookmark on that line.
- Icons in the main menu, Find Action and Search Everywhere, from one list
  per command; menus keep the icon column, so items line up.
- Check marks and boxed icons for on/off items (tool windows, Project view
  options) and for the chosen entry of a list (scope, sort order, file
  type, line separator, indentation, PHP level, themes, run configurations).

### Changed

- Every menu uses the same rows: the main menu, the tool window Options
  and Move To menus, the Project view Options and scope menus, the status
  bar popups, the color scheme actions, the branch popup, the run
  configuration selector and Quick Switch Scheme.
- Project view Options stays open while settings are switched, so several
  can be changed in a row.

### Fixed

- Menus built from the new rows opened as wide as egui's default area
  (600 px) instead of as wide as their items.
- A right click on the editor while its menu was open only closed the menu;
  it now opens again where the click landed.
- Frontend language servers (TypeScript, HTML, CSS) got edits only once
  per frame, after the requests made in it: completion while typing could
  answer for the text before the last keystroke. Every request now sends
  the pending edits first, as for PHP, and only changed buffers are copied.
- The TypeScript gate asked for completion before tsserver had loaded the
  project and failed on the answer it gets then (CI, `experimental`).

## [1.0.0] — release candidate, awaiting the tag

Phase 8, hardening (2026-11-24).

### Added

- Recovery of unsaved changes: open buffers are saved to a recovery folder
  while they change and offered back after a crash.
- Error log with rotation (Help › Show Log, Show Log in Files) and a local
  crash report shown once on the next start.
- Accessibility: every control has a name for screen readers; Tab order in
  dialogs; a commit can be made with the keyboard only.
- Project view: Mark Directory as Sources / Tests / Excluded; file nesting
  with editable rules; file size and date; indent guides; Quick Preview on
  Space; scope views (Open Files, Changed Files, Production, Tests).
- Home: project groups, project icons (`.phpide/icon.png`) and initials
  avatars; a default project directory for New Project and Get from VCS.
- Replace in Files (Ctrl+Shift+R) with a folder scope and a file mask;
  Find in Files and Replace in Files on folders from the Project view.
- Search Everywhere: file-type filter and non-project items (vendor/,
  node_modules/, excluded folders).
- Recent Locations (Ctrl+Shift+E).
- Editor tabs: which tab activates after closing, placement (top or none),
  close button position, alphabetical order.
- Color schemes: duplicate, rename, delete, export and import.
- Configurable keymap (Settings › Keymap); Alt+Enter in Find Action
  assigns a shortcut.
- Line separator per file, shown and converted from the status bar.
- Packages: .deb, AppImage and Flatpak; desktop entry, AppStream metadata
  and app icon; `phpide --version` and `--help`.
- CI: fmt, clippy, all tests in parallel, frozen-feature tests, snapshot
  artifacts, packages, releases on tags.

### Fixed

- Debugger: Stop or Detach right after Resume could be lost, leaving the
  session hanging; a Stop queued behind another command ended as a lost
  connection.
- Go to Definition, Implementation and Type Declaration did nothing when
  the server answered with `LocationLink`s, which this client invites by
  declaring `linkSupport`; both answer forms are read now.
- View Breakpoints grew taller every frame until its top and bottom edges
  left the window; the list and details now have a fixed height.
- Context menus (Project view, editor tabs, Home, Commit, Git log and
  branches, diff, Debug variables and watches, Terminal) show icons on
  their items (Cut, Copy, Paste, Rename, Delete…), and submenus line up
  with the other items.
- The Structure tool window was empty without a language server: it now
  lists classes, interfaces, traits, enums, functions, methods, properties
  (constructor promotion included), constants and enum cases from the
  syntax tree, and uses the server's answer when there is one. File
  Structure (Ctrl+F12) works the same way. An empty window says why.

### Changed

- Ctrl+Shift+F is Find in Files (it toggled the Find tool window; Alt+3
  still does).
- The Wayland app id is `io.github.tavaresgerson.PhpIde`, matching the
  desktop entry.

## Earlier phases

Phases 0 to 7 (foundation, design system, the new editor, the shell, PHP
intelligence, run/debug/test, Git) are described in
[docs/ROADMAP.md](docs/ROADMAP.md).
