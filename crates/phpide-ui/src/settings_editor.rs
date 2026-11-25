//! Settings › Editor › Color Scheme and Font (RF-HL-10, RF-HL-11, RF-UI-05).

use eframe::egui;
use phpide_core::jobs::{JobHandle, JobOutcome};
use phpide_core::Buffer;
use phpide_editor::{Editor, EditorOptions, EditorView, Syntax};
use phpide_php::filetype::{self, Detected};
use phpide_php::SyntaxRole;

use crate::color_scheme::{self, Effect, Element, SchemeChanges, DEFAULTS, ROLES};
use crate::services::system_fonts::SystemFont;
use crate::PhpIdeApp;

/// Code shown under the element editor, for one language.
struct Preview {
    language: String,
    buffer: Buffer,
    view: EditorView,
    syntax: Syntax,
}

pub struct EditorPagesState {
    /// `DEFAULTS` or a file type id.
    scope: String,
    role: SyntaxRole,
    preview: Option<Preview>,
    fonts: Option<Vec<SystemFont>>,
    fonts_job: Option<JobHandle<Vec<SystemFont>>>,
    /// Duplicate, Rename, Export or Import in progress, with its field.
    scheme_op: Option<SchemeOp>,
    scheme_input: String,
    scheme_error: String,
}

