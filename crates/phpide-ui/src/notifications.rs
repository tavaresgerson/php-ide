//! Event Log and toast notifications (RF-UI-05).
//!
//! Every status message the workspace publishes (`ws.message`) is recorded in
//! the Event Log, shown by the Notifications tool window. Errors and warnings
//! also pop up as a toast in the bottom-right corner, so a failure is never
//! hidden in the status bar only.

use crate::design::components::{Toast, Toasts, Tone};

/// Entries kept in the log; older ones are dropped.
const CAPACITY: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Local wall-clock time, `HH:MM:SS`.
    pub time: String,
    pub tone: Tone,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct EventLog {
    entries: Vec<Event>,
    last_message: String,
    /// Errors and warnings logged since the Notifications window was last seen.
    unread: usize,
}

impl EventLog {
    /// Records `message` when it differs from the last one seen; errors and
    /// warnings also become a toast.
    pub fn observe(&mut self, message: &str, toasts: &mut Toasts) {
        if message == self.last_message {
            return;
        }
        self.last_message = message.to_string();
        let text = message.trim();
        if text.is_empty() {
            return;
        }
        let tone = classify(text);
        self.push(Event {
            time: local_time(),
            tone,
            text: text.to_string(),
        });
        if matches!(tone, Tone::Error | Tone::Warning) {
            self.unread += 1;
            let title = if tone == Tone::Error {
                "Error"
            } else {
                "Warning"
            };
            toasts.push(Toast::new(tone, title).body(text));
        }
    }

    fn push(&mut self, event: Event) {
        self.entries.push(event);
        if self.entries.len() > CAPACITY {
            self.entries.remove(0);
        }
    }

    /// Newest first.
    pub fn entries(&self) -> impl Iterator<Item = &Event> {
        self.entries.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn unread(&self) -> usize {
        self.unread
    }

    pub fn mark_read(&mut self) {
        self.unread = 0;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.unread = 0;
    }
}

/// Infers the severity of a status message from its wording.
pub fn classify(text: &str) -> Tone {
    let lower = text.to_lowercase();
    const ERRORS: [&str; 7] = [
        "error",
        "failed",
        "could not",
        "couldn't",
        "cannot",
        "can't",
        "not found",
    ];
    const WARNINGS: [&str; 3] = ["warning", "unsaved", "outside the project"];
    if ERRORS.iter().any(|w| lower.contains(w)) {
        Tone::Error
    } else if WARNINGS.iter().any(|w| lower.contains(w)) {
        Tone::Warning
    } else {
        Tone::Info
    }
}

fn local_time() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    // SAFETY: `localtime_r` writes into the `tm` we own; both pointers are valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&now, &mut tm).is_null() };
    if ok {
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    } else {
        let secs = now.rem_euclid(86_400);
        format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_logged_once_and_failures_become_toasts() {
        let mut log = EventLog::default();
        let mut toasts = Toasts::default();
        log.observe("Saved index.php", &mut toasts);
        log.observe("Saved index.php", &mut toasts);
        assert_eq!(log.len(), 1);
        assert!(toasts.is_empty());

        log.observe("Could not rename: permission denied", &mut toasts);
        assert_eq!(log.len(), 2);
        assert_eq!(toasts.len(), 1);
        assert_eq!(log.unread(), 1);
        let newest = log.entries().next().unwrap();
        assert_eq!(newest.tone, Tone::Error);
        assert_eq!(newest.time.len(), 8);

        log.observe("", &mut toasts);
        assert_eq!(log.len(), 2);
        log.mark_read();
        assert_eq!(log.unread(), 0);
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn classification_follows_wording() {
        assert_eq!(classify("PHP run failed (exit 255)"), Tone::Error);
        assert_eq!(classify("2 files have unsaved changes"), Tone::Warning);
        assert_eq!(classify("Indexed 120 files"), Tone::Info);
    }
}
