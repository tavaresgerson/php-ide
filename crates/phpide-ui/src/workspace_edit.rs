//! Aplicação transacional de `WorkspaceEdit`: valida versões e ranges de
//! todos os documentos antes de mutar, restaura snapshots em falhas e registra
//! undo coeso somente após sucesso completo. Edits versionados para arquivos
//! fechados são recusados porque não existe revisão local confiável; edits sem
//! versão podem ser aplicados após validação integral do texto lido do disco.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};
use phpide_lsp::edits::WorkspaceEdit;

pub trait DocStore {
    fn text(&self, path: &Path) -> Result<String, IdeError>;
    fn set_text(&mut self, path: &Path, text: &str) -> Result<(), IdeError>;
    fn lsp_version(&self, path: &Path) -> Option<i64>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefactorUndo {
    pub label: String,
    pub pre_state: Vec<(PathBuf, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct UndoStack(Vec<RefactorUndo>);

impl UndoStack {
    pub fn push(&mut self, undo: RefactorUndo) {
        self.0.push(undo);
    }

    pub fn pop(&mut self) -> Option<RefactorUndo> {
        self.0.pop()
    }

    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.0.len()
    }

    pub fn top_label(&self) -> Option<&str> {
        self.0.last().map(|u| u.label.as_str())
    }
}

fn error(kind: ErrorKind, message: impl Into<String>) -> IdeError {
    IdeError::new(
        Component::Lsp,
        "workspace_edit",
        kind,
        message,
        "refresh the documents and retry; no outdated edit was applied",
    )
}

fn replace_plan(
    store: &mut dyn DocStore,
    plan: &[(PathBuf, String, String)],
) -> Result<(), IdeError> {
    for index in 0..plan.len() {
        let (path, _, new_text) = &plan[index];
        if let Err(cause) = store.set_text(path, new_text) {
            let mut rollback_errors = Vec::new();
            for (old_path, old_text, _) in plan[..=index].iter().rev() {
                if let Err(rollback) = store.set_text(old_path, old_text) {
                    rollback_errors.push(format!("{}: {rollback}", old_path.display()));
                }
            }
            let mut failure = error(
                cause.kind,
                format!("could not apply the workspace edit to {}", path.display()),
            )
            .with_detail(cause.log_line());
            if rollback_errors.is_empty() {
                failure = failure
                    .with_detail(format!("{}; all documents were restored", cause.log_line()));
            } else {
                failure = failure.with_detail(format!(
                    "{}; partial success: rollback failed for {}",
                    cause.log_line(),
                    rollback_errors.join(" | ")
                ));
            }
            return Err(failure);
        }
    }
    Ok(())
}

pub fn apply(
    store: &mut dyn DocStore,
    edit: &WorkspaceEdit,
    label: &str,
    stack: &mut UndoStack,
) -> Result<(usize, usize, Vec<String>), IdeError> {
    let mut seen = HashSet::new();
    let mut plan = Vec::with_capacity(edit.documents.len());
    for doc in &edit.documents {
        if !seen.insert(doc.path.clone()) {
            return Err(error(
                ErrorKind::InvalidInput,
                format!(
                    "document repeated in the workspace edit: {}",
                    doc.path.display()
                ),
            ));
        }
        let old_text = store.text(&doc.path)?;
        match (doc.version, store.lsp_version(&doc.path)) {
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(error(
                    ErrorKind::InvalidInput,
                    format!(
                        "version conflict in {}: server {}, local {}",
                        doc.path.display(),
                        expected,
                        actual
                    ),
                ));
            }
            (Some(_), None) => {
                return Err(error(
                    ErrorKind::InvalidInput,
                    format!(
                        "cannot verify the version of a closed file: {}",
                        doc.path.display()
                    ),
                ));
            }
            _ => {}
        }
        let new_text =
            phpide_lsp::edits::apply_to_text(&old_text, edit, &doc.path).ok_or_else(|| {
                error(
                    ErrorKind::InvalidInput,
                    format!("invalid or overlapping range in {}", doc.path.display()),
                )
            })?;
        plan.push((doc.path.clone(), old_text, new_text));
    }
    replace_plan(store, &plan)?;
    stack.push(RefactorUndo {
        label: label.to_string(),
        pre_state: plan
            .iter()
            .map(|(path, old, _)| (path.clone(), old.clone()))
            .collect(),
    });
    Ok((plan.len(), edit.edit_count(), Vec::new()))
}

pub fn undo_last(
    store: &mut dyn DocStore,
    stack: &mut UndoStack,
) -> Result<Option<String>, IdeError> {
    let Some(undo) = stack.0.last().cloned() else {
        return Ok(None);
    };
    let mut plan = Vec::with_capacity(undo.pre_state.len());
    for (path, previous) in &undo.pre_state {
        let current = store.text(path)?;
        plan.push((path.clone(), current, previous.clone()));
    }
    replace_plan(store, &plan)?;
    stack.pop();
    Ok(Some(undo.label))
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
        fail_next_write: Option<PathBuf>,
        fail_after_mutation: bool,
    }

    impl DocStore for Mem {
        fn text(&self, path: &Path) -> Result<String, IdeError> {
            self.files.get(path).cloned().ok_or_else(|| {
                error(
                    ErrorKind::NotFound,
                    format!("arquivo inacessível: {}", path.display()),
                )
            })
        }

