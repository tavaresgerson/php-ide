# Reference requirements (PhpStorm) — status

Functional requirements taken from the PhpStorm 2026.2/2026.3 documentation
for projects, the Project view, file search, editor tabs, file types and
highlighting, and the editor layout (source: the maintainer's
"Requisitos de referência — PhpStorm", 2026-11-24). Shortcuts are the
Windows/Linux default keymap. **Priority** is the maintainer's cut: MVP,
Later, Optional.

**Status:** ✅ done · 🟡 partial (the note says what is missing) · ⬜ planned
(the phase that takes it).

## 1. Projects

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-PRJ-01 | Home with recent projects, **Open** and **New Project** | MVP | ✅ |
| RF-PRJ-02 | Open a project from a folder (folder chooser; drop a folder) | MVP | ✅ Native chooser (zenity/kdialog) with a built-in browser as fallback; folders dropped on Home open |
| RF-PRJ-03 | Recent projects on Home and in File › Recent Projects | MVP | ✅ |
| RF-PRJ-04 | Search field that filters the recent list | MVP | ✅ |
| RF-PRJ-05 | Remove a project from the recent list (files stay) | MVP | ✅ Hover ✕ and context menu |
| RF-PRJ-06 | Project groups | Later | ✅ Groups on Home: move to group, new, rename, remove, fold |
| RF-PRJ-07 | Custom project icon | Later | ✅ `.phpide/icon.png` (or `.idea/icon.png`); else initials |
| RF-PRJ-08 | Second project: new window, this window or attach; default New/Current/Ask | MVP | ✅ New window, this window, **attach** (extra root in the Project view and Go to File; Detach in its context menu) and Ask with "remember". Attached roots get no language server of their own yet |
| RF-PRJ-09 | Attached projects resolved after the main one | Later | ⬜ After v1.0 |
| RF-PRJ-10 | Switch between open projects (Ctrl+Alt+] / [, Window menu) | MVP | ✅ Window › project list, Next/Previous Project Window (Ctrl+Alt+] / [); a window registry under `$XDG_RUNTIME_DIR` lists and focuses the other processes |
| RF-PRJ-11 | Reopen the projects open at exit (can be turned off) | MVP | ✅ Every window open at exit is reopened (one process each); Settings › Appearance & Behavior › System turns it off |
| RF-PRJ-12 | Close project / all / others; closing the last goes Home | MVP | ✅ Close Project, Close Other Projects; closing the last one goes Home |
| RF-PRJ-13 | Merge project windows into tabs | Later | ⬜ Not planned (macOS-only in PhpStorm) |
| RF-PRJ-14 | Default projects folder | Later | ✅ Settings › System Settings › Default project directory |
| RF-PRJ-15 | Move the project folder | Optional | ⬜ After v1.0 |
| RF-PRJ-16 | Open a project from the command line | Optional | ✅ `phpide-ui <folder> [files…]` |

## 2. Project view

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-NAV-01 | Project view: folder tree with dependencies as nodes | MVP | ✅ `vendor/` and `node_modules/` at a root are dependency nodes (package icon, "Composer packages" / "npm packages") |
| RF-NAV-02 | Scope views (open files, custom scopes) | Later | ✅ Project, Open Files, Changed Files, Production, Tests (custom scopes later) |
| RF-NAV-03 | Scratches and consoles node | Optional | ⬜ After v1.0 |
| RF-NAV-04 | Open with single click (option) or double click; same for folders | MVP | ✅ Options menu › Open Files with Single Click |
| RF-NAV-05 | Quick preview popup on Space | Later | ✅ |
| RF-NAV-06 | Always select the opened file; "Select Opened File" button when off | MVP | ✅ |
| RF-NAV-07 | Expand all / collapse all | MVP | ✅ Header buttons, context menu and Ctrl++ / Ctrl+− while the Project view has focus (zoom elsewhere) |
| RF-NAV-08 | Speed search by typing (or Ctrl+F) in the tree | MVP | ✅ Matches highlighted; ↑/↓ jump between matches |
| RF-NAV-09 | Class members as tree nodes | Optional | ⬜ After v1.0 (Structure covers it) |
| RF-NAV-10 | Show/hide excluded files and folders | MVP | ✅ `.git`, `.idea`, `.phpide` |
| RF-NAV-11 | File size and date in the tree | Later | ✅ |
| RF-NAV-12 | File nesting | Later | ✅ With editable rules |
| RF-NAV-13 | Indent guides in the tree | Later | ✅ |
| RF-NAV-14 | Sort by name, type, modified; folders on top | MVP | ✅ |
| RF-NAV-15 | New (file, folder, class) with + or Alt+Insert, filtered by typing | MVP | ✅ + / Alt+Insert opens a popup filtered by typing: file, folder, PHP class/interface/trait/enum (PSR-4 namespace), common file types |
| RF-NAV-16 | Cut, copy, paste, rename (Shift+F6), delete with confirmation | MVP | ✅ Delete goes to the desktop Trash when there is one; open tabs follow renames |
| RF-NAV-17 | Copy path/reference: absolute, name, relative, GitHub URL, with line | MVP | ✅ Absolute, name, project-relative, reference with line (Ctrl+Alt+Shift+C) and repository URL with line (GitHub/GitLab, from the `origin` remote) |
| RF-NAV-18 | Open in file manager / terminal | Later | ✅ |
| RF-NAV-19 | Mark directory as source/test/excluded | Later | ✅ Kept in `.phpide/project.json`; composer roots by default |
| RF-NAV-20 | Open in right split (Shift+Enter); jump to source (F4) | Later | ✅ |
| RF-NAV-21 | Find/replace in folder; find usages | Later | ✅ Replace in Files with folder scope and file mask; Find Usages |
| RF-NAV-22 | Override a file's type and revert | Later | ✅ Context menu and status bar |
| RF-NAV-23 | Compare, local history, sync, bookmarks | Optional | ⬜ After v1.0 |
| RF-NAV-24 | Multiple selection for batch actions | MVP | ✅ Ctrl/Shift+click, Shift+arrows |
| RF-NAV-25 | File names colored by VCS status | Later | ✅ Modified, added, deleted, unversioned, conflicted, ignored |
| RF-NAV-26 | Folders with modified files highlighted | Later | ✅ |
| RF-NAV-27 | Background colors by scope | Optional | ⬜ After v1.0 |

