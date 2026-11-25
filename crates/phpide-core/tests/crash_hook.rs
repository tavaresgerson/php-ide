//! The crash hook writes a report when a thread panics (its own test
//! binary: the hook is process-wide).

use phpide_core::diag::{install_crash_hook, unseen_crash_reports, BuildInfo, RecentLines};

#[test]
fn a_panic_leaves_a_report_with_the_message_and_the_last_log_lines() {
    let dir = std::env::temp_dir().join(format!("phpide-crash-hook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let recent = RecentLines::default();
    recent.push("1700000000 INFO ui sess=1 app_start");
    install_crash_hook(
        dir.clone(),
        BuildInfo {
            name: "PHP IDE",
            version: "9.9.9",
        },
        recent,
    );
    let result = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("index out of bounds in the test"))
        .unwrap()
        .join();
    assert!(result.is_err());
    let reports = unseen_crash_reports(&dir);
    assert_eq!(reports.len(), 1);
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    assert!(text.contains("PHP IDE 9.9.9 crashed"), "{text}");
    assert!(text.contains("Panic: index out of bounds in the test"));
    assert!(text.contains("Thread: worker"));
    assert!(text.contains("crash_hook.rs"));
    assert!(text.contains("app_start"));
    let _ = std::fs::remove_dir_all(&dir);
}
