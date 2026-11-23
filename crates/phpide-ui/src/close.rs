use std::path::PathBuf;

use phpide_core::Workspace;

use crate::PendingClose;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub index: usize,
    pub path: Option<PathBuf>,
    pub saved: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveReport {
    pub outcomes: Vec<SaveOutcome>,
}

impl SaveReport {
    pub fn all_saved(&self) -> bool {
        self.outcomes.iter().all(|outcome| outcome.saved)
    }

    pub fn failed(&self) -> impl Iterator<Item = &SaveOutcome> {
        self.outcomes.iter().filter(|outcome| !outcome.saved)
    }

    pub fn summary(&self) -> String {
        let failed = self.failed().count();
        if failed == 0 {
            format!("{} file(s) saved", self.outcomes.len())
        } else {
            format!(
                "{failed} file(s) could not be saved; closing was cancelled and your edits are kept"
            )
        }
    }
}

pub fn targets(pending: &PendingClose, buffer_count: usize) -> Vec<usize> {
    match pending {
        PendingClose::Tab(index) => vec![*index],
        PendingClose::Others(keep) => (0..buffer_count).filter(|i| i != keep).collect(),
        PendingClose::Project(_) | PendingClose::Exit => (0..buffer_count).collect(),
    }
}

pub fn save_dirty_indices(
    descriptors: &[(usize, Option<PathBuf>, bool)],
    mut save: impl FnMut(usize) -> bool,
) -> SaveReport {
    let outcomes = descriptors
        .iter()
        .filter(|(_, _, dirty)| *dirty)
        .map(|(index, path, _)| SaveOutcome {
            index: *index,
            path: path.clone(),
            saved: save(*index),
        })
        .collect();
    SaveReport { outcomes }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseAction {
    Closed,
    LoadProject(PathBuf),
    Exit,
    Blocked,
}

pub fn after_successful_save(
    ws: &mut Workspace,
    pending: &PendingClose,
    report: &SaveReport,
) -> CloseAction {
    let indices = targets(pending, ws.buffers.len());
    let still_dirty = indices
        .iter()
        .any(|i| ws.buffers.get(*i).map(|b| b.dirty).unwrap_or(true));
    if !report.all_saved() || still_dirty {
        return CloseAction::Blocked;
    }
    match pending {
        PendingClose::Tab(index) => {
            if *index >= ws.buffers.len() {
                CloseAction::Blocked
            } else {
                ws.close_tab(*index);
                CloseAction::Closed
            }
        }
        PendingClose::Others(keep) => {
            ws.close_other_tabs(*keep);
            CloseAction::Closed
        }
        PendingClose::Project(path) => CloseAction::LoadProject(path.clone()),
        PendingClose::Exit => CloseAction::Exit,
    }
}

pub fn cancel(pending: &mut Option<PendingClose>) {
    *pending = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::Buffer;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("valid.php");
        std::fs::write(&valid, "<?php\n").unwrap();
        let mut ws = Workspace::open(dir.path().to_path_buf());
        ws.buffers.clear();
        ws.buffers.push(Buffer::open(&valid).unwrap());
        ws.buffers.push(Buffer::open(&valid).unwrap());
        ws.buffers[0].insert(6, "// um\n");
        ws.buffers[1].insert(6, "// dois\n");
        ws.buffers[1].path = Some(dir.path().to_path_buf());
        (dir, ws)
    }

    fn report(indices: &[usize], fail: Option<usize>) -> SaveReport {
        SaveReport {
            outcomes: indices
                .iter()
                .map(|i| SaveOutcome {
                    index: *i,
                    path: None,
                    saved: Some(*i) != fail,
                })
                .collect(),
        }
    }

    #[test]
    fn detailed_report_covers_valid_and_invalid_save() {
        let (_dir, mut ws) = workspace();
        let descriptors = vec![
            (0, ws.buffers[0].path.clone(), ws.buffers[0].dirty),
            (1, ws.buffers[1].path.clone(), ws.buffers[1].dirty),
        ];
        let report = save_dirty_indices(&descriptors, |i| ws.buffers[i].try_save().is_ok());
        assert_eq!(report.outcomes.len(), 2);
        assert!(report.outcomes[0].saved);
        assert!(!report.outcomes[1].saved);
        assert!(!ws.buffers[0].dirty);
        assert!(ws.buffers[1].dirty);
        assert!(ws.buffers[1].document.text().contains("// dois"));
    }

    #[test]
    fn every_close_flow_is_blocked_by_failed_save() {
        let cases = [
            PendingClose::Tab(1),
            PendingClose::Others(0),
            PendingClose::Project(PathBuf::from("/outro")),
            PendingClose::Exit,
        ];
        for pending in cases {
            let (_dir, mut ws) = workspace();
            let before = (ws.root.clone(), ws.buffers.len());
            let indices = targets(&pending, ws.buffers.len());
            let action = after_successful_save(&mut ws, &pending, &report(&indices, Some(1)));
            assert_eq!(action, CloseAction::Blocked, "{pending:?}");
            assert_eq!(ws.root, before.0);
            assert_eq!(ws.buffers.len(), before.1);
            assert!(ws.buffers[1].dirty);
            assert!(ws.buffers[1].document.text().contains("// dois"));
        }
    }

    #[test]
    fn successful_save_allows_each_close_action() {
        let (_dir, mut ws) = workspace();
        ws.buffers[1].path = Some(_dir.path().join("valid.php"));
        for buffer in &mut ws.buffers {
            buffer.try_save().unwrap();
        }
        assert_eq!(
            after_successful_save(&mut ws, &PendingClose::Tab(0), &SaveReport::default()),
            CloseAction::Closed
        );
        assert_eq!(ws.buffers.len(), 1);
        assert_eq!(
            after_successful_save(
                &mut ws,
                &PendingClose::Project(PathBuf::from("/novo")),
                &SaveReport::default()
            ),
            CloseAction::LoadProject(PathBuf::from("/novo"))
        );
        assert_eq!(
            after_successful_save(&mut ws, &PendingClose::Exit, &SaveReport::default()),
            CloseAction::Exit
        );
    }

    #[test]
    fn close_others_does_not_require_saving_kept_tab() {
        let (_dir, mut ws) = workspace();
        ws.buffers[1].try_save().ok();
        ws.buffers[0].dirty = true;
        ws.buffers[1].dirty = false;
        let action = after_successful_save(&mut ws, &PendingClose::Others(0), &report(&[1], None));
        assert_eq!(action, CloseAction::Closed);
        assert_eq!(ws.buffers.len(), 1);
        assert!(ws.buffers[0].dirty);
    }

    #[test]
    fn cancel_dismisses_every_pending_close_flow() {
        for pending in [
            PendingClose::Tab(0),
            PendingClose::Others(0),
            PendingClose::Project(PathBuf::from("/outro")),
            PendingClose::Exit,
        ] {
            let mut state = Some(pending);
            cancel(&mut state);
            assert!(state.is_none());
        }
    }
}
