//! Quick preview of a file from the Project view (Space, RF-NAV-05): the
//! first lines, highlighted like the editor, without opening a tab.

use std::path::PathBuf;

use eframe::egui;
use phpide_core::jobs::{JobHandle, JobSystem};
use phpide_core::Document;
use phpide_editor::Syntax;

use crate::PhpIdeApp;

/// Lines shown at most.
const MAX_LINES: usize = 400;
/// Bigger files show their beginning.
const MAX_BYTES: usize = 512 * 1024;

pub(crate) struct Preview {
    pub path: PathBuf,
    job: Option<JobHandle<Option<String>>>,
    /// `None` while loading; `Some(None)` when it cannot be shown.
    text: Option<Option<String>>,
    doc: Option<(Document, Syntax)>,
}

impl Preview {
    pub(crate) fn load(jobs: &JobSystem, ctx: &egui::Context, path: PathBuf) -> Self {
        let file = path.clone();
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let text = std::fs::read(&file).ok().and_then(|mut bytes| {
                bytes.truncate(MAX_BYTES);
                if bytes.contains(&0) {
                    return None;
                }
                let text = String::from_utf8_lossy(&bytes).replace('\t', "    ");
                Some(text.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n"))
            });
            ctx.request_repaint();
            Ok(text)
        });
        Preview {
            path,
            job: Some(job),
            text: None,
            doc: None,
        }
    }

    fn poll(&mut self) {
        if let Some(job) = &mut self.job {
            if let Some(outcome) = job.poll() {
                let text = match outcome {
                    phpide_core::jobs::JobOutcome::Ok(text) => text,
                    _ => None,
                };
                self.doc = text
                    .as_deref()
                    .map(|t| (Document::new(t), Syntax::for_path(Some(&self.path))));
                self.text = Some(text);
                self.job = None;
            }
        }
    }
}

/// The preview popup, while one is open.
pub(crate) fn show(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some(preview) = app.tree.preview.as_mut() else {
        return;
    };
    preview.poll();
    let theme = crate::design::current(ctx);
    let p = theme.palette;
    let style = theme.editor_style();
    let language = preview
        .doc
        .as_ref()
        .map(|(_, syntax)| crate::app_editor::language_id(syntax.file_type()));
    let syntax_style = theme.syntax_style_for(language.as_deref());
    let name = preview
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut close = false;
    let mut open = false;
    let area = ctx.content_rect();
    let pos = egui::pos2(area.left() + 320.0, area.top() + 90.0);
    let size = egui::vec2(
        (area.width() - 360.0).clamp(320.0, 720.0),
        (area.height() - 180.0).clamp(200.0, 480.0),
    );
    egui::Area::new(egui::Id::new("quick-preview"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(size.x);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&name).strong());
                    ui.label(
                        egui::RichText::new("Enter opens · Esc or Space closes")
                            .small()
                            .color(p.text_muted),
                    );
                });
                ui.separator();
                match (&preview.text, preview.doc.as_mut()) {
                    (None, _) => {
                        ui.spinner();
                    }
                    (Some(None), _) | (_, None) => {
                        ui.label(
                            egui::RichText::new("No preview for this file").color(p.text_muted),
                        );
                    }
                    (Some(Some(_)), Some((doc, syntax))) => {
                        let lines = doc.rope().len_lines();
                        let row = (syntax_style.font.size * style.line_spacing).round();
                        egui::ScrollArea::vertical()
                            .id_salt("quick-preview-scroll")
                            .max_height(size.y)
                            .auto_shrink([false, true])
                            .show_rows(ui, row, lines, |ui, range| {
                                let spans = syntax.line_spans(
                                    doc,
                                    range.start,
                                    range.end.saturating_sub(1),
                                );
                                for (k, line) in range.clone().enumerate() {
                                    let text: String = doc
                                        .rope()
                                        .line(line)
                                        .to_string()
                                        .trim_end_matches(['\n', '\r'])
                                        .to_string();
                                    let job = crate::diff_view::line_job(
                                        &text,
                                        spans.get(k).cloned(),
                                        &syntax_style,
                                        style.colors.text,
                                        None,
                                        None,
                                    );
                                    ui.horizontal(|ui| {
                                        ui.add_sized(
                                            [36.0, row],
                                            egui::Label::new(
                                                egui::RichText::new((line + 1).to_string())
                                                    .font(style.gutter_font.clone())
                                                    .color(style.colors.gutter_text),
                                            ),
                                        );
                                        ui.label(job);
                                    });
                                }
                            });
                    }
                }
            });
        });
    let (esc, space, enter) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(t) if t == " ")),
            i.key_pressed(egui::Key::Enter),
        )
    });
    // Space is handled by the tree (it toggles); Esc and Enter here.
    let _ = space;
    if esc {
        close = true;
    }
    if enter {
        open = true;
    }
    if open {
        let path = preview.path.clone();
        app.tree.preview = None;
        app.open_file_tab(&path, false);
    } else if close {
        app.tree.preview = None;
    }
}
