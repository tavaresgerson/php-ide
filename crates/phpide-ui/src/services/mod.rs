//! Background services owned by the app. Each one runs its I/O in jobs and
//! exposes cached state that the UI reads every frame without blocking.

pub mod desktop;
pub mod file_ops;
pub mod files;
pub mod git;

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Non-test code of a source file (everything before its first test
    /// module).
    fn production_code(source: &str) -> &str {
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let trimmed = line.trim();
            if trimmed.starts_with("#[cfg(") && trimmed.contains("test") {
                let rest = &source[offset + line.len()..];
                if rest.trim_start().starts_with("mod ") {
                    return &source[..offset];
                }
            }
            offset += line.len();
        }
        source
    }

    fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Git commands and folder scans run in `services`, off the UI thread.
    /// This keeps them from creeping back into render code.
    #[test]
    fn ui_code_does_not_run_git_or_scan_folders_directly() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut offenders = Vec::new();
        for file in files {
            let name = file.file_name().unwrap().to_string_lossy();
            // Services own the I/O; gate_* and ui_tests are test-only modules.
            if file.starts_with(src.join("services"))
                || name.starts_with("gate_")
                || name == "ui_tests.rs"
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for line in production_code(&text).lines() {
                let line = line.trim();
                if line.starts_with("//") || line.starts_with("use phpide_vcs::") {
                    continue;
                }
                if line.contains("phpide_vcs::")
                    || line.contains("phpide_core::scan(")
                    || line.contains("refresh_files(")
                {
                    offenders.push(format!("{}: {line}", file.display()));
                }
            }
        }
        assert!(offenders.is_empty(), "{offenders:#?}");
    }
}