## 3. Finding files

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-SEL-01 | Search Everywhere (Shift Shift): files, symbols, actions, settings; recent files first | MVP | ✅ Files, classes, symbols, actions and Settings pages; recent files first when the query is empty |
| RF-SEL-02 | Scope tabs inside Search Everywhere | Later | ✅ All, Classes, Files, Symbols, Actions (Tab / Shift+Tab) |
| RF-SEL-03 | Go to File (Ctrl+Shift+N) with CamelCase/snake_case matching | MVP | ✅ |
| RF-SEL-04 | Go to Class (Ctrl+N), Symbol, Action | Later | ✅ Go to Class (Ctrl+N), Symbol (Ctrl+Alt+Shift+N), Action (Ctrl+Shift+A). New File is the Project view's Alt+Insert popup |
| RF-SEL-05 | Filter results by file type; include non-project items | Later | ✅ File-type filter; vendor/, node_modules/ and excluded folders on request |
| RF-SEL-06 | Pin results in a panel | Optional | ⬜ After v1.0 |
| RF-SEL-07 | Recent Files (Ctrl+E), filter by typing; Ctrl+E again = edited only | MVP | ✅ (the list was empty before this change) |
| RF-SEL-08 | Recent Locations (Ctrl+Shift+E) | Later | ✅ Changed only with Ctrl+Shift+E again |
| RF-SEL-09 | Back/forward (Ctrl+Alt+←/→); last edit location (Ctrl+Shift+Backspace) | MVP | ✅ |
| RF-SEL-10 | Select In (Alt+F1): Project view, file manager | MVP | ✅ Alt+F1 popup: Project View, Files (file manager), Terminal |
| RF-SEL-11 | Navigation bar (Alt+Home), top/status bar/hidden | Later | ✅ Path at the bottom or top; each segment opens a menu of its folder |

## 4. Editor tabs

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-TAB-01 | New tab next to the active one; option: at the end | MVP | ✅ Settings › Editor Tabs |
| RF-TAB-02 | Preview tab (italic), replaced by the next preview; edit or double click keeps it | MVP | ✅ |
| RF-TAB-03 | Tab limit (10): close the unmodified tab opened longest ago | MVP | ✅ "Least used" variant not offered |
| RF-TAB-04 | Which tab activates after closing | Later | ✅ |
| RF-TAB-05 | Tab placement, including none | Later | 🟡 Top or none; bottom, left and right later |
| RF-TAB-06 | One row (scrolls) or several; list of hidden tabs | MVP | ✅ One scrolling row with the list of every tab, or several rows (Settings › Editor Tabs) |
| RF-TAB-07 | File type icon (and extension) on the tab | MVP | ✅ |
| RF-TAB-08 | Parent folder when two open files share a name | MVP | ✅ |
| RF-TAB-09 | `*` on modified tabs | MVP | ✅ |
| RF-TAB-10 | Full path in the tooltip | MVP | ✅ |
| RF-TAB-11 | Close button position; alphabetical order | Later | ✅ |
| RF-TAB-12 | Tab color by VCS status and analysis | Later | 🟡 VCS status; analysis colors after v1.0 |
| RF-TAB-13 | Tab font size / small font | Optional | ⬜ After v1.0 (follows the UI zoom) |
| RF-TAB-14 | Close: Ctrl+F4, middle click, others (keeps pinned), all, Alt+click ✕ | MVP | ✅ Plus "Close Unpinned Tabs" |
| RF-TAB-15 | Reopen closed tab | MVP | ✅ Ctrl+Shift+T |
| RF-TAB-16 | Pin: survives the limit and Close Others; pinned left / own row | MVP | ✅ Pinning survives the limit and Close Others; pinned tabs stay first (option) |
| RF-TAB-17 | Copy path/name from the tab menu | MVP | ✅ |
| RF-TAB-18 | Detach a tab into its own window | Optional | ⬜ After v1.0 |
| RF-TAB-19 | Alt+←/→ between tabs; Switcher (Ctrl+Tab) | MVP | ✅ |
| RF-TAB-20 | Split right/down from the tab menu or by dragging | Later | 🟡 Tab menu; no dragging |
| RF-TAB-21 | Move/open in the other split; open in right split from the tree | Later | 🟡 From the tree (Shift+Enter) |
| RF-TAB-22 | Unsplit, switch, stretch, equalize splits | Later | 🟡 Unsplit; one split at a time |
| RF-TAB-23 | Maximize the editor (Ctrl+Shift+F12, double click on a tab) | MVP | ✅ |

