//! WorkspaceEdit multiarquivo com undo coeso — `T-PHP-021-01` (infra) e
//! alvo do auto-import `T-PHP-003-05`.
//!
//! [`apply`] aplica um `WorkspaceEdit` (LSP) sobre um [`DocStore`]:
//! arquivos abertos são editados no buffer (ficam dirty); arquivos fechados
//! vão ao disco com escrita atômica. Antes de aplicar, snapshot de todos os
//! arquivos afetados é empilhado num único [`RefactorUndo`] — desfazer a
//! refatoração restaura **todos** os arquivos de uma vez (undo coeso,
//! NFR-007-03), independentemente do undo por buffer.
//!
//! Conflitos: `DocumentEdits.version` divergente de um buffer aberto gera
//! aviso no preview (a aplicação prossegue — a versão LSP é informativa
//! para o servidor, não trava a edição local; documentado).

use std::path::{Path, PathBuf};

use phpide_lsp::edits::WorkspaceEdit;

/// Acesso a documentos (buffer aberto ou disco) — implementado pela UI.
pub trait DocStore {
    /// Texto corrente do arquivo (buffer sujo tem prioridade).
    fn text(&self, path: &Path) -> Option<String>;
    /// Substitui o conteúdo (buffer aberto ou disco atômico).
    fn set_text(&mut self, path: &Path, text: &str);
    /// Versão conhecida (LSP) do buffer aberto, se houver.
    fn lsp_version(&self, path: &Path) -> Option<i64>;
}

/// Um passo de refatoração desfazível de uma só vez.
#[derive(Debug, Clone, PartialEq)]
pub struct RefactorUndo {
    pub label: String,
    /// (arquivo, conteúdo pré-refatoração)
    pub pre_state: Vec<(PathBuf, String)>,
}

/// Pilha de undos de refatoração (LIFO).
#[derive(Debug, Clone, Default)]
pub struct UndoStack(Vec<RefactorUndo>);

impl UndoStack {
    pub fn push(&mut self, undo: RefactorUndo) {
        self.0.push(undo);
    }

    pub fn pop(&mut self) -> Option<RefactorUndo> {
        self.0.pop()
    }

    #[allow(dead_code)]
    pub fn depth(&self) -> usize {
        self.0.len()
    }

    pub fn top_label(&self) -> Option<&str> {
        self.0.last().map(|u| u.label.as_str())
    }
}

/// Aplica o WorkspaceEdit inteiro e empilha o undo coeso. Devolve resumo
/// `(arquivos, editos, conflitos)` ou erro recuperável quando algum arquivo
/// é inacessível (nada é aplicado — snapshot antes de mutar).
pub fn apply(
    store: &mut dyn DocStore,
    edit: &WorkspaceEdit,
    label: &str,
    stack: &mut UndoStack,
) -> Result<(usize, usize, Vec<String>), phpide_core::IdeError> {
    if edit.documents.is_empty() {
        return Ok((0, 0, Vec::new()));
    }
    let mut pre_state: Vec<(PathBuf, String)> = Vec::new();
    let mut conflicts = Vec::new();
    let mut total_edits = 0;
    // Snapshot antes de qualquer mutação (undo atômico).
    for doc in &edit.documents {
        let text = store.text(&doc.path).ok_or_else(|| {
            phpide_core::IdeError::new(
                phpide_core::Component::Lsp,
                "workspace_edit",
                phpide_core::ErrorKind::Io,
                format!("arquivo inacessível: {}", doc.path.display()),
                "abra o arquivo ou verifique permissões e tente de novo",
            )
        })?;
        if let (Some(want), Some(have)) = (doc.version, store.lsp_version(&doc.path)) {
            if want != have {
                conflicts.push(format!(
                    "{}: versão LSP diverge (servidor {}, local {})",
                    doc.path.display(),
                    want,
                    have
                ));
            }
        }
        pre_state.push((doc.path.clone(), text));
    }
    // Aplica.
    for (doc, (_, _)) in edit.documents.iter().zip(pre_state.iter()) {
        let new_text = phpide_lsp::edits::apply_to_text(
            &pre_state
                .iter()
                .find(|(p, _)| p == &doc.path)
                .map(|(_, t)| t.clone())
                .unwrap_or_default(),
            edit,
            &doc.path,
        )
        .ok_or_else(|| {
            phpide_core::IdeError::new(
                phpide_core::Component::Lsp,
                "workspace_edit",
                phpide_core::ErrorKind::InvalidInput,
                format!("edito inválido em {}", doc.path.display()),
                "reporte ao provedor da refatoração",
            )
        })?;
        store.set_text(&doc.path, &new_text);
        total_edits += doc.edits.len();
    }
    stack.push(RefactorUndo {
        label: label.to_string(),
        pre_state,
    });
    Ok((edit.documents.len(), total_edits, conflicts))
}