        fn set_text(&mut self, path: &Path, text: &str) -> Result<(), IdeError> {
            let fail = self.fail_next_write.as_deref() == Some(path);
            if fail {
                self.fail_next_write = None;
            }
            if !fail || self.fail_after_mutation {
                self.files.insert(path.to_path_buf(), text.to_string());
            }
            if fail {
                Err(error(ErrorKind::Io, "falha de escrita injetada"))
            } else {
                Ok(())
            }
        }

        fn lsp_version(&self, path: &Path) -> Option<i64> {
            self.versions.get(path).copied()
        }
    }

    fn text_edit(line: u32, column: u32, new_text: &str) -> TextEdit {
        TextEdit {
            start_line: line,
            start_col_utf16: column,
            end_line: line,
            end_col_utf16: column,
            new_text: new_text.into(),
        }
    }

    fn two_file_edit(second_line: u32) -> WorkspaceEdit {
        WorkspaceEdit {
            documents: vec![
                DocumentEdits {
                    path: PathBuf::from("/a.php"),
                    version: Some(1),
                    edits: vec![text_edit(0, 0, "use App\\Util;\n")],
                },
                DocumentEdits {
                    path: PathBuf::from("/b.php"),
                    version: Some(1),
                    edits: vec![text_edit(second_line, 0, "$u = new Util();\n")],
                },
            ],
        }
    }

    fn memory() -> Mem {
        let mut mem = Mem::default();
        mem.files.insert(PathBuf::from("/a.php"), "<?php\n".into());
        mem.files
            .insert(PathBuf::from("/b.php"), "<?php\r\necho 1;\r\n".into());
        mem.versions.insert(PathBuf::from("/a.php"), 1);
        mem.versions.insert(PathBuf::from("/b.php"), 1);
        mem
    }

    #[test]
    fn applies_multi_file_and_undo_restores_all() {
        let mut mem = memory();
        let mut stack = UndoStack::default();
        let (files, edits, conflicts) =
            apply(&mut mem, &two_file_edit(1), "auto-import", &mut stack).unwrap();
        assert_eq!((files, edits), (2, 2));
        assert!(conflicts.is_empty());
        assert!(mem.files[Path::new("/a.php")].starts_with("use App\\Util;"));
        assert!(mem.files[Path::new("/b.php")].contains("$u = new Util();\n"));
        assert_eq!(stack.depth(), 1);
        assert_eq!(
            undo_last(&mut mem, &mut stack).unwrap().as_deref(),
            Some("auto-import")
        );
        assert_eq!(mem.files[Path::new("/a.php")], "<?php\n");
        assert_eq!(mem.files[Path::new("/b.php")], "<?php\r\necho 1;\r\n");
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn version_conflict_blocks_before_any_mutation() {
        let mut mem = memory();
        mem.versions.insert(PathBuf::from("/b.php"), 2);
        let before = mem.files.clone();
        let mut stack = UndoStack::default();
        let err = apply(&mut mem, &two_file_edit(1), "rename", &mut stack).unwrap_err();
        assert!(err.message.contains("version conflict"));
        assert_eq!(mem.files, before);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn invalid_second_document_preserves_first() {
        let mut mem = memory();
        let before = mem.files.clone();
        let mut stack = UndoStack::default();
        apply(&mut mem, &two_file_edit(99), "rename", &mut stack).unwrap_err();
        assert_eq!(mem.files, before);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn disk_failure_rolls_back_every_document_and_creates_no_undo() {
        let mut mem = memory();
        let before = mem.files.clone();
        mem.fail_next_write = Some(PathBuf::from("/b.php"));
        mem.fail_after_mutation = true;
        let mut stack = UndoStack::default();
        let err = apply(&mut mem, &two_file_edit(1), "rename", &mut stack).unwrap_err();
        assert!(err.detail.unwrap().contains("restored"));
        assert_eq!(mem.files, before);
        assert_eq!(stack.depth(), 0);
        assert!(undo_last(&mut mem, &mut stack).unwrap().is_none());
    }

    #[test]
    fn versioned_closed_file_is_rejected_but_unversioned_is_validated() {
        let mut mem = memory();
        mem.versions.clear();
        let before = mem.files.clone();
        let mut stack = UndoStack::default();
        apply(&mut mem, &two_file_edit(1), "rename", &mut stack).unwrap_err();
        assert_eq!(mem.files, before);
        let mut unversioned = two_file_edit(1);
        for doc in &mut unversioned.documents {
            doc.version = None;
        }
        apply(&mut mem, &unversioned, "rename", &mut stack).unwrap();
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn eof_insert_and_undo_failure_are_safe() {
        let mut mem = Mem::default();
        mem.files.insert(PathBuf::from("/a.php"), "a\r\n".into());
        let edit = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a.php"),
                version: None,
                edits: vec![text_edit(1, 0, "b")],
            }],
        };
        let mut stack = UndoStack::default();
        apply(&mut mem, &edit, "eof", &mut stack).unwrap();
        assert_eq!(mem.files[Path::new("/a.php")], "a\r\nb");
        mem.fail_next_write = Some(PathBuf::from("/a.php"));
        let err = undo_last(&mut mem, &mut stack).unwrap_err();
        assert!(err.detail.unwrap().contains("restored"));
        assert_eq!(mem.files[Path::new("/a.php")], "a\r\nb");
        assert_eq!(stack.depth(), 1);
    }
}
