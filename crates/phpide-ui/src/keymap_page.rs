//! Settings › Keymap (RF-UI-06): every action with its shortcuts, searched
//! by name or by shortcut; shortcuts are added by pressing them, removed,
//! or reset to the IntelliJ defaults. Find Action opens this page on an
//! action with Alt+Enter.

use eframe::egui::{self, KeyboardShortcut};

use crate::commands::{self, Command, REGISTRY};
use crate::PhpIdeApp;

#[derive(Default)]
pub struct KeymapState {
    query: String,
    /// Action whose shortcuts are being edited.
    pub selected: Option<Command>,
    /// Waiting for the keys of a new shortcut.
    pub recording: bool,
    /// Keys pressed, waiting for a decision about a conflict.
    conflict: Option<(KeyboardShortcut, Vec<Command>)>,
    /// Scroll to the selected action on the next frame.
    reveal: bool,
}

impl KeymapState {
    /// Opens the page on `id`, ready to record a shortcut.
    pub fn assign(&mut self, id: Command) {
        self.selected = Some(id);
        self.recording = true;
        self.conflict = None;
        self.query.clear();
        self.reveal = true;
    }
}

/// Saves the shortcuts of `changes` (the rest keep theirs) and applies them.
fn save(app: &mut PhpIdeApp, changes: &[(Command, Vec<KeyboardShortcut>)]) {
    let table: Vec<(Command, Vec<KeyboardShortcut>)> = REGISTRY
        .iter()
        .map(|spec| {
            let list = changes
                .iter()
                .find(|(id, _)| *id == spec.id)
                .map(|(_, list)| list.clone())
                .unwrap_or_else(|| commands::shortcuts_of(spec.id));
            (spec.id, list)
        })
        .collect();
    app.settings.shortcuts = commands::overrides_for_settings(&table);
    commands::set_overrides(&app.settings.shortcuts);
    app.persist_settings();
}

/// The key pressed this frame as a shortcut, ignoring lone modifiers.
fn pressed_shortcut(ui: &egui::Ui) -> Option<KeyboardShortcut> {
    ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if *key != egui::Key::Escape => {
                let mut m = egui::Modifiers::NONE;
                m.ctrl = modifiers.ctrl || modifiers.command;
                m.shift = modifiers.shift;
                m.alt = modifiers.alt;
                Some(KeyboardShortcut::new(m, *key))
            }
            _ => None,
        })
    })
}

pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    ui.horizontal(|ui| {
        ui.label("Keymap:");
        ui.label(egui::RichText::new("IntelliJ IDEA Classic").strong());
        if !app.settings.shortcuts.is_empty() {
            ui.label(
                egui::RichText::new(format!("({} changed)", app.settings.shortcuts.len()))
                    .color(p.text_secondary),
            );
            if ui.button("Reset All").clicked() {
                app.settings.shortcuts.clear();
                commands::set_overrides(&[]);
                app.persist_settings();
            }
        }
    });
    ui.add_space(4.0);
    let state = &mut app.settings_dialog.keymap;
    crate::design::components::TextInput::search(&mut state.query)
        .hint("Search by action name or shortcut")
        .id_salt("keymap-search")
        .show(ui);
    ui.add_space(4.0);

    if state.recording {
        recorder(app, ui);
    }

    let query = app.settings_dialog.keymap.query.trim().to_lowercase();
    let mut select: Option<Command> = None;
    egui::ScrollArea::vertical()
        .id_salt("keymap-list")
        .max_height(360.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let mut category = None;
            for spec in REGISTRY.iter().filter(|s| commands::is_available(s.id)) {
                let shortcuts = commands::shortcuts_of(spec.id);
                let text: Vec<String> = shortcuts.iter().map(commands::format_shortcut).collect();
                let text = text.join(", ");
                let matches = query.is_empty()
                    || spec.palette_label().to_lowercase().contains(&query)
                    || text.to_lowercase().contains(&query);
                if !matches {
                    continue;
                }
                if category != Some(spec.category) {
                    category = Some(spec.category);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(spec.category.label()).strong());
                }
                let selected = app.settings_dialog.keymap.selected == Some(spec.id);
                let changed = shortcuts.as_slice() != spec.shortcuts;
                let row = ui
                    .horizontal(|ui| {
                        let label = if changed {
                            format!("{} *", spec.label)
                        } else {
                            spec.label.to_string()
                        };
                        let response = ui.selectable_label(selected, label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(&text)
                                    .monospace()
                                    .color(p.text_secondary),
                            );
                        });
                        response
                    })
                    .inner;
                if row.clicked() {
                    select = Some(spec.id);
                }
                if selected && std::mem::take(&mut app.settings_dialog.keymap.reveal) {
                    row.scroll_to_me(Some(egui::Align::Center));
                }
                if selected {
                    editor(app, ui, spec.id, &shortcuts);
                }
            }
        });
    if let Some(id) = select {
        let state = &mut app.settings_dialog.keymap;
        state.selected = (state.selected != Some(id)).then_some(id);
        state.recording = false;
        state.conflict = None;
    }
}

