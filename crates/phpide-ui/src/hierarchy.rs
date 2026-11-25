//! Type Hierarchy (Ctrl+H) and Call Hierarchy (Ctrl+Alt+H): a tree of
//! supertypes or subtypes, callers or callees, loaded level by level from
//! the language server.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use phpide_icons::IconId;
use serde_json::Value;

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyKind {
    Type,
    Call,
}

impl HierarchyKind {
    /// Labels of the two directions: (upward, downward).
    pub fn directions(self) -> (&'static str, &'static str) {
        match self {
            HierarchyKind::Type => ("Supertypes", "Subtypes"),
            HierarchyKind::Call => ("Callers", "Callees"),
        }
    }
}

/// One item of the tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// The `TypeHierarchyItem` / `CallHierarchyItem`, sent back as is.
    pub item: Value,
    pub name: String,
    pub detail: String,
    pub kind: u64,
    pub path: Option<PathBuf>,
    pub line0: usize,
    pub col0: usize,
    pub depth: usize,
    /// `None` until loaded.
    pub children: Option<Vec<usize>>,
    pub expanded: bool,
}

/// A hierarchy item from the server.
pub fn node_of(item: &Value, depth: usize) -> Option<Node> {
    let name = item["name"].as_str()?.to_string();
    let at = &item["selectionRange"]["start"];
    Some(Node {
        item: item.clone(),
        name,
        detail: item["detail"].as_str().unwrap_or_default().to_string(),
        kind: item["kind"].as_u64().unwrap_or(0),
        path: item["uri"].as_str().and_then(phpide_lsp::uri_to_path),
        line0: at["line"].as_u64().unwrap_or(0) as usize,
        col0: at["character"].as_u64().unwrap_or(0) as usize,
        depth,
        children: None,
        expanded: false,
    })
}

/// The items of a children answer: type items directly; calls under
/// `from` (incoming) or `to` (outgoing).
pub fn child_items(kind: HierarchyKind, upward: bool, value: &Value) -> Vec<Value> {
    let list = value.as_array().cloned().unwrap_or_default();
    match kind {
        HierarchyKind::Type => list,
        HierarchyKind::Call => {
            let key = if upward { "from" } else { "to" };
            list.iter().map(|call| call[key].clone()).collect()
        }
    }
}

#[derive(Default)]
pub struct HierarchyState {
    pub kind: Option<HierarchyKind>,
    /// Supertypes / callers when true.
    pub upward: bool,
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
    pub message: String,
    prepare: Option<mpsc::Receiver<Value>>,
    children: Vec<(usize, mpsc::Receiver<Value>)>,
}

/// Levels below the root a tree may reach (inheritance and recursion are
/// finite, call chains are not).
const MAX_DEPTH: usize = 32;

impl HierarchyState {
    fn reset(&mut self, kind: HierarchyKind, upward: bool) {
        *self = HierarchyState {
            kind: Some(kind),
            upward,
            ..Default::default()
        };
    }

    /// Rows to draw: (node, depth) in tree order.
    pub fn rows(&self) -> Vec<usize> {
        fn walk(state: &HierarchyState, index: usize, out: &mut Vec<usize>) {
            out.push(index);
            let node = &state.nodes[index];
            if node.expanded {
                for &child in node.children.iter().flatten() {
                    walk(state, child, out);
                }
            }
        }
        let mut out = Vec::new();
        for &root in &self.roots {
            walk(self, root, &mut out);
        }
        out
    }

    pub fn loading(&self) -> bool {
        self.prepare.is_some() || !self.children.is_empty()
    }
}

