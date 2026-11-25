//! PHPantom on a real project: time until the index is ready, completion
//! latency and diagnostics latency after a keystroke (p50/p95).
//!
//! ```text
//! PHPIDE_MEASURE_ROOT=/path/to/laravel PHPIDE_MEASURE_FILE=app/Models/User.php \
//!   cargo test -p phpide-lsp --release --test measure_phpantom -- --ignored --nocapture
//! ```
//!
//! Without `PHPIDE_MEASURE_ROOT` it builds a synthetic project of 2,000
//! classes. The numbers go to `docs/ROADMAP.md` (Phase 11).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use phpide_lsp::engine::{resolve, EngineChoice};
use phpide_lsp::{InitOptions, LspClient, LspStatus};
use serde_json::{json, Value};

fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    let at = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples[at]
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

/// A project of `classes` classes in 20 namespaces, each calling the next.
fn synthetic(root: &Path, classes: usize) -> PathBuf {
    std::fs::write(
        root.join("composer.json"),
        r#"{ "autoload": { "psr-4": { "Synth\\": "src/" } } }"#,
    )
    .unwrap();
    for i in 0..classes {
        let ns = i % 20;
        let dir = root.join(format!("src/N{ns}"));
        std::fs::create_dir_all(&dir).unwrap();
        let next = (i + 1) % classes;
        let text = format!(
            "<?php\n\nnamespace Synth\\N{ns};\n\nuse Synth\\N{nns}\\C{next};\n\n/** Class number {i}. */\nclass C{i}\n{{\n    public int $value = {i};\n\n    public function next(): C{next}\n    {{\n        return new C{next}();\n    }}\n\n    public function label(string $prefix): string\n    {{\n        return $prefix . $this->value;\n    }}\n}}\n",
            nns = next % 20,
        );
        std::fs::write(dir.join(format!("C{i}.php")), text).unwrap();
    }
    let probe = root.join("src/Probe.php");
    std::fs::write(
        &probe,
        "<?php\n\nnamespace Synth;\n\nuse Synth\\N0\\C0;\n\nfunction probe(C0 $c): string\n{\n    return $c->label('x');\n}\n",
    )
    .unwrap();
    probe
}

fn wait<T>(timeout: Duration, mut poll: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(v) = poll() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
#[ignore = "measurement: run on demand"]
fn measure_phpantom() {
    let scratch = tempfile::tempdir().unwrap();
    let (root, file) = match std::env::var_os("PHPIDE_MEASURE_ROOT") {
        Some(root) => {
            let root = PathBuf::from(root);
            let file = root
                .join(std::env::var("PHPIDE_MEASURE_FILE").unwrap_or_else(|_| "index.php".into()));
            (root, file)
        }
        None => {
            let root = scratch.path().to_path_buf();
            let file = synthetic(&root, 2000);
            (root, file)
        }
    };
    let server = resolve(&EngineChoice::Phpantom, &root).expect("PHPantom");
    let php_files = walk(&root);
    eprintln!("project: {} ({} PHP files)", root.display(), php_files);

    let started = Instant::now();
    let args: Vec<&str> = server.args.iter().map(String::as_str).collect();
    let (lsp, init) = LspClient::spawn_with(
        &server.program.display().to_string(),
        &args,
        &root,
        &InitOptions::default(),
    )
    .unwrap();
    let result = init.recv_timeout(Duration::from_secs(30)).unwrap();
    lsp.register_capabilities(&result);
    lsp.initialized();
    *lsp.shared.status.lock().unwrap() = LspStatus::Running("PHPantom".into());

    // Ready: every piece of indexing work has ended.
    let mut ended = Vec::new();
    wait(Duration::from_secs(600), || {
        ended.extend(lsp.take_finished());
        let full = ended
            .iter()
            .any(|(title, _)| title.to_lowercase().contains("full index"));
        (full && lsp.progress().is_empty()).then_some(())
    })
    .expect("indexing ends");
    let ready = started.elapsed();
    eprintln!("indexing: {ended:?}");

    let text = std::fs::read_to_string(&file).unwrap();
    lsp.did_open(&file, &text, 1);
    // Completion after `->` on the last `->` of the file.
    let at = text.rfind("->").expect("a `->` in the file") + 2;
    let line = text[..at].matches('\n').count();
    let col = at - text[..at].rfind('\n').map_or(0, |n| n + 1);
    let mut completion = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let answer = lsp
            .completion(&file, line, col)
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        completion.push(t.elapsed());
        assert!(!phpide_lsp::parse_completions(&answer).is_empty());
    }

    // Diagnostics: a syntax error typed and removed, timed until the
    // matching publication.
    let mut diagnostics = Vec::new();
    for version in 2..22u64 {
        let broken = version % 2 == 0;
        let edited = if broken {
            format!("{text}\nfunction oops( {{\n")
        } else {
            text.clone()
        };
        lsp.shared.diagnostics.lock().unwrap().remove(&file);
        let t = Instant::now();
        lsp.did_change(&file, &edited, version);
        wait(Duration::from_secs(30), || {
            let map = lsp.shared.diagnostics.lock().unwrap();
            let errors = map
                .get(&file)?
                .iter()
                .any(|d| d.raw["code"] == "syntax_error");
            (errors == broken).then_some(())
        })
        .expect("diagnostics after the edit");
        diagnostics.push(t.elapsed());
    }
    lsp.stop();

    let report = json!({
        "files": php_files,
        "ready": ms(ready),
        "completion_p50": ms(percentile(&mut completion, 0.5)),
        "completion_p95": ms(percentile(&mut completion, 0.95)),
        "diagnostics_p50": ms(percentile(&mut diagnostics, 0.5)),
        "diagnostics_p95": ms(percentile(&mut diagnostics, 0.95)),
    });
    eprintln!("MEASURE {report}");
    let _: Value = report;
}

fn walk(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                walk(&path)
            } else {
                usize::from(path.extension().is_some_and(|x| x == "php"))
            }
        })
        .sum()
}
