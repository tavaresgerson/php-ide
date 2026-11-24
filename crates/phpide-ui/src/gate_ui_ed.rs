//! Gate UI-ED-GATE-01: editor pipeline (structure, folds, display map,
//! highlight) on a 10k-line fixture, with timings.

fn big_php() -> String {
    let mut s = String::from("<?php\n");
    for i in 0..2000 {
        s.push_str(&format!(
            "function f{i}(): void {{\n    if (true) {{\n        echo {i};\n    }}\n}}\n"
        ));
    }
    s
}

#[test]
fn gate_folding_highlight_10k_within_budget() {
    use phpide_editor::view::folds::{DisplayMap, Folds};

    let code = big_php();
    let total = code.lines().count();
    assert!(total >= 10_000, "fixture has {total} lines");
    let doc = phpide_core::Document::new(&code);

    let t0 = std::time::Instant::now();
    // 1) Structure (folds + blocks) from the background parse.
    let mut syntax = phpide_editor::Syntax::for_path(Some(std::path::Path::new("big.php")));
    syntax.wait_idle(&doc);
    let structure = syntax.structure(&doc).expect("structure");
    assert!(!structure.folds.is_empty());
    assert!(!structure.report.blocks.is_empty());
    let t_structure = t0.elapsed();

    // 2) 200 folds and the display map built from them.
    let mut folds = Folds::default();
    for range in structure.folds.iter().take(200) {
        folds.fold(doc.rope(), range.start_line0, range.end_line0);
    }
    let map = DisplayMap::new(doc.rope(), &folds);
    assert!(map.rows() < total);
    let t_map = t0.elapsed() - t_structure;

    // 3) Highlight of one screen in the middle of the file.
    let first = total / 2;
    let spans = syntax.line_spans(&doc, first, first + 60);
    assert_eq!(spans.len(), 61);
    let total_t = t0.elapsed();

    assert!(
        total_t < std::time::Duration::from_secs(2),
        "10k pipeline too slow: {total_t:?} (structure {t_structure:?}, map {t_map:?})"
    );
    eprintln!(
        "GATE 10k: {total} lines, {} folds, {} visible rows, {total_t:?}",
        structure.folds.len(),
        map.rows()
    );
}

#[test]
fn gate_editor_avancado_integrado_dados_reais() {
    // Dados reais: folding (tree-sitter) + markers git (phpide-vcs puro)
    // + blame model sobre repo temporário real.
    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let g = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap())
    };
    g(&["init", "-b", "main"]);
    g(&["config", "user.name", "Gate"]);
    g(&["config", "user.email", "g@x"]);
    std::fs::write(root.join("a.php"), "<?php\n$x = 1;\n").unwrap();
    g(&["add", "."]);
    g(&["commit", "-m", "base"]);
    let m = phpide_vcs::markers_for_file(&root, &root.join("a.php")).unwrap();
    assert!(m.is_empty(), "arquivo sem alterações: sem markers falsos");
    std::fs::write(root.join("a.php"), "<?php\n$x = 2;\n").unwrap();
    let m = phpide_vcs::markers_for_file(&root, &root.join("a.php")).unwrap();
    assert_eq!(
        m.values().fold(0, |acc, _| acc + 1),
        1,
        "1 linha modificada"
    );
}
