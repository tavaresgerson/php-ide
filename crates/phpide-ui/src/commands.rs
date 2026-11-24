//! The single list of IDE commands.
//!
//! Menu, Find Action (palette) and keyboard shortcuts all read from
//! [`REGISTRY`]; behaviour lives in `PhpIdeApp::dispatch_command`. Adding a
//! command means one entry here and one arm there.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use phpide_editor::ViewCommand;

use crate::toolwindows::ToolWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    OpenProject,
    CloseProject,
    NewFile,
    Save,
    CloseTab,
    CloseOthers,
    CloseAllTabs,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    PinTab,
    RefreshTree,
    OpenSettings,
    // Edit
    Undo,
    Redo,
    UndoRefactor,
    Complete,
    CopyPath,
    CopyReference,
    CopyRepositoryUrl,
    RenameSymbol,
    ShowContextActions,
    ReformatCode,
    QuickDocumentation,
    ParameterInfo,
    /// Text commands routed to the focused editor pane.
    Editor(ViewCommand),
    // View
    ToggleToolWindow(ToolWindow),
    HideActiveToolWindow,
    HideAllToolWindows,
    LastToolWindow,
    ToggleTheme,
    OpenScheme,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleBlame,
    ToggleFolding,
    SplitRight,
    SplitDown,
    Unsplit,
    // Navigate
    SearchEverywhere,
    GotoFile,
    GotoClass,
    RecentFiles,
    FindAction,
    GotoSymbol,
    GotoLine,
    GotoDefinition,
    GotoImplementation,
    GotoTypeDeclaration,
    FindUsages,
    NavigateBack,
    NavigateForward,
    LastEditLocation,
    SelectIn,
    FileStructure,
    NextProblem,
    PrevProblem,
    BlockStart,
    BlockEnd,
    ToggleBookmark,
    // Run
    RunCurrent,
    DebugCurrent,
    RunContext,
    DebugContext,
    RunWithCoverage,
    RerunLast,
    EditConfigurations,
    EvaluateExpression,
    ViewBreakpoints,
    Stop,
    ToggleBreakpoint,
    // Window
    NextProjectWindow,
    PreviousProjectWindow,
    CloseOtherProjects,
    // Tools
    RestartLsp,
    LanguageServerLog,
    OpenWelcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    File,
    Edit,
    View,
    Navigate,
    Run,
    Tools,
    Window,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::File,
        Category::Edit,
        Category::View,
        Category::Navigate,
        Category::Run,
        Category::Tools,
        Category::Window,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::View => "View",
            Category::Navigate => "Navigate",
            Category::Run => "Run",
            Category::Tools => "Tools",
            Category::Window => "Window",
        }
    }
}

pub struct CommandSpec {
    pub id: Command,
    pub category: Category,
    pub label: &'static str,
    /// The first shortcut is the one shown in menus.
    pub shortcuts: &'static [KeyboardShortcut],
    /// Listed in the main menu (everything is listed in Find Action).
    pub in_menu: bool,
}

impl CommandSpec {
    /// Label used by Find Action: `Category: Label`.
    pub fn palette_label(&self) -> String {
        format!("{}: {}", self.category.label(), self.label)
    }
}

const fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
    Modifiers {
        alt,
        ctrl,
        shift,
        mac_cmd: false,
        command: false,
    }
}

const fn key(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(false, false, false), key)
}
const fn ctrl(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(true, false, false), key)
}
const fn ctrl_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(true, true, false), key)
}
const fn ctrl_alt(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(true, false, true), key)
}
const fn ctrl_alt_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(true, true, true), key)
}
const fn alt(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(false, false, true), key)
}
const fn shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(false, true, false), key)
}
const fn alt_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods(false, true, true), key)
}

const fn spec(
    id: Command,
    category: Category,
    label: &'static str,
    shortcuts: &'static [KeyboardShortcut],
    in_menu: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        category,
        label,
        shortcuts,
        in_menu,
    }
}

use Category as C;
use Command as K;
use ToolWindow as Tw;
use ViewCommand as V;