/// Desfaz o último passo: restaura o conteúdo pré-refatoração de todos os
/// arquivos (abertos ou em disco).
pub fn undo_last(store: &mut dyn DocStore, stack: &mut UndoStack) -> Option<String> {
    let undo = stack.pop()?;
    for (path, pre) in &undo.pre_state {
        store.set_text(path, pre);
    }
    Some(undo.label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_lsp::edits::{DocumentEdits, TextEdit};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Mem {
        files: HashMap<PathBuf, String>,
        versions: HashMap<PathBuf, i64>,
    }
    impl DocStore for Mem {
        fn text(&self, path: &Path) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn set_text(&mut self, path: &Path, text: &str) {
            self.files.insert(path.to_path_buf(), text.to_string());
        }
        fn lsp_version(&self, path: &Path) -> Option<i64> {
            self.versions.get(path).copied()
        }
    }

    fn use_edit() -> WorkspaceEdit {
        WorkspaceEdit {
            documents: vec![
                DocumentEdits {
                    path: PathBuf::from("/a.php"),
                    version: Some(1),
                    edits: vec![TextEdit {
                        start_line: 0,
                        start_col_utf16: 0,
                        end_line: 0,
                        end_col_utf16: 0,
                        new_text: "use App\\Util;\n".into(),
                    }],
                },
                DocumentEdits {
                    path: PathBuf::from("/b.php"),
                    version: Some(1),
                    edits: vec![TextEdit {
                        start_line: 1,
                        start_col_utf16: 0,
                        end_line: 1,
                        end_col_utf16: 0,
                        new_text: "$u = new Util();\n".into(),
                    }],
                },
            ],
        }
    }

    #[test]
    fn applies_multi_file_and_undo_restores_all() {
        let mut mem = Mem::default();
        mem.files.insert(PathBuf::from("/a.php"), "<?php\n".into());
        mem.files
            .insert(PathBuf::from("/b.php"), "<?php\necho 1;\n".into());
        mem.versions.insert(PathBuf::from("/a.php"), 1);

        let mut stack = UndoStack::default();
        let edit = use_edit();
        let (files, edits, conflicts) = apply(&mut mem, &edit, "auto-import", &mut stack).unwrap();
        assert_eq!((files, edits), (2, 2));
        assert!(conflicts.is_empty());
        assert!(mem.files[&PathBuf::from("/a.php")].starts_with("use App\\Util;"));
        assert!(mem.files[&PathBuf::from("/b.php")]
            .lines()
            .nth(1)
            .unwrap()
            .starts_with("$u"));
        assert_eq!(stack.depth(), 1);

        // Undo coeso: um passo restaura os DOIS arquivos.
        let label = undo_last(&mut mem, &mut stack).unwrap();
        assert_eq!(label, "auto-import");
        assert_eq!(mem.files[&PathBuf::from("/a.php")], "<?php\n");
        assert_eq!(mem.files[&PathBuf::from("/b.php")], "<?php\necho 1;\n");
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn version_conflict_is_reported_not_fatal() {
        let mut mem = Mem::default();
        mem.files.insert(PathBuf::from("/a.php"), "x\n".into());
        mem.versions.insert(PathBuf::from("/a.php"), 9);
        let edit = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a.php"),
                version: Some(3),
                edits: vec![TextEdit {
                    start_line: 0,
                    start_col_utf16: 0,
                    end_line: 0,
                    end_col_utf16: 1,
                    new_text: "y".into(),
                }],
            }],
        };
        let mut stack = UndoStack::default();
        let (_, _, conflicts) = apply(&mut mem, &edit, "rename", &mut stack).unwrap();
        assert_eq!(conflicts.len(), 1, "divergência reportada");
        assert_eq!(
            mem.files[&PathBuf::from("/a.php")],
            "y\n",
            "edição aplicada mesmo assim"
        );
    }

    #[test]
    fn missing_file_is_recoverable_error() {
        let mut mem = Mem::default();
        let edit = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/nope.php"),
                version: None,
                edits: vec![],
            }],
        };
        let mut stack = UndoStack::default();
        let err = apply(&mut mem, &edit, "x", &mut stack).unwrap_err();
        assert_eq!(err.kind, phpide_core::ErrorKind::Io);
        assert!(!err.action.is_empty());
    }
}