impl PhpIdeApp {
    /// Opens the hierarchy of the symbol at the caret.
    pub(crate) fn request_hierarchy(&mut self, kind: HierarchyKind) {
        self.sync_documents();
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            phpide_core::PositionEncoding::Utf16,
        );
        let Some(lsp) = self
            .lsp
            .as_ref()
            .filter(|l| matches!(l.status(), LspStatus::Running(_)))
        else {
            self.ws.message = "the language server is not ready yet".to_string();
            return;
        };
        let caps = lsp.capabilities();
        let (supported, what) = match kind {
            HierarchyKind::Type => (caps.type_hierarchy_provider, "type hierarchies"),
            HierarchyKind::Call => (caps.call_hierarchy_provider, "call hierarchies"),
        };
        if !supported {
            self.ws.message = format!("the language server does not support {what}");
            return;
        }
        let rx = match kind {
            HierarchyKind::Type => lsp.prepare_type_hierarchy(&path, line, col),
            HierarchyKind::Call => lsp.prepare_call_hierarchy(&path, line, col),
        };
        // Types open on their supertypes, calls on their callers (IntelliJ).
        self.hierarchy.reset(kind, true);
        self.hierarchy.prepare = Some(rx);
        self.tw.set_visible(ToolWindow::Hierarchy, true);
    }

    /// Asks for the children of `index`.
    fn load_children(&mut self, index: usize) {
        let (Some(kind), Some(lsp)) = (self.hierarchy.kind, self.lsp.as_ref()) else {
            return;
        };
        let item = &self.hierarchy.nodes[index].item;
        let rx = match kind {
            HierarchyKind::Type => lsp.type_hierarchy(item, self.hierarchy.upward),
            HierarchyKind::Call => lsp.call_hierarchy(item, self.hierarchy.upward),
        };
        self.hierarchy.children.push((index, rx));
    }

    /// Switches between supertypes/subtypes or callers/callees, keeping the
    /// root.
    fn set_hierarchy_direction(&mut self, upward: bool) {
        let Some(kind) = self.hierarchy.kind else {
            return;
        };
        if self.hierarchy.upward == upward {
            return;
        }
        let roots: Vec<Node> = self
            .hierarchy
            .roots
            .iter()
            .map(|&r| self.hierarchy.nodes[r].clone())
            .collect();
        self.hierarchy.reset(kind, upward);
        for mut node in roots {
            node.children = None;
            node.expanded = true;
            self.hierarchy.nodes.push(node);
            let index = self.hierarchy.nodes.len() - 1;
            self.hierarchy.roots.push(index);
            self.load_children(index);
        }
    }

    pub(crate) fn poll_hierarchy(&mut self) {
        if let Some(rx) = &self.hierarchy.prepare {
            match rx.try_recv() {
                Ok(result) => {
                    self.hierarchy.prepare = None;
                    let roots: Vec<Node> = result
                        .as_array()
                        .map(|a| a.iter().filter_map(|i| node_of(i, 0)).collect())
                        .unwrap_or_default();
                    if roots.is_empty() {
                        self.hierarchy.message = "No hierarchy for the symbol at the caret".into();
                    }
                    for mut node in roots {
                        node.expanded = true;
                        self.hierarchy.nodes.push(node);
                        let index = self.hierarchy.nodes.len() - 1;
                        self.hierarchy.roots.push(index);
                        self.load_children(index);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.hierarchy.prepare = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(kind) = self.hierarchy.kind else {
            return;
        };
        let upward = self.hierarchy.upward;
        let mut done = Vec::new();
        self.hierarchy
            .children
            .retain(|(index, rx)| match rx.try_recv() {
                Ok(result) => {
                    done.push((*index, result));
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done.push((*index, Value::Null));
                    false
                }
            });
        for (parent, result) in done {
            let depth = self.hierarchy.nodes[parent].depth + 1;
            let mut children = Vec::new();
            if depth <= MAX_DEPTH {
                for item in child_items(kind, upward, &result) {
                    if let Some(node) = node_of(&item, depth) {
                        self.hierarchy.nodes.push(node);
                        children.push(self.hierarchy.nodes.len() - 1);
                    }
                }
            }
            self.hierarchy.nodes[parent].children = Some(children);
        }
    }

    pub(crate) fn hierarchy_panel(&mut self, ui: &mut egui::Ui) {
        use crate::design::components::{EmptyState, ListRow};
        let placement = self.tw.placement(ToolWindow::Hierarchy);
        crate::toolwindows::dock(ui, ToolWindow::Hierarchy, placement, 260.0, |ui| {
            crate::app_panels::tool_window_header(self, ui, ToolWindow::Hierarchy);
            let Some(kind) = self.hierarchy.kind else {
                EmptyState::new(IconId::Hierarchy, "No hierarchy")
                    .description(
                        "Type Hierarchy (Ctrl+H) and Call Hierarchy (Ctrl+Alt+H) of the symbol at the caret appear here.",
                    )
                    .show(ui);
                return;
            };
            let (up, down) = kind.directions();
            let mut upward = self.hierarchy.upward;
            ui.horizontal(|ui| {
                if ui.selectable_label(upward, up).clicked() {
                    upward = true;
                }
                if ui.selectable_label(!upward, down).clicked() {
                    upward = false;
                }
                if self.hierarchy.loading() {
                    ui.add(egui::Spinner::new().size(12.0));
                }
            });
            if upward != self.hierarchy.upward {
                self.set_hierarchy_direction(upward);
            }
            if !self.hierarchy.message.is_empty() && self.hierarchy.roots.is_empty() {
                ui.label(egui::RichText::new(&self.hierarchy.message).weak());
                return;
            }
            let p = crate::design::current(ui.ctx()).palette;
            let mut toggle = None;
            let mut open = None;
            egui::ScrollArea::vertical()
                .id_salt("hierarchy-tree")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for index in self.hierarchy.rows() {
                        let node = &self.hierarchy.nodes[index];
                        let leaf = node.children.as_ref().is_some_and(Vec::is_empty);
                        let row = ui
                            .horizontal(|ui| {
                                ui.add_space(16.0 * node.depth as f32);
                                let chevron = if leaf {
                                    None
                                } else if node.expanded {
                                    Some(IconId::ChevronDown)
                                } else {
                                    Some(IconId::ChevronRight)
                                };
                                match chevron {
                                    Some(icon) => {
                                        if crate::design::components::IconButton::new(
                                            icon,
                                            if node.expanded { "Collapse" } else { "Expand" },
                                        )
                                        .show(ui)
                                        .clicked()
                                        {
                                            toggle = Some(index);
                                        }
                                    }
                                    None => ui.add_space(20.0),
                                }
                                let file = node
                                    .path
                                    .as_ref()
                                    .and_then(|p| p.file_name())
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                ListRow::new(&node.name)
                                    .icon(
                                        crate::app_intel::symbol_icon(&symbol_kind(node.kind)),
                                        p.accent,
                                    )
                                    .detail(node.detail.clone())
                                    .trailing(format!("{file}:{}", node.line0 + 1))
                                    .show(ui)
                            })
                            .inner;
                        if row.clicked() {
                            open = Some(index);
                        }
                        if row.double_clicked() {
                            toggle = Some(index);
                        }
                    }
                });
            if let Some(index) = toggle {
                let node = &mut self.hierarchy.nodes[index];
                node.expanded = !node.expanded;
                if node.expanded && node.children.is_none() {
                    self.load_children(index);
                }
            }
            if let Some(index) = open {
                let node = &self.hierarchy.nodes[index];
                if let Some(path) = node.path.clone() {
                    let (line0, col0) = (node.line0, node.col0);
                    self.ws.open_file_at(&path, line0, col0);
                }
            }
        });
    }
}

/// The Structure kind name of an LSP `SymbolKind`.
fn symbol_kind(kind: u64) -> String {
    match kind {
        5 => "class",
        6 => "method",
        10 => "enum",
        11 => "interface",
        12 => "function",
        23 => "struct",
        _ => "symbol",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(name: &str, line: u64) -> Value {
        json!({
            "name": name, "kind": 5, "detail": "App",
            "uri": "file:///p/a.php",
            "range": { "start": { "line": line, "character": 0 }, "end": { "line": line + 3, "character": 1 } },
            "selectionRange": { "start": { "line": line, "character": 6 }, "end": { "line": line, "character": 9 } }
        })
    }

    #[test]
    fn items_and_children() {
        let node = node_of(&item("Cat", 4), 1).unwrap();
        assert_eq!(
            (node.name.as_str(), node.line0, node.col0, node.depth),
            ("Cat", 4, 6, 1)
        );
        assert_eq!(node.path, Some(PathBuf::from("/p/a.php")));

        let types = json!([item("Animal", 0)]);
        assert_eq!(child_items(HierarchyKind::Type, true, &types).len(), 1);
        let incoming = json!([{ "from": item("main", 9), "fromRanges": [] }]);
        assert_eq!(
            child_items(HierarchyKind::Call, true, &incoming)[0]["name"],
            "main"
        );
        let outgoing = json!([{ "to": item("helper", 2), "fromRanges": [] }]);
        assert_eq!(
            child_items(HierarchyKind::Call, false, &outgoing)[0]["name"],
            "helper"
        );
    }

    #[test]
    fn rows_follow_expansion() {
        let mut state = HierarchyState::default();
        for (name, depth) in [("Cat", 0), ("Animal", 1), ("Living", 2)] {
            state.nodes.push(node_of(&item(name, 0), depth).unwrap());
        }
        state.roots = vec![0];
        state.nodes[0].children = Some(vec![1]);
        state.nodes[1].children = Some(vec![2]);
        assert_eq!(state.rows(), [0]);
        state.nodes[0].expanded = true;
        assert_eq!(state.rows(), [0, 1]);
        state.nodes[1].expanded = true;
        assert_eq!(state.rows(), [0, 1, 2]);
    }
}