pub const REGISTRY: &[CommandSpec] = &[
    // File
    spec(K::OpenProject, C::File, "Open Project…", &[], true),
    spec(K::CloseProject, C::File, "Close Project", &[], true),
    spec(K::NewFile, C::File, "New File", &[], true),
    spec(K::Save, C::File, "Save", &[ctrl(Key::S)], true),
    spec(K::CloseTab, C::File, "Close Tab", &[ctrl(Key::F4)], true),
    spec(K::CloseOthers, C::File, "Close Other Tabs", &[], true),
    spec(K::CloseAllTabs, C::File, "Close All Tabs", &[], true),
    spec(
        K::ReopenClosedTab,
        C::File,
        "Reopen Closed Tab",
        &[ctrl_shift(Key::T)],
        true,
    ),
    spec(
        K::NextTab,
        C::Navigate,
        "Select Next Tab",
        &[alt(Key::ArrowRight)],
        false,
    ),
    spec(
        K::PreviousTab,
        C::Navigate,
        "Select Previous Tab",
        &[alt(Key::ArrowLeft)],
        false,
    ),
    spec(K::PinTab, C::File, "Pin/Unpin Tab", &[], false),
    spec(K::RefreshTree, C::File, "Reload Project Tree", &[], false),
    spec(
        K::OpenSettings,
        C::File,
        "Settings",
        &[ctrl_alt(Key::S)],
        true,
    ),
    // Edit
    spec(K::Undo, C::Edit, "Undo", &[ctrl(Key::Z)], true),
    spec(K::Redo, C::Edit, "Redo", &[ctrl_shift(Key::Z)], true),
    spec(K::UndoRefactor, C::Edit, "Undo Last Refactoring", &[], true),
    spec(
        K::Complete,
        C::Edit,
        "Code Completion",
        &[ctrl(Key::Space)],
        true,
    ),
    spec(
        K::CopyPath,
        C::Edit,
        "Copy Absolute Path",
        &[ctrl_shift(Key::C)],
        true,
    ),
    spec(
        K::CopyReference,
        C::Edit,
        "Copy Reference (path:line)",
        &[ctrl_alt_shift(Key::C)],
        true,
    ),
    spec(
        K::CopyRepositoryUrl,
        C::Edit,
        "Copy Repository URL",
        &[],
        true,
    ),
    spec(K::Editor(V::SelectAll), C::Edit, "Select All", &[], true),
    spec(K::Editor(V::Find), C::Edit, "Find", &[ctrl(Key::F)], true),
    spec(
        K::Editor(V::Replace),
        C::Edit,
        "Replace",
        &[ctrl(Key::R)],
        true,
    ),
    spec(
        K::Editor(V::FindNext),
        C::Edit,
        "Find Next",
        &[key(Key::F3)],
        true,
    ),
    spec(
        K::Editor(V::FindPrevious),
        C::Edit,
        "Find Previous",
        &[shift(Key::F3)],
        true,
    ),
    spec(
        K::Editor(V::ToggleLineComment),
        C::Edit,
        "Comment with Line Comment",
        &[ctrl(Key::Slash)],
        true,
    ),
    spec(
        K::Editor(V::ToggleBlockComment),
        C::Edit,
        "Comment with Block Comment",
        &[ctrl_shift(Key::Slash), ctrl_shift(Key::Questionmark)],
        true,
    ),
    spec(
        K::Editor(V::Duplicate),
        C::Edit,
        "Duplicate Line or Selection",
        &[ctrl(Key::D)],
        true,
    ),
    spec(
        K::Editor(V::DeleteLines),
        C::Edit,
        "Delete Line",
        &[ctrl(Key::Y)],
        true,
    ),
    spec(
        K::Editor(V::MoveLinesUp),
        C::Edit,
        "Move Line Up",
        &[alt_shift(Key::ArrowUp)],
        true,
    ),
    spec(
        K::Editor(V::MoveLinesDown),
        C::Edit,
        "Move Line Down",
        &[alt_shift(Key::ArrowDown)],
        true,
    ),
    spec(K::Editor(V::Indent), C::Edit, "Indent Lines", &[], false),
    spec(K::Editor(V::Outdent), C::Edit, "Unindent Lines", &[], false),
    spec(
        K::Editor(V::SelectNextOccurrence),
        C::Edit,
        "Add Selection for Next Occurrence",
        &[alt(Key::J)],
        true,
    ),
    spec(
        K::Editor(V::UnselectOccurrence),
        C::Edit,
        "Unselect Occurrence",
        &[alt_shift(Key::J)],
        true,
    ),
    spec(
        K::Editor(V::SelectAllOccurrences),
        C::Edit,
        "Select All Occurrences",
        &[ctrl_alt_shift(Key::J)],
        true,
    ),
    spec(
        K::Editor(V::CaretAbove),
        C::Edit,
        "Add Caret Above",
        &[ctrl_alt_shift(Key::ArrowUp)],
        false,
    ),
    spec(
        K::Editor(V::CaretBelow),
        C::Edit,
        "Add Caret Below",
        &[ctrl_alt_shift(Key::ArrowDown)],
        false,
    ),
    spec(
        K::Editor(V::Fold),
        C::Edit,
        "Collapse",
        &[ctrl_shift(Key::Minus)],
        false,
    ),
    spec(
        K::Editor(V::Unfold),
        C::Edit,
        "Expand",
        &[ctrl_shift(Key::Equals), ctrl_shift(Key::Plus)],
        false,
    ),
    spec(K::Editor(V::FoldAll), C::Edit, "Collapse All", &[], false),
    spec(K::Editor(V::UnfoldAll), C::Edit, "Expand All", &[], false),
    // View
    spec(
        K::ToggleToolWindow(Tw::Project),
        C::View,
        "Project",
        &[alt(Key::Num1)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Bookmarks),
        C::View,
        "Bookmarks",
        &[alt(Key::Num2)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Find),
        C::View,
        "Find",
        &[alt(Key::Num3), ctrl_shift(Key::F)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Run),
        C::View,
        "Run",
        &[alt(Key::Num4)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Debug),
        C::View,
        "Debug",
        &[alt(Key::Num5)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Problems),
        C::View,
        "Problems",
        &[alt(Key::Num6), ctrl_shift(Key::M)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Structure),
        C::View,
        "Structure",
        &[alt(Key::Num7)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Services),
        C::View,
        "Services",
        &[alt(Key::Num8)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::VersionControl),
        C::View,
        "Version Control",
        &[alt(Key::Num9)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Commit),
        C::View,
        "Commit",
        &[alt(Key::Num0)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Terminal),
        C::View,
        "Terminal",
        &[alt(Key::F12)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Database),
        C::View,
        "Database",
        &[ctrl_alt(Key::Num3)],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::Notifications),
        C::View,
        "Notifications",
        &[],
        true,
    ),
    spec(
        K::ToggleToolWindow(Tw::RemoteHost),
        C::View,
        "Remote Host",
        &[],
        true,
    ),
    spec(
        K::HideActiveToolWindow,
        C::View,
        "Hide Active Tool Window",
        &[shift(Key::Escape)],
        false,
    ),
    spec(
        K::HideAllToolWindows,
        C::View,
        "Hide All Tool Windows",
        &[ctrl_shift(Key::F12)],
        true,
    ),
    spec(
        K::LastToolWindow,
        C::View,
        "Jump to Last Tool Window",
        &[key(Key::F12)],
        false,
    ),
    spec(
        K::ToggleTheme,
        C::View,
        "Toggle Light/Dark Theme",
        &[],
        false,
    ),
    spec(
        K::OpenScheme,
        C::View,
        "Quick Switch Scheme",
        &[ctrl(Key::Backtick)],
        true,
    ),
    spec(
        K::ZoomIn,
        C::View,
        "Zoom In",
        &[ctrl(Key::Equals), alt_shift(Key::Period)],
        false,
    ),
    spec(
        K::ZoomOut,
        C::View,
        "Zoom Out",
        &[ctrl(Key::Minus), alt_shift(Key::Comma)],
        false,
    ),
    spec(
        K::ZoomReset,
        C::View,
        "Reset Zoom",
        &[ctrl(Key::Num0)],
        false,
    ),
    spec(K::ToggleBlame, C::View, "Toggle Blame Gutter", &[], false),
    spec(K::ToggleFolding, C::View, "Toggle Folding", &[], false),
    spec(K::SplitRight, C::View, "Split Right", &[], false),
    spec(K::SplitDown, C::View, "Split Down", &[], false),
    spec(K::Unsplit, C::View, "Unsplit", &[], false),
    // Navigate
    spec(
        K::SearchEverywhere,
        C::Navigate,
        "Search Everywhere",
        &[],
        true,
    ),
    spec(
        K::GotoFile,
        C::Navigate,
        "Go to File",
        &[ctrl_shift(Key::N)],
        true,
    ),
    spec(
        K::GotoClass,
        C::Navigate,
        "Go to Class",
        &[ctrl(Key::N)],
        true,
    ),
    spec(
        K::RecentFiles,
        C::Navigate,
        "Recent Files",
        &[ctrl(Key::E)],
        true,
    ),
    spec(
        K::FindAction,
        C::Navigate,
        "Find Action",
        &[ctrl_shift(Key::A)],
        true,
    ),
    spec(
        K::GotoSymbol,
        C::Navigate,
        "Go to Symbol",
        &[ctrl_alt_shift(Key::N)],
        true,
    ),
    spec(
        K::GotoLine,
        C::Navigate,
        "Go to Line",
        &[ctrl(Key::G)],
        true,
    ),
    spec(
        K::GotoDefinition,
        C::Navigate,
        "Go to Definition",
        &[ctrl(Key::B)],
        true,
    ),
    spec(
        K::GotoImplementation,
        C::Navigate,
        "Go to Implementation",
        &[ctrl_alt(Key::B)],
        true,
    ),
    spec(
        K::GotoTypeDeclaration,
        C::Navigate,
        "Go to Type Declaration",
        &[ctrl_shift(Key::B)],
        true,
    ),
    spec(
        K::FindUsages,
        C::Navigate,
        "Find Usages",
        &[alt(Key::F7)],
        true,
    ),
    spec(
        K::NavigateBack,
        C::Navigate,
        "Back",
        &[ctrl_alt(Key::ArrowLeft)],
        true,
    ),
    spec(
        K::NavigateForward,
        C::Navigate,
        "Forward",
        &[ctrl_alt(Key::ArrowRight)],
        true,
    ),
    spec(
        K::LastEditLocation,
        C::Navigate,
        "Last Edit Location",
        &[ctrl_shift(Key::Backspace)],
        true,
    ),
    spec(
        K::SelectIn,
        C::Navigate,
        "Select Opened File in Project View",
        &[alt(Key::F1)],
        true,
    ),
    spec(
        K::NextProblem,
        C::Navigate,
        "Next Problem",
        &[key(Key::F2)],
        true,
    ),
    spec(
        K::PrevProblem,
        C::Navigate,
        "Previous Problem",
        &[shift(Key::F2)],
        true,
    ),
    spec(
        K::BlockStart,
        C::Navigate,
        "Go to Block Start",
        &[ctrl(Key::OpenBracket)],
        false,
    ),
    spec(
        K::BlockEnd,
        C::Navigate,
        "Go to Block End",
        &[ctrl(Key::CloseBracket)],
        false,
    ),
    spec(
        K::ToggleBookmark,
        C::Navigate,
        "Toggle Bookmark",
        &[key(Key::F11)],
        true,
    ),
    // Run
    spec(K::RunCurrent, C::Run, "Run", &[shift(Key::F10)], true),
    spec(K::DebugCurrent, C::Run, "Debug", &[shift(Key::F9)], true),
    spec(
        K::RunContext,
        C::Run,
        "Run Context Configuration",
        &[ctrl_shift(Key::F10)],
        true,
    ),
    spec(
        K::DebugContext,
        C::Run,
        "Debug Context Configuration",
        &[ctrl_shift(Key::F9)],
        true,
    ),
    spec(
        K::RunWithCoverage,
        C::Run,
        "Run Tests with Coverage",
        &[],
        true,
    ),
    spec(K::RerunLast, C::Run, "Rerun", &[ctrl(Key::F5)], true),
    spec(
        K::EvaluateExpression,
        C::Run,
        "Evaluate Expression…",
        &[alt(Key::F8)],
        true,
    ),
    spec(
        K::ViewBreakpoints,
        C::Run,
        "View Breakpoints…",
        &[ctrl_shift(Key::F8)],
        true,
    ),
    spec(
        K::EditConfigurations,
        C::Run,
        "Edit Configurations…",
        &[],
        true,
    ),
    spec(K::Stop, C::Run, "Stop", &[ctrl(Key::F2)], true),
    spec(
        K::ToggleBreakpoint,
        C::Run,
        "Toggle Breakpoint",
        &[key(Key::F9)],
        true,
    ),
    // Tools
    spec(
        K::RestartLsp,
        C::Tools,
        "Restart Language Server",
        &[],
        true,
    ),
    spec(
        K::LanguageServerLog,
        C::Tools,
        "Language Server Log",
        &[],
        true,
    ),
    // Code (Edit menu)
    spec(
        K::RenameSymbol,
        C::Edit,
        "Rename Symbol…",
        &[shift(Key::F6)],
        true,
    ),
    spec(
        K::ShowContextActions,
        C::Edit,
        "Show Context Actions",
        &[alt(Key::Enter)],
        true,
    ),
    spec(
        K::ReformatCode,
        C::Edit,
        "Reformat Code",
        &[ctrl_alt(Key::L)],
        true,
    ),
    spec(
        K::QuickDocumentation,
        C::View,
        "Quick Documentation",
        &[ctrl(Key::Q)],
        true,
    ),
    spec(
        K::ParameterInfo,
        C::View,
        "Parameter Info",
        &[ctrl(Key::P)],
        true,
    ),
    spec(
        K::FileStructure,
        C::Navigate,
        "File Structure",
        &[ctrl(Key::F12)],
        true,
    ),
    spec(K::OpenWelcome, C::Tools, "Welcome Screen", &[], true),
    // Window
    spec(
        K::NextProjectWindow,
        C::Window,
        "Next Project Window",
        &[ctrl_alt(Key::CloseBracket)],
        true,
    ),
    spec(
        K::PreviousProjectWindow,
        C::Window,
        "Previous Project Window",
        &[ctrl_alt(Key::OpenBracket)],
        true,
    ),
    spec(
        K::CloseOtherProjects,
        C::Window,
        "Close Other Projects",
        &[],
        true,
    ),
];

