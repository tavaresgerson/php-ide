//! Undo history with grouping.
//!
//! Each entry stores the change and its inverse. Consecutive edits of the
//! same kind (typing, deleting) merge into one undo step while they follow
//! each other closely in time and the caret did not move in between; any
//! other edit, a pause, a caret move or an explicit [`History::break_group`]
//! starts a new step. Undo and redo apply inverse change sets, so the
//! document revision only grows.

use std::time::{Duration, Instant};

use crate::text::ChangeSet;

/// Carets as `(anchor, head)` char offsets, restored by undo/redo.
pub type Carets = Vec<(usize, usize)>;

/// How an edit merges with the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Typed text: merges with the previous typing step.
    Typing,
    /// Backspace/Delete: merges with the previous deletion step.
    Deletion,
    /// Paste, line operations, refactorings, external edits: always its own step.
    Other,
}

/// Pause after which typing starts a new undo step.
pub const MERGE_WINDOW: Duration = Duration::from_millis(1500);
const CAP: usize = 1000;

#[derive(Debug, Clone)]
struct Step {
    forward: Vec<ChangeSet>,
    inverse: Vec<ChangeSet>,
    kind: EditKind,
    before: Carets,
    after: Carets,
    last: Instant,
    /// Unique id, used to know whether the text matches the saved file.
    id: u64,
}

/// What undo or redo must apply, in order, and the carets to restore.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub changes: Vec<ChangeSet>,
    pub carets: Carets,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The last step may still absorb edits.
    open: bool,
    next_id: u64,
}

impl History {
    pub fn new() -> Self {
        History {
            next_id: 1,
            ..Default::default()
        }
    }

    /// Records an applied edit. `ends_word` closes the step after this edit
    /// (typing a newline or punctuation ends a word).
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        forward: ChangeSet,
        inverse: ChangeSet,
        kind: EditKind,
        before: Carets,
        after: Carets,
        now: Instant,
        ends_word: bool,
    ) {
        self.redo.clear();
        let merge = self.open
            && kind != EditKind::Other
            && self.undo.last().is_some_and(|last| {
                last.kind == kind
                    && now.saturating_duration_since(last.last) < MERGE_WINDOW
                    && (before.is_empty() || last.after == before)
            });
        if merge {
            let last = self.undo.last_mut().expect("checked");
            last.forward.push(forward);
            last.inverse.push(inverse);
            last.after = after;
            last.last = now;
        } else {
            let id = self.next_id.max(1);
            self.next_id = id + 1;
            self.undo.push(Step {
                forward: vec![forward],
                inverse: vec![inverse],
                kind,
                before,
                after,
                last: now,
                id,
            });
            if self.undo.len() > CAP {
                self.undo.remove(0);
            }
        }
        self.open = kind != EditKind::Other && !ends_word;
    }

    /// The next edit starts a new undo step.
    pub fn break_group(&mut self) {
        self.open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Change sets to apply (in order) to undo the last step. The caller
    /// applies them and keeps the returned carets.
    pub fn undo(&mut self) -> Option<Replay> {
        let step = self.undo.pop()?;
        self.open = false;
        let replay = Replay {
            changes: step.inverse.iter().rev().cloned().collect(),
            carets: step.before.clone(),
        };
        self.redo.push(step);
        Some(replay)
    }

    pub fn redo(&mut self) -> Option<Replay> {
        let step = self.redo.pop()?;
        self.open = false;
        let replay = Replay {
            changes: step.forward.clone(),
            carets: step.after.clone(),
        };
        self.undo.push(step);
        Some(replay)
    }

    /// Identifies the current text state: 0 when nothing was edited.
    pub fn state_id(&self) -> u64 {
        self.undo.last().map(|s| s.id).unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = false;
    }

    /// Number of undo steps (tests and diagnostics).
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::ChangeSet;

    fn typed(h: &mut History, at: usize, now: Instant) {
        h.record(
            ChangeSet::single(at, at, "x"),
            ChangeSet::single(at, at + 1, ""),
            EditKind::Typing,
            vec![(at, at)],
            vec![(at + 1, at + 1)],
            now,
            false,
        );
    }

    #[test]
    fn typing_merges_until_pause_or_caret_move() {
        let mut h = History::new();
        let t = Instant::now();
        typed(&mut h, 0, t);
        typed(&mut h, 1, t + Duration::from_millis(100));
        typed(&mut h, 2, t + Duration::from_millis(200));
        assert_eq!(h.undo_len(), 1);
        // Caret moved: new step.
        typed(&mut h, 10, t + Duration::from_millis(300));
        assert_eq!(h.undo_len(), 2);
        // Pause: new step.
        typed(&mut h, 11, t + Duration::from_secs(5));
        assert_eq!(h.undo_len(), 3);
    }

    #[test]
    fn other_edits_and_breaks_never_merge() {
        let mut h = History::new();
        let t = Instant::now();
        typed(&mut h, 0, t);
        h.break_group();
        typed(&mut h, 1, t);
        h.record(
            ChangeSet::single(0, 0, "y"),
            ChangeSet::single(0, 1, ""),
            EditKind::Other,
            vec![],
            vec![],
            t,
            false,
        );
        assert_eq!(h.undo_len(), 3);
    }

    #[test]
    fn undo_returns_inverses_in_reverse_and_redo_forwards() {
        let mut h = History::new();
        let t = Instant::now();
        typed(&mut h, 0, t);
        typed(&mut h, 1, t);
        let undo = h.undo().unwrap();
        assert_eq!(undo.changes.len(), 2);
        assert_eq!(undo.changes[0], ChangeSet::single(1, 2, ""));
        assert_eq!(undo.carets, vec![(0, 0)]);
        assert!(h.can_redo());
        let redo = h.redo().unwrap();
        assert_eq!(redo.changes[0], ChangeSet::single(0, 0, "x"));
        assert_eq!(redo.carets, vec![(2, 2)]);
        assert_ne!(h.state_id(), 0);
    }
}
