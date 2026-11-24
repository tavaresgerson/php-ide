//! Data the application lays over the text: gutter marks, diagnostics,
//! inlay hints, semantic tokens, occurrences. Lines are 0-based, offsets
//! are chars.

use phpide_php::SyntaxRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakpointMark {
    Enabled,
    /// Confirmed by the debugger.
    Verified,
    Disabled,
    /// Rejected by the debugger.
    Invalid,
    /// "Mute breakpoints" is on.
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitMark {
    Added,
    Modified,
    /// Lines were deleted after this line.
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageMark {
    Full,
    Partial,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Hint,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub start: usize,
    pub end: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlayHint {
    /// Shown before this char.
    pub position: usize,
    pub label: String,
}

/// One line of the blame gutter.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameLine {
    pub text: String,
    pub tooltip: String,
}

#[derive(Default)]
pub struct Decorations<'a> {
    pub breakpoints: Vec<(usize, BreakpointMark)>,
    pub bookmarks: Vec<usize>,
    pub git: Vec<(usize, GitMark)>,
    pub coverage: Vec<(usize, CoverageMark)>,
    pub diagnostics: Vec<Diagnostic>,
    pub inlay_hints: Vec<InlayHint>,
    /// Semantic tokens `(start, end, role)`, sorted, painted over syntax.
    pub semantic: Vec<(usize, usize, SyntaxRole)>,
    /// Occurrences of the symbol under the caret.
    pub occurrences: Vec<(usize, usize)>,
    /// Line where the debugger is paused.
    pub execution_line: Option<usize>,
    /// Blame text per line (the gutter column shows when set).
    pub blame: Option<&'a dyn Fn(usize) -> Option<BlameLine>>,
    /// Marker of stale annotations (blame on an edited file).
    pub blame_stale: bool,
}