impl Command {
    /// Commands that act on the focused text (editor edits, undo, find).
    pub fn is_text_command(self) -> bool {
        matches!(
            self,
            Command::Editor(_)
                | Command::Undo
                | Command::Redo
                | Command::CopyPath
                | Command::CopyReference
                | Command::RenameSymbol
                | Command::ShowContextActions
                | Command::ReformatCode
                | Command::QuickDocumentation
                | Command::ParameterInfo
        )
    }
}

/// The spec of `id`. Every command has one (checked by a test).
pub fn spec_of(id: Command) -> &'static CommandSpec {
    REGISTRY
        .iter()
        .find(|spec| spec.id == id)
        .expect("command missing from REGISTRY")
}

/// Whether the command belongs to the build (frozen tool windows are only
/// listed with the `experimental` feature).
pub fn is_available(id: Command) -> bool {
    match id {
        Command::ToggleToolWindow(tw) => tw.is_available(),
        _ => true,
    }
}

/// Text of the primary shortcut, e.g. `Ctrl+Shift+Z`; empty when none.
pub fn shortcut_text(id: Command) -> String {
    spec_of(id)
        .shortcuts
        .first()
        .map(|sc| sc.format(&eframe::egui::ModifierNames::NAMES, false))
        .unwrap_or_default()
}

