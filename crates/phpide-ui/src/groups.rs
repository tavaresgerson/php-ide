//! Editor groups com documento compartilhado — `T-UI-ED-003-03/04`
//! (REQUISITOS UI-ED-003 P0: split right/down com buffers e caret
//! independentes).
//!
//! Modelo puro: a árvore de grupos referencia buffers por id; cada vista
//! mantém seu próprio caret/scroll/seleção. Duas vistas do mesmo buffer
//! **compartilham** o documento (a `Document`/rope do `phpide-core` é a fonte
//! única — dirty/undo ficam no Buffer, fora da vista), mas navegam de forma
//! independente; fechar uma vista não fecha a outra (aceite UI-ED-003).

use std::collections::HashMap;

/// Identificador estável de buffer (path ou id interno).
pub type BufferId = String;

/// Estado de navegação de UMA vista (independente por definição).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub buffer: BufferId,
    /// Char offset do caret.
    pub cursor: usize,
    /// Seleção (char offsets, start≤end).
    pub selection: Option<(usize, usize)>,
    /// Scroll vertical em linhas (fração tratada pelo egui; aqui linha base).
    pub top_line: usize,
}

impl ViewState {
    pub fn new(buffer: BufferId) -> Self {
        ViewState {
            buffer,
            cursor: 0,
            selection: None,
            top_line: 0,
        }
    }
}