## 5. File types and highlighting

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-HL-01 | File type by name pattern, bound to a language service | MVP | ✅ `phpide_php::filetype`: 19 types, patterns like `*.blade.php`, `Dockerfile.*`, `.env.*`, `*.xml.dist` |
| RF-HL-02 | Unknown file: offer a type, detect by content (shebang) | MVP | ✅ Shebang and `<?php` first lines; an editor notice offers a type for the extension |
| RF-HL-03 | List of types and patterns (add, edit, remove) | Later | 🟡 Settings › Editor › File Types lists them; user patterns are added from the notice or the status bar and removed there |
| RF-HL-04 | Override the type of specific files and revert | Later | ✅ |
| RF-HL-05 | Custom types: keyword lists, comments, numbers, braces | Optional | 🟡 The engine exists (`SimpleLanguage`, used by .env, INI, TOML, Dockerfile); no UI to define one |
| RF-HL-06 | TextMate grammars | Optional | 🟡 Bundled syntect grammars (Python, Ruby, Go, …); user bundles not loaded |
| RF-HL-07 | Ignored files and folders by pattern | Later | 🟡 Fixed list (`.git`, `.idea`, `.phpide`) |
| RF-HL-08 | Icon per file type in the tree and tabs | MVP | ✅ |
| RF-HL-09 | Built-in schemes tied to the UI theme | MVP | ✅ Dark, Light, High Contrast |
| RF-HL-10 | Language defaults inherited by each language | MVP | ✅ Language Defaults scope inherited by each language with highlighting; a language overrides an element or inherits it |
| RF-HL-11 | Per element: foreground, background, font style, effects | MVP | ✅ Settings › Editor › Color Scheme: foreground, background, bold/italic, underline/strikethrough with color, live preview (click a word to pick its element) |
| RF-HL-12 | Duplicate, rename, export, import schemes | Later | ✅ Also delete; JSON export format |
| RF-HL-13 | Semantic highlighting (one color per variable) | Optional | ⬜ After v1.0 |
| RF-HL-14 | Quick switch scheme (Ctrl+`) | Later | ✅ |
| RF-HL-15 | Open the color setting of the symbol at the caret | Optional | ⬜ After v1.0 |
| RF-HL-16 | Scrollbar marks by severity, clickable; problems widget | Later | ✅ Editor scrollbar marks; Inspections widget in the status bar |
| RF-HL-17 | Lens mode: preview on scrollbar hover | Optional | ✅ |
| RF-HL-18 | VCS change marks in the gutter | Later | ✅ |

## 6. Layout and editor

| ID | Requirement | Priority | Status |
|---|---|---|---|
| RF-UI-01 | Navigation bar, Project view, editor, status bar, tool windows | MVP | ✅ |
| RF-UI-02 | Tool windows: Alt+1…9, Shift+Esc, Ctrl+Shift+F12, Esc to editor, F12 to the last one; move/detach | MVP | ✅ Shortcuts done; Esc returns to the editor; every tool window can move to left/right/bottom or float (Move To in its header) |
| RF-UI-03 | Gutter, clickable breadcrumbs, scrollbar marks | MVP | ✅ Gutter, scrollbar marks and clickable navigation bar |
| RF-UI-04 | Sticky lines (up to 5, clickable) | Later | ✅ |
| RF-UI-05 | Editor font family, size, line height, ligatures; zoom | MVP | ✅ Family (bundled JetBrains Mono or any monospaced system font), size, line height, zoom. Ligatures: not supported by the text engine |
| RF-UI-06 | Configurable keymap; assign a shortcut from Search Everywhere | Later | ✅ Alt+Enter in Find Action; IntelliJ keymap only |
| RF-UI-07 | Bookmarks: anonymous (F11) and mnemonic (Ctrl+F11) | Optional | 🟡 F11 only |

## Shortcut changes made for this list

| Action | Before | Now (PhpStorm) |
|---|---|---|
| Back / Forward | Alt+←/→ | Ctrl+Alt+←/→ |
| Previous / next tab | — | Alt+←/→ |
| Go to definition | F12 or Ctrl+B | Ctrl+B |
| Jump to last tool window | — | F12 |
| Reopen closed tab | — | Ctrl+Shift+T |
| Last edit location | — | Ctrl+Shift+Backspace |
| Select opened file in Project view | — | Alt+F1 |