/// Shortcuts of the selected action, with Add, Remove and Reset.
fn editor(app: &mut PhpIdeApp, ui: &mut egui::Ui, id: Command, shortcuts: &[KeyboardShortcut]) {
    let default = commands::spec_of(id).shortcuts;
    let mut change: Option<Vec<KeyboardShortcut>> = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if shortcuts.is_empty() {
            ui.weak("No shortcut");
        }
        for (k, sc) in shortcuts.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(commands::format_shortcut(sc)).monospace());
                if ui
                    .small_button(format!("Remove {}", commands::format_shortcut(sc)))
                    .clicked()
                {
                    let mut list = shortcuts.to_vec();
                    list.remove(k);
                    change = Some(list);
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Add Keyboard Shortcut").clicked() {
                let state = &mut app.settings_dialog.keymap;
                state.recording = true;
                state.conflict = None;
            }
            if ui
                .add_enabled(shortcuts != default, egui::Button::new("Reset to Default"))
                .clicked()
            {
                change = Some(default.to_vec());
            }
        });
    });
    if let Some(list) = change {
        save(app, &[(id, list)]);
    }
}

/// "Press the keys" and the conflict question.
fn recorder(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(id) = app.settings_dialog.keymap.selected else {
        app.settings_dialog.keymap.recording = false;
        return;
    };
    let p = crate::design::current(ui.ctx()).palette;
    let label = commands::spec_of(id).palette_label();
    let mut assign: Option<(KeyboardShortcut, Vec<Command>)> = None;
    let mut cancel = false;
    egui::Frame::group(ui.style())
        .fill(p.bg_elevated)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            match app.settings_dialog.keymap.conflict.clone() {
                None => {
                    ui.label(egui::RichText::new(format!("New shortcut for {label}")).strong());
                    ui.label("Press the keys now. Esc cancels.");
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                        cancel = true;
                    } else if let Some(sc) = pressed_shortcut(ui) {
                        let others = commands::commands_using(&sc, id);
                        if others.is_empty() {
                            assign = Some((sc, Vec::new()));
                        } else {
                            app.settings_dialog.keymap.conflict = Some((sc, others));
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                }
                Some((sc, others)) => {
                    let names: Vec<String> = others
                        .iter()
                        .map(|o| commands::spec_of(*o).palette_label())
                        .collect();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} is already assigned to {}.",
                            commands::format_shortcut(&sc),
                            names.join(", ")
                        ))
                        .color(p.warning),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Remove from the Others").clicked() {
                            assign = Some((sc, others.clone()));
                        }
                        if ui.button("Keep Both").clicked() {
                            assign = Some((sc, Vec::new()));
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                        cancel = true;
                    }
                }
            }
        });
    if let Some((sc, take_from)) = assign {
        let mut changes = Vec::new();
        let mut mine = commands::shortcuts_of(id);
        if !mine.contains(&sc) {
            mine.push(sc);
        }
        changes.push((id, mine));
        for other in take_from {
            let list = commands::shortcuts_of(other)
                .into_iter()
                .filter(|o| {
                    !(o.logical_key == sc.logical_key
                        && o.modifiers.ctrl == sc.modifiers.ctrl
                        && o.modifiers.shift == sc.modifiers.shift
                        && o.modifiers.alt == sc.modifiers.alt)
                })
                .collect();
            changes.push((other, list));
        }
        save(app, &changes);
        app.ws.message = format!("{} now runs {label}", commands::format_shortcut(&sc));
        cancel = true;
    }
    if cancel {
        let state = &mut app.settings_dialog.keymap;
        state.recording = false;
        state.conflict = None;
    }
}
