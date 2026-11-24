//! Hand-offs to the desktop: the file manager and a terminal emulator
//! (RF-NAV-18). Processes are spawned detached; nothing waits for them.

use std::path::Path;
use std::process::{Command, Stdio};

fn spawn(program: &str, args: &[&std::ffi::OsStr]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Opens `dir` in the desktop file manager.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    for program in ["xdg-open", "gio"] {
        let ok = if program == "gio" {
            spawn(program, &["open".as_ref(), dir.as_os_str()])
        } else {
            spawn(program, &[dir.as_os_str()])
        };
        if ok {
            return Ok(());
        }
    }
    Err("No file manager found (xdg-open is missing)".into())
}

/// Opens a terminal emulator in `dir`: `$TERMINAL` first, then the usual
/// desktop terminals. Returns the program used.
pub fn open_terminal(dir: &Path) -> Result<String, String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(terminal) = std::env::var("TERMINAL") {
        if !terminal.trim().is_empty() {
            candidates.push(terminal);
        }
    }
    for name in [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "kgx",
        "xfce4-terminal",
        "tilix",
        "alacritty",
        "kitty",
        "wezterm",
        "foot",
        "xterm",
    ] {
        candidates.push(name.to_string());
    }
    for program in candidates {
        let ok = Command::new(&program)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
        if ok {
            return Ok(program);
        }
    }
    Err("No terminal emulator found; set $TERMINAL".into())
}

/// Result of a native folder chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    Folder(std::path::PathBuf),
    Cancelled,
    /// No chooser program on this desktop: use the built-in browser.
    Unavailable,
}

/// Asks the desktop for a folder (`zenity` on GNOME-like desktops,
/// `kdialog` on KDE). Blocks until the dialog closes: run it in a job.
pub fn pick_folder(title: &str, start: &Path) -> Picked {
    let start_slash = format!("{}/", start.display());
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                format!("--title={title}"),
                format!("--filename={start_slash}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--getexistingdirectory".into(),
                start.display().to_string(),
                "--title".into(),
                title.into(),
            ],
        ),
    ];
    for (program, args) in attempts {
        let output = Command::new(program)
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            continue; // not installed
        };
        if !output.status.success() {
            return Picked::Cancelled;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Picked::Cancelled;
        }
        return Picked::Folder(std::path::PathBuf::from(text));
    }
    Picked::Unavailable
}