#[cfg(test)]
impl EditorPagesState {
    pub(crate) fn set_scheme_input(&mut self, text: &str) {
        self.scheme_input = text.to_string();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemeOp {
    Duplicate,
    Rename,
    Export,
    Import,
}

impl Default for EditorPagesState {
    fn default() -> Self {
        EditorPagesState {
            scope: DEFAULTS.to_string(),
            role: SyntaxRole::Keyword,
            preview: None,
            scheme_op: None,
            scheme_input: String::new(),
            scheme_error: String::new(),
            fonts: None,
            fonts_job: None,
        }
    }
}

fn preview_language(scope: &str) -> &str {
    if scope == DEFAULTS {
        "php"
    } else {
        scope
    }
}

/// Applies a change of the scheme and re-themes the window.
fn save_scheme(app: &mut PhpIdeApp, ctx: &egui::Context, change: impl FnOnce(&mut SchemeChanges)) {
    let key = color_scheme::active_scheme(&app.settings.color_scheme, app.theme.name());
    let mut changes = SchemeChanges::new(&mut app.settings.color_scheme, &key);
    change(&mut changes);
    app.persist_settings();
    app.apply_design(ctx);
}

/// Scheme chooser and its menu: Duplicate, Rename, Delete, Export and
/// Import (RF-HL-12).
fn scheme_bar(app: &mut PhpIdeApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let active = color_scheme::active_scheme(&app.settings.color_scheme, app.theme.name());
    let customs = color_scheme::custom_schemes(&app.settings.color_scheme);
    let custom = customs.iter().any(|(name, _)| name == &active);
    let shown = if custom {
        active.clone()
    } else {
        app.theme.label().to_string()
    };
    let mut choose: Option<(crate::design::ThemeId, String)> = None;
    let mut delete = false;
    ui.horizontal(|ui| {
        let label = ui.label("Scheme:");
        egui::ComboBox::from_id_salt("scheme-theme")
            .selected_text(&shown)
            .width(220.0)
            .show_ui(ui, |ui| {
                for id in crate::design::ThemeId::ALL {
                    if ui
                        .selectable_label(!custom && app.theme == id, id.label())
                        .clicked()
                    {
                        choose = Some((id, id.name().to_string()));
                    }
                }
                if !customs.is_empty() {
                    ui.separator();
                }
                for (name, base) in &customs {
                    let id = crate::design::ThemeId::from_name(base);
                    let text = format!("{name}  ({})", id.label());
                    if ui.selectable_label(name == &active, text).clicked() {
                        choose = Some((id, name.clone()));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        let menu = crate::design::components::IconButton::new(
            phpide_icons::IconId::More,
            "Scheme actions",
        )
        .show(ui);
        egui::Popup::menu(&menu).show(|ui| {
            ui.set_min_width(180.0);
            let state = &mut app.settings_dialog.editor_pages;
            let mut start = |op: SchemeOp, input: String| {
                state.scheme_op = Some(op);
                state.scheme_input = input;
                state.scheme_error.clear();
            };
            if ui.button("Duplicate…").clicked() {
                start(SchemeOp::Duplicate, format!("{shown} copy"));
                ui.close();
            }
            if ui
                .add_enabled(custom, egui::Button::new("Rename…"))
                .clicked()
            {
                start(SchemeOp::Rename, active.clone());
                ui.close();
            }
            if ui
                .add_enabled(custom, egui::Button::new("Delete"))
                .clicked()
            {
                delete = true;
                ui.close();
            }
            ui.separator();
            let home = std::env::var("HOME").unwrap_or_default();
            if ui.button("Export…").clicked() {
                let file = format!(
                    "{home}/{}.phpide-scheme.json",
                    shown.replace(['/', ' '], "-")
                );
                start(SchemeOp::Export, file);
                ui.close();
            }
            if ui.button("Import…").clicked() {
                start(SchemeOp::Import, format!("{home}/"));
                ui.close();
            }
        });
    });
    if let Some((id, scheme)) = choose {
        color_scheme::set_active(&mut app.settings.color_scheme, id.name(), &scheme);
        app.apply_theme(id, ctx);
    }
    if delete {
        color_scheme::delete(&mut app.settings.color_scheme, &active);
        app.persist_settings();
        app.apply_design(ctx);
        app.ws.message = format!("Deleted the color scheme {active}");
    }
    scheme_op_row(app, ui, ctx, &active, &shown);
}

/// The inline field of the scheme operation in progress.
fn scheme_op_row(
    app: &mut PhpIdeApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    active: &str,
    shown: &str,
) {
    let Some(op) = app.settings_dialog.editor_pages.scheme_op else {
        return;
    };
    let (prompt, action) = match op {
        SchemeOp::Duplicate => ("New scheme name:", "Duplicate"),
        SchemeOp::Rename => ("New name:", "Rename"),
        SchemeOp::Export => ("Export to file:", "Export"),
        SchemeOp::Import => ("Import file:", "Import"),
    };
    let mut run = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let state = &mut app.settings_dialog.editor_pages;
        let label = ui.label(prompt);
        let field = ui
            .add(
                egui::TextEdit::singleline(&mut state.scheme_input)
                    .id_salt("scheme-op-input")
                    .desired_width(320.0),
            )
            .labelled_by(label.id);
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            run = true;
        }
        if ui.button(action).clicked() {
            run = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    let error = app.settings_dialog.editor_pages.scheme_error.clone();
    if !error.is_empty() {
        let p = crate::design::current(ctx).palette;
        ui.colored_label(p.error, error);
    }
    if cancel {
        app.settings_dialog.editor_pages.scheme_op = None;
        return;
    }
    if !run {
        return;
    }
    let input = app
        .settings_dialog
        .editor_pages
        .scheme_input
        .trim()
        .to_string();
    let theme = app.theme.name().to_string();
    let value = &mut app.settings.color_scheme;
    let result: Result<String, String> = match op {
        SchemeOp::Duplicate | SchemeOp::Rename => match color_scheme::invalid_name(value, &input) {
            Some(why) => Err(why.to_string()),
            None if op == SchemeOp::Duplicate => {
                color_scheme::duplicate(value, active, &input, &theme);
                Ok(format!("Created the color scheme {input}"))
            }
            None => {
                color_scheme::rename(value, active, &input);
                Ok(format!("Renamed the color scheme to {input}"))
            }
        },
        SchemeOp::Export => {
            let file = color_scheme::export(value, active, shown, &theme);
            let text = serde_json::to_string_pretty(&file).unwrap_or_default();
            std::fs::write(&input, text)
                .map(|()| format!("Exported the color scheme to {input}"))
                .map_err(|e| format!("Could not write {input}: {e}"))
        }
        SchemeOp::Import => std::fs::read_to_string(&input)
            .map_err(|e| format!("Could not read {input}: {e}"))
            .and_then(|text| serde_json::from_str(&text).map_err(|e| format!("Not JSON: {e}")))
            .and_then(|file: serde_json::Value| color_scheme::import(value, &file))
            .map(|(name, base)| {
                let id = crate::design::ThemeId::from_name(&base);
                app.theme = id;
                format!("Imported the color scheme {name}")
            }),
    };
    match result {
        Ok(message) => {
            app.settings_dialog.editor_pages.scheme_op = None;
            app.ws.message = message;
            app.persist_settings();
            app.apply_design(ctx);
        }
        Err(error) => app.settings_dialog.editor_pages.scheme_error = error,
    }
}

pub fn color_scheme_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let theme = crate::design::current(&ctx);
    scheme_bar(app, ui, &ctx);
    ui.horizontal(|ui| {
        ui.weak("A scheme is built on a theme; choosing it switches the theme.");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Reset All Changes")
                .on_hover_text("Back to the scheme's own colors")
                .clicked()
            {
                save_scheme(app, &ctx, |c| c.reset());
            }
        });
    });
    ui.add_space(6.0);

    let state = &mut app.settings_dialog.editor_pages;
    let scope = state.scope.clone();
    let role = state.role;
    let lists = egui::vec2(ui.available_width(), 262.0);
    ui.allocate_ui_with_layout(lists, egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_height(lists.y);
        // Sections: Language Defaults, then each language.
        ui.vertical(|ui| {
            ui.set_width(170.0);
            egui::ScrollArea::vertical()
                .id_salt("scheme-scopes")
                .max_height(250.0)
                .show(ui, |ui| {
                    let state = &mut app.settings_dialog.editor_pages;
                    ui.selectable_value(
                        &mut state.scope,
                        DEFAULTS.to_string(),
                        "Language Defaults",
                    );
                    ui.separator();
                    for (id, name) in color_scheme::languages() {
                        ui.selectable_value(&mut state.scope, id.to_string(), name);
                    }
                });
        });
        ui.separator();
        // Elements of the section.
        ui.vertical(|ui| {
            ui.set_width(190.0);
            egui::ScrollArea::vertical()
                .id_salt("scheme-roles")
                .max_height(250.0)
                .show(ui, |ui| {
                    let state = &mut app.settings_dialog.editor_pages;
                    for (r, _, label) in ROLES {
                        ui.selectable_value(&mut state.role, *r, *label);
                    }
                });
        });
        ui.separator();
        ui.vertical(|ui| element_editor(app, ui, &scope, role));
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Preview").strong());
    ui.weak("Click a word to edit its element.");
    let language = preview_language(&scope).to_string();
    let state = &mut app.settings_dialog.editor_pages;
    if state
        .preview
        .as_ref()
        .is_none_or(|p| p.language != language)
    {
        let (name, text) = color_scheme::sample(&language);
        let mut buffer = Buffer::untitled();
        buffer.replace_all(text);
        buffer.readonly = true;
        let detected = match filetype::by_id(&language) {
            Some(file_type) => Detected::Known(file_type),
            None => filetype::detect(name, ""),
        };
        let mut syntax = Syntax::for_type(&detected);
        syntax.wait_idle(&buffer.document);
        state.preview = Some(Preview {
            language: language.clone(),
            buffer,
            view: EditorView::new(),
            syntax,
        });
    }
    let mut style = theme.editor_style();
    style.syntax = theme.syntax_style_for(Some(&language));
    let Some(preview) = state.preview.as_mut() else {
        return;
    };
    let decorations = phpide_editor::decorations::Decorations::default();
    let mut clicked_role = None;
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme.palette.border_subtle))
        .show(ui, |ui| {
            ui.set_height(220.0);
            ui.set_width(ui.available_width());
            let output = Editor::new(
                "scheme-preview",
                &mut preview.buffer,
                &mut preview.view,
                &mut preview.syntax,
                &style,
                &decorations,
            )
            .options(EditorOptions {
                sticky_lines: 0,
                folding: false,
                blink: false,
                ..EditorOptions::default()
            })
            .show(ui);
            if output.response.as_ref().is_some_and(|r| r.clicked()) {
                if let Some(pos) = output.hover {
                    clicked_role = Some(role_at(preview, pos));
                }
            }
        });
    if let Some(role) = clicked_role {
        app.settings_dialog.editor_pages.role = role;
    }
}

/// Role of the highlighted span at char `pos` of the preview.
fn role_at(preview: &mut Preview, pos: usize) -> SyntaxRole {
    let rope = preview.buffer.document.rope().clone();
    let line = rope.char_to_line(pos.min(rope.len_chars()));
    let column = pos - rope.line_to_char(line);
    let spans = preview
        .syntax
        .line_spans(&preview.buffer.document, line, line);
    spans
        .first()
        .and_then(|spans| {
            spans
                .iter()
                .rev()
                .find(|s| s.start <= column && column < s.end)
                .map(|s| s.role)
        })
        .unwrap_or(SyntaxRole::Plain)
}

fn element_editor(app: &mut PhpIdeApp, ui: &mut egui::Ui, scope: &str, role: SyntaxRole) {
    let ctx = ui.ctx().clone();
    let theme = crate::design::current(&ctx);
    let theme_name = color_scheme::active_scheme(&app.settings.color_scheme, app.theme.name());
    let label = ROLES
        .iter()
        .find(|(r, _, _)| *r == role)
        .map_or("", |(_, _, label)| *label);
    ui.label(egui::RichText::new(label).strong());
    let user = SchemeChanges::new(&mut app.settings.color_scheme, &theme_name).get(scope, role);
    let language = (scope != DEFAULTS).then_some(scope);
    let shown = user.unwrap_or_else(|| {
        // What the element looks like now: inherited values.
        let parent = if scope == DEFAULTS {
            theme.base_syntax_style()
        } else {
            theme.syntax_style_for(None)
        };
        Element::of(&parent, role)
    });
    let mut element = shown;
    let mut changed = false;
    let mut inherit = user.is_none();
    if language.is_some() {
        if ui
            .checkbox(&mut inherit, "Inherit values from Language Defaults")
            .changed()
        {
            let value = if inherit { None } else { Some(shown) };
            save_scheme(app, &ctx, |c| c.set(scope, role, value));
            return;
        }
    } else if user.is_some() && ui.button("Use the scheme's value").clicked() {
        save_scheme(app, &ctx, |c| c.set(scope, role, None));
        return;
    }
    let editable = language.is_none() || !inherit;
    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut element.bold, "Bold").changed();
            changed |= ui.checkbox(&mut element.italic, "Italic").changed();
        });
        changed |= color_row(
            ui,
            "Foreground",
            &mut element.foreground,
            theme.palette.text_primary,
        );
        changed |= color_row(
            ui,
            "Background",
            &mut element.background,
            theme.palette.bg_editor,
        );
        ui.horizontal(|ui| {
            let mut on = element.effect.is_some();
            if ui.checkbox(&mut on, "Effects").changed() {
                element.effect = on.then_some((Effect::Underline, None));
                changed = true;
            }
            if let Some((kind, color)) = &mut element.effect {
                egui::ComboBox::from_id_salt("scheme-effect")
                    .width(130.0)
                    .selected_text(match kind {
                        Effect::Underline => "Underline",
                        Effect::Strikethrough => "Strikethrough",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(kind, Effect::Underline, "Underline")
                            .changed();
                        changed |= ui
                            .selectable_value(kind, Effect::Strikethrough, "Strikethrough")
                            .changed();
                    });
                let mut c =
                    color.unwrap_or(element.foreground.unwrap_or(theme.palette.text_primary));
                if egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::Opaque,
                )
                .changed()
                {
                    *color = Some(c);
                    changed = true;
                }
            }
        });
    });
    if changed {
        save_scheme(app, &ctx, |c| c.set(scope, role, Some(element)));
    }
}

