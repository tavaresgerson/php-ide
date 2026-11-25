//! Diagnostics that stay on the machine: a rotating log file, the last log
//! lines kept in memory, and a crash report written when the IDE panics.
//!
//! Nothing is sent anywhere. Log lines are already redacted by the
//! [`crate::Logger`]; the crash report repeats only those lines.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::log::LogSink;

/// Where the IDE keeps state that is not configuration: logs, crash
/// reports, unsaved buffers. `PHPIDE_STATE_DIR` overrides it (tests);
/// otherwise `$XDG_STATE_HOME/phpide` or `~/.local/state/phpide`.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PHPIDE_STATE_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("phpide");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".local/state/phpide")
}

/// The log folder.
pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

/// The log file being written now.
pub fn log_file() -> PathBuf {
    log_dir().join("phpide.log")
}

/// The last lines logged, for crash reports.
#[derive(Clone, Default)]
pub struct RecentLines(Arc<Mutex<VecDeque<String>>>);

impl RecentLines {
    const KEEP: usize = 200;

    pub fn push(&self, line: &str) {
        if let Ok(mut lines) = self.0.lock() {
            if lines.len() == Self::KEEP {
                lines.pop_front();
            }
            lines.push_back(line.to_string());
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// A log file that rotates at `max_bytes`: `phpide.log` becomes
/// `phpide.1.log`, and so on up to `keep` old files.
pub struct RotatingFileSink {
    dir: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: Mutex<Option<(File, u64)>>,
    recent: RecentLines,
    /// Also print to stderr (development).
    echo: bool,
}

impl RotatingFileSink {
    pub fn new(dir: &Path, max_bytes: u64, keep: usize, recent: RecentLines) -> Self {
        let _ = fs::create_dir_all(dir);
        let sink = RotatingFileSink {
            dir: dir.to_path_buf(),
            max_bytes,
            keep,
            file: Mutex::new(None),
            recent,
            echo: std::env::var_os("PHPIDE_LOG_STDERR").is_some(),
        };
        sink.reopen();
        sink
    }

    fn path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join("phpide.log")
        } else {
            self.dir.join(format!("phpide.{index}.log"))
        }
    }

    fn reopen(&self) {
        let path = self.path(0);
        let opened = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
            .map(|f| {
                let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                (f, size)
            });
        if let Ok(mut slot) = self.file.lock() {
            *slot = opened;
        }
    }

    fn rotate(&self) {
        for index in (0..self.keep).rev() {
            let from = self.path(index);
            if from.exists() {
                let _ = fs::rename(&from, self.path(index + 1));
            }
        }
        let _ = fs::remove_file(self.path(self.keep + 1));
        self.reopen();
    }
}

impl LogSink for RotatingFileSink {
    fn write_line(&self, line: &str) {
        self.recent.push(line);
        if self.echo {
            eprintln!("{line}");
        }
        let full = {
            let Ok(mut slot) = self.file.lock() else {
                return;
            };
            let Some((file, size)) = slot.as_mut() else {
                return;
            };
            let _ = writeln!(file, "{line}");
            *size += line.len() as u64 + 1;
            *size >= self.max_bytes
        };
        if full {
            self.rotate();
        }
    }
}

/// What a crash report says about the build and the system.
#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub name: &'static str,
    pub version: &'static str,
}

static CRASH_HOOK: OnceLock<()> = OnceLock::new();

/// Writes a crash report into `dir` when any thread panics, then lets the
/// previous hook run. Installed once per process.
pub fn install_crash_hook(dir: PathBuf, build: BuildInfo, recent: RecentLines) {
    if CRASH_HOOK.set(()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let report = crash_report(&build, &message, &location, &backtrace, &recent.lines());
        let _ = write_crash_report(&dir, &report);
        previous(info);
    }));
}

