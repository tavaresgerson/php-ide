//! Editor state of the window: one view per open buffer in the main pane,
//! an optional split pane (a second view, possibly of the same buffer),
//! and the syntax state shared by all views of a buffer.

use std::collections::HashMap;

use eframe::egui;
use phpide_core::{Buffer, Workspace};
use phpide_editor::{EditorView, Syntax, ViewCommand};
use phpide_php::filetype::{self, Detected};

/// `(pattern or path, file type id)` pairs from the settings.
type Pairs = Vec<(String, String)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitAxis {
    /// Side by side.
    Right,
    /// One above the other.
    Down,
}

pub(crate) struct SplitPane {
    pub axis: SplitAxis,
    /// Share of the first pane.
    pub fraction: f32,
    pub buffer: u64,
    pub view: EditorView,
}

pub(crate) struct EditorArea {
    syntaxes: HashMap<u64, Syntax>,
    views: HashMap<u64, EditorView>,
    pub split: Option<SplitPane>,
    /// Pane that receives editor commands (the last one focused).
    pub focus: Pane,
    /// Screen rect of the focused caret (completion popup anchor).
    pub caret_rect: Option<egui::Rect>,
    /// An editor pane had keyboard focus last frame (editor shortcuts only
    /// apply then, or when nothing else is focused).
    pub editor_focused: bool,
    /// Give keyboard focus to the focused pane on its next frame (file
    /// opened, popup closed).
    focus_request: bool,
    /// Buffer shown by the primary pane last frame.
    last_active: Option<u64>,
    /// User patterns and per-file overrides (from settings).
    file_types: (Pairs, Pairs),
}

impl Default for EditorArea {
    fn default() -> Self {
        Syntax::warm_up();
        EditorArea {
            syntaxes: HashMap::new(),
            views: HashMap::new(),
            split: None,
            focus: Pane::Primary,
            caret_rect: None,
            editor_focused: false,
            focus_request: false,
            last_active: None,
            file_types: (Vec::new(), Vec::new()),
        }
    }
}

impl EditorArea {
    /// Moves keyboard focus to the focused pane on its next frame.
    pub fn request_focus(&mut self) {
        self.focus_request = true;
    }

    /// Notes the active buffer; switching to another one focuses its editor.
    pub fn note_active(&mut self, ws: &Workspace) {
        let active = ws.active_buffer().map(|b| b.id);
        if active.is_some() && active != self.last_active {
            self.focus = Pane::Primary;
            self.focus_request = true;
        }
        self.last_active = active;
    }

    /// Whether `pane` should take focus now (consumes the request).
    pub fn take_focus_request(&mut self, pane: Pane) -> bool {
        let focused = self.split.is_none() || self.focus == pane;
        if self.focus_request && focused {
            self.focus_request = false;
            return true;
        }
        false
    }

    /// Applies the user's file-type patterns and per-file overrides;
    /// buffers whose type changes get a new syntax.
    pub fn set_file_types(
        &mut self,
        patterns: &[(String, String)],
        overrides: &[(String, String)],
    ) {
        if self.file_types.0 != patterns || self.file_types.1 != overrides {
            self.file_types = (patterns.to_vec(), overrides.to_vec());
            self.syntaxes.clear();
        }
    }

