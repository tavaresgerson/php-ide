//! Editor state of the window: one view per open buffer in the main pane,
//! an optional split pane (a second view, possibly of the same buffer),
//! and the syntax state shared by all views of a buffer.

use std::collections::HashMap;

use eframe::egui;
use phpide_core::{Buffer, Workspace};
use phpide_editor::{EditorView, Syntax, ViewCommand};

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
    /// Give keyboard focus to the focused pane on its next frame (file
    /// opened, popup closed).
    focus_request: bool,
    /// Buffer shown by the primary pane last frame.
    last_active: Option<u64>,
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
            focus_request: false,
            last_active: None,
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

    pub fn syntax(&mut self, buffer: &Buffer) -> &mut Syntax {
        self.syntaxes
            .entry(buffer.id)
            .or_insert_with(|| Syntax::for_path(buffer.path.as_deref()))
    }

    /// View and syntax of a pane's buffer, borrowed together.
    pub fn pane_parts(&mut self, pane: Pane, buffer: &Buffer) -> (&mut EditorView, &mut Syntax) {
        let syntax = self
            .syntaxes
            .entry(buffer.id)
            .or_insert_with(|| Syntax::for_path(buffer.path.as_deref()));
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