/// The text of a crash report.
pub fn crash_report(
    build: &BuildInfo,
    message: &str,
    location: &str,
    backtrace: &str,
    recent: &[String],
) -> String {
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let mut text = String::new();
    text.push_str(&format!("{} {} crashed\n\n", build.name, build.version));
    text.push_str(&format!("Time (unix): {}\n", now_secs()));
    text.push_str(&format!(
        "System: {} {} ({})\n",
        std::env::consts::OS,
        os_release().unwrap_or_default(),
        std::env::consts::ARCH
    ));
    text.push_str(&format!("Thread: {thread}\n"));
    text.push_str(&format!("Panic: {message}\n"));
    if !location.is_empty() {
        text.push_str(&format!("At: {location}\n"));
    }
    text.push_str("\nBacktrace:\n");
    text.push_str(backtrace);
    text.push_str("\n\nLast log lines:\n");
    for line in recent {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(
        "\nThis report stays on your computer. Attach it to an issue if you want to report the crash.\n",
    );
    text
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `PRETTY_NAME` from `/etc/os-release`.
fn os_release() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

/// Saves a report as `crash-<unix seconds>.txt`; keeps the newest ten.
pub fn write_crash_report(dir: &Path, report: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("crash-{}.txt", now_secs()));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("crash-{}-{n}.txt", now_secs()));
        n += 1;
    }
    fs::write(&path, report)?;
    let mut all = reports(dir);
    all.sort();
    while all.len() > 10 {
        let _ = fs::remove_file(all.remove(0));
    }
    Ok(path)
}

fn reports(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reports the user has not seen yet (newest first).
pub fn unseen_crash_reports(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = reports(dir)
        .into_iter()
        .filter(|p| !p.to_string_lossy().ends_with(".seen.txt"))
        .collect();
    list.sort();
    list.reverse();
    list
}

/// Marks a report as seen (renamed to `*.seen.txt`); returns the new path.
pub fn mark_seen(report: &Path) -> PathBuf {
    let name = report
        .file_name()
        .map(|n| n.to_string_lossy().replace(".txt", ".seen.txt"))
        .unwrap_or_default();
    let seen = report.with_file_name(name);
    if fs::rename(report, &seen).is_ok() {
        seen
    } else {
        report.to_path_buf()
    }
}

/// The crash report folder.
pub fn crash_dir() -> PathBuf {
    state_dir().join("crashes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_rotates_and_keeps_recent_lines() {
        let dir = tempfile_dir();
        let recent = RecentLines::default();
        let sink = RotatingFileSink::new(&dir, 100, 2, recent.clone());
        for i in 0..30 {
            sink.write_line(&format!("line number {i:02}"));
        }
        assert!(dir.join("phpide.log").exists());
        assert!(dir.join("phpide.1.log").exists());
        assert!(dir.join("phpide.2.log").exists());
        assert!(!dir.join("phpide.3.log").exists());
        let current = fs::read_to_string(dir.join("phpide.log")).unwrap();
        assert!(current.len() <= 100, "{current}");
        assert_eq!(recent.lines().last().unwrap(), "line number 29");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_reports_are_written_listed_and_marked_seen() {
        let dir = tempfile_dir();
        let build = BuildInfo {
            name: "PHP IDE",
            version: "1.0.0",
        };
        let report = crash_report(
            &build,
            "boom",
            "src/x.rs:1:2",
            "frame 0",
            &["a line".into()],
        );
        assert!(report.contains("PHP IDE 1.0.0 crashed"));
        assert!(report.contains("Panic: boom") && report.contains("a line"));
        let path = write_crash_report(&dir, &report).unwrap();
        assert_eq!(unseen_crash_reports(&dir), std::slice::from_ref(&path));
        let seen = mark_seen(&path);
        assert!(seen.exists());
        assert!(unseen_crash_reports(&dir).is_empty());
        for _ in 0..12 {
            write_crash_report(&dir, "x").unwrap();
        }
        assert!(reports(&dir).len() <= 10);
        let _ = fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("phpide-diag-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