fn modifier_count(shortcut: &KeyboardShortcut) -> usize {
    let m = shortcut.modifiers;
    usize::from(m.ctrl) + usize::from(m.shift) + usize::from(m.alt)
}

/// Every `(shortcut, command)` pair, most specific first.
///
/// egui matches modifiers "logically": extra Shift and Alt are ignored, so
/// Ctrl+Z also matches Ctrl+Shift+Z. Consuming the more specific shortcut
/// first is what keeps Redo from running Undo.
pub fn shortcuts_by_specificity() -> Vec<(KeyboardShortcut, Command)> {
    let mut pairs: Vec<(KeyboardShortcut, Command)> = REGISTRY
        .iter()
        .filter(|spec| is_available(spec.id))
        .flat_map(|spec| spec.shortcuts.iter().map(move |sc| (*sc, spec.id)))
        .collect();
    pairs.sort_by_key(|(sc, _)| std::cmp::Reverse(modifier_count(sc)));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn every_command_is_registered_once() {
        let mut seen = HashMap::new();
        for spec in REGISTRY {
            assert!(
                seen.insert(spec.id, spec.label).is_none(),
                "{:?} registered twice",
                spec.id
            );
        }
    }

    #[test]
    fn no_shortcut_is_bound_twice() {
        let mut seen: HashMap<(bool, bool, bool, Key), Command> = HashMap::new();
        for spec in REGISTRY {
            for sc in spec.shortcuts {
                let m = sc.modifiers;
                let k = (m.ctrl, m.shift, m.alt, sc.logical_key);
                if let Some(other) = seen.insert(k, spec.id) {
                    panic!("{:?} and {:?} share a shortcut", other, spec.id);
                }
            }
        }
    }

    /// Simulates egui's matching: the first pair whose modifiers match
    /// "logically" (extra Shift/Alt ignored) wins.
    fn resolve(pressed: KeyboardShortcut) -> Option<Command> {
        shortcuts_by_specificity()
            .into_iter()
            .find(|(sc, _)| {
                sc.logical_key == pressed.logical_key
                    && pressed.modifiers.matches_logically(sc.modifiers)
            })
            .map(|(_, cmd)| cmd)
    }

    #[test]
    fn specific_shortcuts_win_over_their_subsets() {
        assert_eq!(resolve(ctrl_shift(Key::Z)), Some(Command::Redo));
        assert_eq!(resolve(ctrl(Key::Z)), Some(Command::Undo));
        assert_eq!(resolve(ctrl_shift(Key::N)), Some(Command::GotoFile));
        assert_eq!(resolve(ctrl_alt_shift(Key::N)), Some(Command::GotoSymbol));
        assert_eq!(resolve(ctrl(Key::N)), Some(Command::GotoClass));
        assert_eq!(resolve(ctrl_alt(Key::S)), Some(Command::OpenSettings));
        assert_eq!(resolve(ctrl(Key::S)), Some(Command::Save));
        assert_eq!(resolve(ctrl_alt(Key::B)), Some(Command::GotoImplementation));
        assert_eq!(
            resolve(ctrl_shift(Key::B)),
            Some(Command::GotoTypeDeclaration)
        );
        assert_eq!(resolve(shift(Key::F2)), Some(Command::PrevProblem));
        assert_eq!(resolve(key(Key::F2)), Some(Command::NextProblem));
        assert_eq!(
            resolve(ctrl_shift(Key::F12)),
            Some(Command::HideAllToolWindows)
        );
        assert_eq!(
            resolve(ctrl_shift(Key::Minus)),
            Some(Command::Editor(ViewCommand::Fold))
        );
        assert_eq!(resolve(ctrl(Key::Minus)), Some(Command::ZoomOut));
        assert_eq!(
            resolve(alt_shift(Key::ArrowUp)),
            Some(Command::Editor(ViewCommand::MoveLinesUp))
        );
        assert_eq!(resolve(alt(Key::ArrowLeft)), Some(Command::PreviousTab));
        assert_eq!(
            resolve(ctrl_alt(Key::ArrowLeft)),
            Some(Command::NavigateBack)
        );
        assert_eq!(resolve(key(Key::F12)), Some(Command::LastToolWindow));
        // Phase 5: IntelliJ code shortcuts.
        assert_eq!(resolve(ctrl(Key::P)), Some(Command::ParameterInfo));
        assert_eq!(resolve(ctrl(Key::Q)), Some(Command::QuickDocumentation));
        assert_eq!(resolve(ctrl(Key::F12)), Some(Command::FileStructure));
        assert_eq!(resolve(shift(Key::F6)), Some(Command::RenameSymbol));
        assert_eq!(resolve(alt(Key::Enter)), Some(Command::ShowContextActions));
        assert_eq!(resolve(ctrl_alt(Key::L)), Some(Command::ReformatCode));
        assert_eq!(
            resolve(ctrl_alt_shift(Key::J)),
            Some(Command::Editor(ViewCommand::SelectAllOccurrences))
        );
        assert_eq!(
            resolve(alt(Key::J)),
            Some(Command::Editor(ViewCommand::SelectNextOccurrence))
        );
    }
}