    /// The file type of `buffer`: per-file override, user patterns, then
    /// the built-in patterns and the first line.
    pub fn detect(&self, buffer: &Buffer) -> Detected {
        let Some(path) = buffer.path.as_deref() else {
            return Detected::Known(filetype::by_id("php").expect("PHP type"));
        };
        let key = path.to_string_lossy();
        if let Some(file_type) = self
            .file_types
            .1
            .iter()
            .find(|(p, _)| *p == key)
            .and_then(|(_, id)| filetype::by_id(id))
        {
            return Detected::Known(file_type);
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rope = buffer.document.rope();
        let first_line: String = if rope.len_lines() > 0 {
            rope.line(0).chars().take(200).collect()
        } else {
            String::new()
        };
        filetype::detect_with(&name, &first_line, &self.file_types.0)
    }

    fn ensure_syntax(&mut self, buffer: &Buffer) {
        if !self.syntaxes.contains_key(&buffer.id) {
            let detected = self.detect(buffer);
            self.syntaxes.insert(buffer.id, Syntax::for_type(&detected));
        }
    }

    pub fn syntax(&mut self, buffer: &Buffer) -> &mut Syntax {
        self.ensure_syntax(buffer);
        self.syntaxes
            .get_mut(&buffer.id)
            .expect("syntax just created")
    }

    /// View and syntax of a pane's buffer, borrowed together.
    pub fn pane_parts(&mut self, pane: Pane, buffer: &Buffer) -> (&mut EditorView, &mut Syntax) {
        self.ensure_syntax(buffer);
        let syntax = self
            .syntaxes
            .get_mut(&buffer.id)
            .expect("syntax just created");
        let view = match (pane, self.split.as_mut()) {
            (Pane::Secondary, Some(split)) => &mut split.view,
            _ => self.views.entry(buffer.id).or_default(),
        };
        (view, syntax)
    }

    /// Index of the buffer shown in `pane`.
    pub fn pane_buffer(&self, pane: Pane, ws: &Workspace) -> Option<usize> {
        match pane {
            Pane::Primary => (ws.active < ws.buffers.len()).then_some(ws.active),
            Pane::Secondary => {
                let id = self.split.as_ref()?.buffer;
                ws.buffers.iter().position(|b| b.id == id)
            }
        }
    }

    /// Drops the state of closed buffers (and the split showing one).
    pub fn retain_open(&mut self, ws: &Workspace) {
        let open: Vec<u64> = ws.buffers.iter().map(|b| b.id).collect();
        self.views.retain(|id, _| open.contains(id));
        self.syntaxes.retain(|id, _| open.contains(id));
        if self
            .split
            .as_ref()
            .is_some_and(|s| !open.contains(&s.buffer))
        {
            self.split = None;
            self.focus = Pane::Primary;
        }
    }

    /// Opens the active buffer in a second pane.
    pub fn split(&mut self, ws: &Workspace, axis: SplitAxis) {
        let Some(buffer) = ws.active_buffer() else {
            return;
        };
        let mut view = EditorView::new();
        if let Some(primary) = self.views.get(&buffer.id) {
            view.selection = primary.selection.clone();
            view.scroll = primary.scroll;
        }
        self.split = Some(SplitPane {
            axis,
            fraction: 0.5,
            buffer: buffer.id,
            view,
        });
        self.focus = Pane::Secondary;
    }

    pub fn unsplit(&mut self) -> bool {
        self.focus = Pane::Primary;
        self.split.take().is_some()
    }

    /// Runs an editor command on the focused pane.
    pub fn run(&mut self, command: ViewCommand, ws: &mut Workspace) -> bool {
        let pane = if self.split.is_some() {
            self.focus
        } else {
            Pane::Primary
        };
        let Some(index) = self.pane_buffer(pane, ws) else {
            return false;
        };
        let buffer = &mut ws.buffers[index];
        let (view, syntax) = self.pane_parts(pane, buffer);
        view.run(command, buffer, syntax)
    }

    /// Drops every view's cached layouts (snapshot renders use a fresh
    /// egui context and font atlas).
    #[cfg(test)]
    pub fn clear_layouts(&mut self) {
        for view in self.views.values_mut() {
            view.clear_layouts();
        }
        if let Some(split) = self.split.as_mut() {
            split.view.clear_layouts();
        }
    }

    /// Selects `anchor..head` in the active buffer's view (snippet stops,
    /// rename).
    pub fn select(&mut self, ws: &mut Workspace, anchor: usize, head: usize) {
        let pane = if self.split.is_some() {
            self.focus
        } else {
            Pane::Primary
        };
        let Some(index) = self.pane_buffer(pane, ws) else {
            return;
        };
        let (view, _) = self.pane_parts(pane, &ws.buffers[index]);
        view.set_caret(&ws.buffers[index].document, head, false);
        view.set_selection(
            &mut ws.buffers[index],
            phpide_editor::selection::Selection::single(anchor, head),
        );
    }

    /// The primary selection of the active view, `(start, end)`.
    pub fn selection(&mut self, ws: &Workspace) -> Option<(usize, usize)> {
        let pane = if self.split.is_some() {
            self.focus
        } else {
            Pane::Primary
        };
        let index = self.pane_buffer(pane, ws)?;
        let (view, _) = self.pane_parts(pane, &ws.buffers[index]);
        let range = view.selection.primary();
        Some((range.start(), range.end()))
    }

    /// Moves the caret of the active buffer's view (completion, jumps).
    pub fn set_caret(&mut self, ws: &mut Workspace, pos: usize) {
        let pane = if self.split.is_some() {
            self.focus
        } else {
            Pane::Primary
        };
        let Some(index) = self.pane_buffer(pane, ws) else {
            return;
        };
        let buffer = &ws.buffers[index];
        let (view, _) = self.pane_parts(pane, buffer);
        view.set_caret(&buffer.document, pos, false);
    }
}