/// Nó da árvore de layout.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupNode {
    /// Folha: um grupo com uma ou mais vistas (abas internas).
    Leaf {
        views: Vec<ViewState>,
        active: usize,
    },
    /// Divisão horizontal (panes lado a lado).
    Split {
        axis: Axis,
        children: Vec<GroupNode>,
        sizes: Vec<f32>,
        active: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Grupos de editores.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorGroups {
    pub root: GroupNode,
    /// Foco corrente (caminho implícito: folha ativa da raiz).
    pub focused: usize,
}

impl Default for EditorGroups {
    fn default() -> Self {
        EditorGroups {
            root: GroupNode::Leaf {
                views: Vec::new(),
                active: 0,
            },
            focused: 0,
        }
    }
}

impl EditorGroups {
    /// Abre um buffer na folha focada (reutiliza vista existente).
    pub fn open(&mut self, buffer: BufferId) {
        open_in(&mut self.root, buffer);
    }

    /// Move o foco para o filho i da raiz (recorte P0: split único).
    #[allow(dead_code)]
    pub fn focus_child(&mut self, i: usize) {
        if let GroupNode::Split { active, .. } = &mut self.root {
            *active = i.min(1);
        }
    }

    /// Conta vistas de um buffer (compartilhamento).
    #[allow(dead_code)]
    pub fn view_count_of(&self, buffer: &str) -> usize {
        fn count(node: &GroupNode, buffer: &str) -> usize {
            match node {
                GroupNode::Leaf { views, .. } => {
                    views.iter().filter(|v| v.buffer == buffer).count()
                }
                GroupNode::Split { children, .. } => {
                    children.iter().map(|c| count(c, buffer)).sum()
                }
            }
        }
        count(&self.root, buffer)
    }

    /// Vista ativa da raiz.
    #[allow(dead_code)]
    pub fn active_view(&self) -> Option<&ViewState> {
        fn find(node: &GroupNode) -> Option<&ViewState> {
            match node {
                GroupNode::Leaf { views, active } => views.get(*active),
                GroupNode::Split {
                    children, active, ..
                } => children.get(*active).and_then(find),
            }
        }
        find(&self.root)
    }

    /// Divide a folha da raiz no eixo (Split Right = Horizontal,
    /// Split Down = Vertical): a vista ativa move para o novo grupo.
    pub fn split_root(&mut self, axis: Axis) {
        let GroupNode::Leaf { views, active } = &mut self.root else {
            // Recorte P0: split do grupo raiz; aninhados seguem por modelo.
            return;
        };
        if views.is_empty() {
            return;
        }
        let idx = (*active).min(views.len() - 1);
        let moved = views.remove(idx);
        if views.is_empty() {
            // Única vista: mantém a original e adiciona o novo grupo.
            views.push(ViewState::new(moved.buffer.clone()));
        }
        self.root = GroupNode::Split {
            axis,
            children: vec![
                GroupNode::Leaf {
                    views: std::mem::take(views),
                    active: 0,
                },
                GroupNode::Leaf {
                    views: vec![moved],
                    active: 0,
                },
            ],
            sizes: vec![0.5, 0.5],
            active: 1,
        };
    }

    /// Fecha a vista ativa do grupo informado; grupo vazio some (a outra
    /// vista do mesmo buffer continua — fechar uma não fecha a outra).
    #[allow(dead_code)]
    pub fn close_active_in_root_second(&mut self) -> Option<BufferId> {
        let GroupNode::Split { children, .. } = &mut self.root else {
            return None;
        };
        let closed = if let GroupNode::Leaf { views, active } = &mut children[1] {
            let v = views.get(*active).cloned();
            if !views.is_empty() {
                let idx = (*active).min(views.len() - 1);
                views.remove(idx);
                if *active >= views.len() && !views.is_empty() {
                    *active = views.len() - 1;
                }
            }
            v.map(|v| v.buffer)
        } else {
            None
        };
        // Se o segundo grupo esvaziou, colapsa o split.
        #[allow(clippy::collapsible_if)]
        if let GroupNode::Leaf { views, .. } = &children[1] {
            if views.is_empty() {
                let first = children.remove(0);
                self.root = first;
            }
        }
        closed
    }

    /// Rótulos para debug/teste: buffers em cada folha.
    #[allow(dead_code)]
    pub fn leaf_buffers(&self) -> Vec<Vec<BufferId>> {
        fn leaves(node: &GroupNode, out: &mut Vec<Vec<BufferId>>) {
            match node {
                GroupNode::Leaf { views, .. } => {
                    out.push(views.iter().map(|v| v.buffer.clone()).collect());
                }
                GroupNode::Split { children, .. } => {
                    for c in children {
                        leaves(c, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        leaves(&self.root, &mut out);
        out
    }
}

fn open_in(node: &mut GroupNode, buffer: BufferId) {
    match node {
        GroupNode::Leaf { views, active } => {
            if let Some(i) = views.iter().position(|v| v.buffer == buffer) {
                *active = i;
                return;
            }
            views.push(ViewState::new(buffer));
            *active = views.len() - 1;
        }
        GroupNode::Split {
            children, active, ..
        } => {
            let idx = (*active).min(children.len().saturating_sub(1));
            open_in(&mut children[idx], buffer);
        }
    }
}

/// Rótulos de buffers para títulos de aba (extensão em destaque).
pub fn tab_title(path: &str, dirty: bool, pinned: bool) -> String {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let mark = if pinned { "📌 " } else { "" };
    let dirt = if dirty { " ●" } else { "" };
    format!("{mark}{name}{dirt}")
}

/// Compat: mapa de nomes simples (reservado para lotes de migração).
#[allow(dead_code)]
pub type Titles = HashMap<BufferId, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_right_shares_document_and_closes_independently() {
        let mut g = EditorGroups::default();
        g.open("a.php".into());
        g.open("b.php".into());
        // Foca a.php e divide para a direita: a.php MUDA para o novo grupo;
        // b.php fica no primeiro.
        if let GroupNode::Leaf { views, active } = &mut g.root {
            *active = views.iter().position(|v| v.buffer == "a.php").unwrap();
        }
        g.split_root(Axis::Horizontal);
        assert_eq!(g.leaf_buffers(), vec![vec!["b.php"], vec!["a.php"]]);
        // Abrir a.php no primeiro grupo também: duas vistas compartilhando o
        // documento (dirty/undo vivem no Buffer, fora da vista).
        g.focus_child(0);
        g.open("a.php".into());
        assert_eq!(g.view_count_of("a.php"), 2, "mesmo buffer em duas vistas");
        // Fechar a vista do segundo grupo não fecha a do primeiro.
        let closed = g.close_active_in_root_second();
        assert_eq!(closed.as_deref(), Some("a.php"));
        assert_eq!(g.view_count_of("a.php"), 1, "outra vista segue viva");
        assert_eq!(
            g.leaf_buffers(),
            vec![vec!["b.php", "a.php"]],
            "split colapsou"
        );
    }

    #[test]
    fn split_down_axis_and_views_navigate_independently() {
        let mut g = EditorGroups::default();
        g.open("x.php".into());
        g.split_root(Axis::Vertical);
        let GroupNode::Split { axis, .. } = &g.root else {
            panic!("esperava split");
        };
        assert_eq!(*axis, Axis::Vertical);
        // Vista ativa é a movida (novo grupo); com uma única vista aberta o
        // primeiro grupo recebe uma segunda vista do MESMO buffer — é o
        // comportamento Split: o arquivo aparece nos dois lados.
        assert_eq!(g.active_view().unwrap().buffer, "x.php");
        assert_eq!(g.leaf_buffers(), vec![vec!["x.php"], vec!["x.php"]]);
        // Navegação independente: abrir y.php só no primeiro grupo.
        let GroupNode::Split { children, .. } = &mut g.root else {
            panic!("split esperado");
        };
        if let GroupNode::Leaf { views, .. } = &mut children[0] {
            views.push(ViewState::new("y.php".into()));
        }
        let leaves = g.leaf_buffers();
        assert_eq!(leaves[0], vec!["x.php", "y.php"]);
        assert_eq!(leaves[1], vec!["x.php"], "segundo grupo intocado");
    }

    #[test]
    fn reopen_focuses_existing_view() {
        let mut g = EditorGroups::default();
        g.open("a".into());
        g.open("b".into());
        g.open("a".into());
        if let GroupNode::Leaf { views, active } = &g.root {
            assert_eq!(views.len(), 2, "sem duplicar vista");
            assert_eq!(views[*active].buffer, "a");
        } else {
            panic!("folha esperada");
        }
    }
}
