# Changelog

All notable changes to PHP IDE. Dates are UTC.

## [Unreleased] — 1.0 release candidate

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
