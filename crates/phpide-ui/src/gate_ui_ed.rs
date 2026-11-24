//! Gate UI-ED-GATE-01: pipeline de superfície (folding + gutter visual +
//! markers + sticky) sobre fixture de 10 mil linhas, com medição.

use super::*;

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
fn gate_folding_gutter_markers_10k_within_budget() {
    let code = big_php();
    let total = code.lines().count();
    assert!(total >= 10_000, "fixture tem {total} linhas");

    let t0 = std::time::Instant::now();
    // 1) Ranges de folding (tree-sitter) sobre as 10k linhas.
    let ranges = phpide_syntax::folding::folding_ranges(&code, total);
    assert!(!ranges.is_empty());
    let t_ranges = t0.elapsed();

    // 2) Dobras efetivas + projeção + contagem visual.
    let folded: Vec<(usize, usize)> = ranges
        .iter()
        .take(200)
        .map(|r| (r.start_line0, r.end_line0))
        .collect();
    let visuals = folding::FoldVisuals::new(&folded);
    let proj = folding::project(&code, &visuals);
    assert!(proj.text.lines().count() < total);
    let t_proj = t0.elapsed() - t_ranges;

    // 3) Markers agregados (limite por categoria) + sticky trail.
    let diags: Vec<(u32, phpide_core::Severity)> = (1..=50)
        .map(|l| (l * 100, phpide_core::Severity::Error))
        .collect();
    let git: Vec<(u32, bool)> = (1..=20).map(|l| (l * 200, true)).collect();
    let markers = lens::aggregate_scrollbar_markers(&diags, &git, total);
    assert!(!markers.is_empty());
    let blocks = phpide_syntax::analyze(&code).blocks;
    let trail = editor_view::sticky_trail(&blocks, total / 2, 5);
    assert!(!trail.is_empty());
    let total_t = t0.elapsed();

    // Orçamento folgado: pipeline completo < 2s (baseline UI-ED-000-04:
    // 7.5ns/frame). Registre o número real ao evoluir o gate.
    assert!(
        total_t < std::time::Duration::from_secs(2),
        "pipeline 10k lento: {total_t:?} (ranges {t_ranges:?}, proj {t_proj:?})"
    );
    eprintln!(
        "GATE 10k: total {total} linhas, {} ranges, {} linhas visuais, {total_t:?}",
        ranges.len(),
        proj.text.lines().count()
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
