// Gate do recorte UI-ED-000 e verificação do modo offline.
use crate::DIRTY_BUTTON_RECTS;

/// Modo offline (sem LSP): highlight sintático e diagnósticos estruturais
/// locais continuam funcionais — são a degradação prevista pelo NFR
/// (tree-sitter cobre estrutura e erros de sintaxe offline).
#[test]
fn offline_highlight_and_structural_diagnostics_still_work() {
    // Highlight: código PHP produz fragments com formatação colorida.
    let style = crate::design::Theme::default().syntax_style();
    let job = phpide_php::highlight("<?php\n$nome = 'phpide';\n", &style);
    assert!(!job.sections.is_empty(), "highlight gera seções");
    let colored = job
        .sections
        .iter()
        .any(|s| s.format.color != eframe::egui::Color32::default());
    assert!(colored, "alguma seção recebe cor do syntect");

    // Diagnósticos estruturais: código inválido gera erros do tree-sitter.
    let ok = phpide_syntax::analyze("<?php class Ok { public function a(): int { return 1; } }");
    assert!(
        ok.errors.is_empty(),
        "código válido sem erros: {:?}",
        ok.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let broken = phpide_syntax::analyze("<?php class Quebrado { public function a( {");
    assert!(
        !broken.errors.is_empty(),
        "código inválido produz diagnóstico estrutural offline"
    );
    // Decisão do poll_syntax replicada com as peças reais: sem LSP, os
    // erros locais alimentam o painel de problemas do arquivo.
    let lsp_running = false;
    let diags: Vec<phpide_core::Diagnostic> = if lsp_running {
        Vec::new()
    } else {
        broken
            .errors
            .iter()
            .map(|e| phpide_core::Diagnostic {
                path: std::path::PathBuf::from("/p/offline.php"),
                line: e.line0 + 1,
                col: e.col0 + 1,
                message: e.message.clone(),
                severity: phpide_core::Severity::Error,
                source: "tree-sitter".to_string(),
            })
            .collect()
    };
    assert_eq!(diags.len(), broken.errors.len());
    assert!(diags.iter().all(|d| d.source == "tree-sitter"));
}

/// Zoom: atalhos e limites consolidados (UI-ED-010-01).
#[test]
fn zoom_shortcut_values_are_clamped() {
    // Mesma função usada por set_zoom e pela carga do settings.
    assert_eq!(crate::settings::clamp_zoom(1.0 * 1.1f32.powi(20)), 3.0);
    assert_eq!(crate::settings::clamp_zoom(1.0 / 1.1f32.powi(20)), 0.5);
    // Roundtrip: persistir e recarregar preserva o valor dentro dos limites.
    let s = crate::settings::Settings {
        zoom: crate::settings::clamp_zoom(1.7),
        ..Default::default()
    };
    let v = serde_json::json!({ "zoom": s.zoom });
    let back = v["zoom"].as_f64().unwrap() as f32;
    assert_eq!(crate::settings::clamp_zoom(back), 1.7);
}

fn click_pending_close_button(
    action: &str,
) -> (crate::PhpIdeApp, tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let file = root.join("b.php");
    std::fs::write(&file, "<?php\n").unwrap();
    let other = root.join("other");
    std::fs::create_dir(&other).unwrap();
    let mut app = crate::PhpIdeApp::new(root.clone(), false, vec![file.clone()]);
    app.zoom = 1.0;
    app.ws
        .active_buffer_mut()
        .unwrap()
        .insert(0, "// alteração\n");
    app.pending_close = Some(crate::PendingClose::Project(other));
    let mut frame = eframe::Frame::_new_kittest();
    let context = eframe::egui::Context::default();
    crate::design::install(&context, crate::design::Theme::default());
    let input = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    context
        .run_ui(input, |ui| eframe::App::ui(&mut app, ui, &mut frame))
        .drop_without_applying_deltas();
    context
        .run_ui(eframe::egui::RawInput::default(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame)
        })
        .drop_without_applying_deltas();
    let index = match action {
        "save" => 0,
        "discard" => 1,
        "cancel" => 2,
        _ => unreachable!(),
    };
    let rect = DIRTY_BUTTON_RECTS.with(|rects| rects.get().expect("botões renderizados")[index]);
    let (x, y) = (rect.center().x, rect.center().y);
    let press = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(1280.0, 800.0),
        )),
        events: vec![
            eframe::egui::Event::PointerMoved(eframe::egui::pos2(x, y)),
            eframe::egui::Event::PointerButton {
                pos: eframe::egui::pos2(x, y),
                button: eframe::egui::PointerButton::Primary,
                pressed: true,
                modifiers: eframe::egui::Modifiers::default(),
            },
        ],
        ..Default::default()
    };
    context
        .run_ui(press, |ui| eframe::App::ui(&mut app, ui, &mut frame))
        .drop_without_applying_deltas();
    let release = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(1280.0, 800.0),
        )),
        events: vec![eframe::egui::Event::PointerButton {
            pos: eframe::egui::pos2(x, y),
            button: eframe::egui::PointerButton::Primary,
            pressed: false,
            modifiers: eframe::egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    context
        .run_ui(release, |ui| eframe::App::ui(&mut app, ui, &mut frame))
        .drop_without_applying_deltas();
    (app, dir, file)
}

#[test]
fn project_close_buttons_are_clickable_and_cancel_keeps_current_project() {
    let (app, _, _) = click_pending_close_button("cancel");
    assert!(app.pending_close.is_none(), "Cancel was not routed");
    assert!(!app.ws.root.ends_with("other"));
}

#[test]
fn project_close_discard_switches_project_without_saving() {
    let (app, _, _) = click_pending_close_button("discard");
    assert!(app.pending_close.is_none(), "Don't Save was not routed");
    assert!(app.ws.root.ends_with("other"));
}

#[test]
fn project_close_save_persists_and_switches_project() {
    let (app, _dir, file) = click_pending_close_button("save");
    assert!(app.pending_close.is_none(), "Save was not routed");
    assert!(app.ws.root.ends_with("other"));
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains("// alteração"));
    assert!(app.ws.buffers.iter().all(|buffer| !buffer.dirty));
}