/// `[x] Label [color]`: unchecked means "no value" (`None`).
fn color_row(
    ui: &mut egui::Ui,
    label: &str,
    color: &mut Option<egui::Color32>,
    fallback: egui::Color32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut on = color.is_some();
        if ui.checkbox(&mut on, label).changed() {
            *color = on.then_some(color.unwrap_or(fallback));
            changed = true;
        }
        if let Some(c) = color {
            changed |= egui::color_picker::color_edit_button_srgba(
                ui,
                c,
                egui::color_picker::Alpha::Opaque,
            )
            .changed();
            ui.weak(crate::design::tokens::hex_color_string(*c));
        }
    });
    changed
}

pub fn font_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // Installed monospaced fonts, read once in the background.
    let state = &mut app.settings_dialog.editor_pages;
    if state.fonts.is_none() && state.fonts_job.is_none() {
        let repaint = ctx.clone();
        state.fonts_job = Some(app.jobs.spawn(move |_| {
            let fonts = crate::services::system_fonts::monospace_families();
            repaint.request_repaint();
            Ok(fonts)
        }));
    }
    if let Some(job) = &mut state.fonts_job {
        if let Some(outcome) = job.poll() {
            state.fonts = Some(match outcome {
                JobOutcome::Ok(fonts) => fonts,
                _ => Vec::new(),
            });
            state.fonts_job = None;
        }
    }
    let bundled = crate::design::fonts::BUNDLED_EDITOR_FAMILY;
    let current = if app.settings.editor_font_family.is_empty() {
        bundled.to_string()
    } else {
        app.settings.editor_font_family.clone()
    };
    let mut chosen: Option<String> = None;
    ui.horizontal(|ui| {
        let label = ui.label("Font:");
        egui::ComboBox::from_id_salt("editor-font-family")
            .selected_text(&current)
            .width(260.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current == bundled, format!("{bundled} (bundled)"))
                    .clicked()
                {
                    chosen = Some(String::new());
                }
                match &app.settings_dialog.editor_pages.fonts {
                    None => {
                        ui.spinner();
                    }
                    Some(fonts) => {
                        for font in fonts.iter().filter(|f| f.family != bundled) {
                            let label = if font.bold.is_some() {
                                font.family.clone()
                            } else {
                                format!("{} (no bold)", font.family)
                            };
                            if ui.selectable_label(current == font.family, label).clicked() {
                                chosen = Some(font.family.clone());
                            }
                        }
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    if let Some(family) = chosen {
        app.settings.editor_font_family = family;
        app.persist_settings();
        app.apply_design(&ctx);
    }
    ui.horizontal(|ui| {
        let label = ui.label("Size:");
        let mut size = app.settings.editor_font_size;
        let slider = ui
            .add(
                egui::Slider::new(&mut size, 8.0..=32.0)
                    .step_by(1.0)
                    .show_value(false),
            )
            .labelled_by(label.id);
        let value = ui
            .add(
                egui::DragValue::new(&mut size)
                    .range(8.0..=32.0)
                    .suffix(" pt"),
            )
            .labelled_by(label.id);
        if slider.changed() || value.changed() {
            app.settings.editor_font_size = crate::settings::clamp_editor_font(size);
            app.persist_settings();
            app.apply_design(&ctx);
        }
    });
    ui.horizontal(|ui| {
        let label = ui.label("Line height:");
        let mut height = app.settings.editor_line_height;
        let slider = ui
            .add(
                egui::Slider::new(&mut height, 1.0..=2.0)
                    .step_by(0.05)
                    .show_value(false),
            )
            .labelled_by(label.id);
        let value = ui
            .add(
                egui::DragValue::new(&mut height)
                    .range(1.0..=2.0)
                    .speed(0.05)
                    .fixed_decimals(2),
            )
            .labelled_by(label.id);
        if slider.changed() || value.changed() {
            app.settings.editor_line_height = height;
            app.persist_settings();
            app.apply_design(&ctx);
        }
    });
    ui.weak("Ligatures: not available — the text renderer draws one glyph per character.");
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let label = ui.label("IDE zoom:");
        let mut zoom = app.zoom;
        let slider = ui
            .add(egui::Slider::new(&mut zoom, 0.5..=3.0).show_value(false))
            .labelled_by(label.id);
        let value = ui
            .add(
                egui::DragValue::new(&mut zoom)
                    .range(0.5..=3.0)
                    .speed(0.05)
                    .suffix("×"),
            )
            .labelled_by(label.id);
        if slider.changed() || value.changed() {
            app.set_zoom(zoom);
        }
    });
    ui.label("Shortcuts: Alt+Shift+. bigger · Alt+Shift+, smaller · Ctrl+0 reset");
    ui.add_space(8.0);
    let theme = crate::design::current(&ctx);
    egui::Frame::new()
        .fill(theme.palette.bg_editor)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("function total(int $count): float { return 0.15 * $count; }")
                    .font(theme.editor_font())
                    .color(theme.palette.text_primary),
            );
        });
}
