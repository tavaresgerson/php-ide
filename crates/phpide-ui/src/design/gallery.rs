//! Component gallery: every component in every state, for the active theme
//! and density. Run it with `phpide --gallery`; tests snapshot it for each
//! theme × density (`tests/snapshots/gallery-*.png`).

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};
use phpide_icons::{file_icon, IconId};

use super::components::{
    menu_frame, menu_item, menu_separator, panel_frame, splitter, toast_card, tooltip_contents,
    Button, ButtonKind, Checkbox, Dialog, EmptyState, IconButton, ListRow, PanelHeader,
    ProgressBar, Select, Spinner, SplitAxis, State, StatusItem, Tab, TextInput, Toast, Tone,
    ToolbarWidget, TreeRow,
};
use super::theme::{current, install, Theme, ThemeId};
use super::tokens::{space, typography, Density, Palette};

/// Mutable values the gallery widgets edit.
pub struct GalleryState {
    pub text: String,
    pub search: String,
    pub empty: String,
    pub checked: bool,
    pub unchecked: bool,
    pub select: usize,
}

impl Default for GalleryState {
    fn default() -> Self {
        GalleryState {
            text: "src/Controller/UserController.php".into(),
            search: "Repository".into(),
            empty: String::new(),
            checked: true,
            unchecked: false,
            select: 1,
        }
    }
}

pub const GALLERY_WIDTH: f32 = 1180.0;

const SAMPLE_PHP: &str = r#"<?php
declare(strict_types=1);

namespace App\Service;

/** Sends the weekly report. */
#[AsCommand(name: 'report:send')]
final class ReportMailer extends Mailer
{
    private const LIMIT = 50;

    public function send(int $userId, ?string $locale = null): bool
    {
        // Load the recipient lazily.
        $user = $this->users->find($userId);
        return $user !== null && \count($this->queue) < self::LIMIT;
    }
}"#;

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let theme = current(ui.ctx());
    let p = theme.palette;
    ui.add_space(space::S6);
    ui.label(
        RichText::new(title)
            .font(egui::FontId::new(
                typography::TITLE,
                super::fonts::family(super::fonts::UI_SEMIBOLD),
            ))
            .color(p.text_primary),
    );
    ui.add_space(space::S3);
    egui::Frame::new()
        .fill(p.bg_panel)
        .stroke(Stroke::new(1.0, p.border_subtle))
        .corner_radius(CornerRadius::same(super::tokens::radius::CARD))
        .inner_margin(egui::Margin::same(space::S6 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
}

fn caption(ui: &mut egui::Ui, text: &str) {
    let p = current(ui.ctx()).palette;
    ui.label(
        RichText::new(text)
            .size(typography::SMALL)
            .color(p.text_muted),
    );
}

fn state_row(ui: &mut egui::Ui, label: &str, mut item: impl FnMut(&mut egui::Ui, State)) {
    ui.horizontal(|ui| {
        ui.set_min_height(current(ui.ctx()).metrics.control);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(110.0, 20.0), egui::Sense::hover());
        let p = current(ui.ctx()).palette;
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(typography::LABEL),
            p.text_secondary,
        );
        for state in State::ALL {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                item(ui, state);
            });
        }
    });
}

fn header_row(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(110.0 + ui.spacing().item_spacing.x);
        for state in State::ALL {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                caption(ui, state.label());
            });
        }
    });
}

/// A row of the demo tree.
struct DemoRow {
    name: &'static str,
    depth: usize,
    expanded: Option<bool>,
    color: Option<Color32>,
    selected: bool,
    focused: bool,
    forced: Option<State>,
}

impl DemoRow {
    fn folder(name: &'static str, depth: usize, open: bool) -> Self {
        DemoRow {
            name,
            depth,
            expanded: Some(open),
            color: None,
            selected: false,
            focused: false,
            forced: None,
        }
    }

    fn file(name: &'static str, depth: usize, color: Option<Color32>) -> Self {
        DemoRow {
            expanded: None,
            color,
            ..DemoRow::folder(name, depth, false)
        }
    }
}

/// The gallery page. Returns the height used (for snapshots).
pub fn show(ui: &mut egui::Ui, state: &mut GalleryState) -> f32 {
    let theme = current(ui.ctx());
    let p = theme.palette;
    let top = ui.cursor().top();
    ui.set_width(GALLERY_WIDTH);
    ui.label(
        RichText::new(format!(
            "PHP IDE components — {} · {}",
            theme.id.label(),
            theme.density.label()
        ))
        .font(egui::FontId::new(
            typography::DISPLAY,
            super::fonts::family(super::fonts::UI_SEMIBOLD),
        ))
        .color(p.text_primary),
    );

    section(ui, "Buttons", |ui| {
        header_row(ui);
        for kind in ButtonKind::ALL {
            state_row(ui, kind.label(), |ui, s| {
                Button::new("Run Tests").kind(kind).force(s).show(ui);
            });
        }
        state_row(ui, "With icon", |ui, s| {
            Button::primary("Debug")
                .icon(IconId::Debug)
                .force(s)
                .show(ui);
        });
        state_row(ui, "Icon button", |ui, s| {
            ui.horizontal(|ui| {
                IconButton::new(IconId::Run, "Run").force(s).show(ui);
                IconButton::new(IconId::Project, "Project")
                    .selected(true)
                    .force(s)
                    .show(ui);
            });
        });
    });

    section(ui, "Inputs", |ui| {
        header_row(ui);
        state_row(ui, "Text", |ui, s| {
            TextInput::new(&mut state.text)
                .id_salt(("gallery-text", s))
                .width(140.0)
                .force(s)
                .show(ui);
        });
        state_row(ui, "Search", |ui, s| {
            TextInput::search(&mut state.search)
                .id_salt(("gallery-search", s))
                .width(140.0)
                .force(s)
                .show(ui);
        });
        state_row(ui, "Placeholder", |ui, s| {
            TextInput::new(&mut state.empty)
                .hint("Class name")
                .id_salt(("gallery-empty", s))
                .width(140.0)
                .force(s)
                .show(ui);
        });
        state_row(ui, "Select", |ui, s| {
            Select::new(
                ("gallery-select", s),
                &mut state.select,
                &["PHP 8.2", "PHP 8.3", "PHP 8.4"],
            )
            .width(140.0)
            .force(s)
            .show(ui);
        });
        state_row(ui, "Checkbox", |ui, s| {
            ui.horizontal(|ui| {
                Checkbox::new(&mut state.checked, "On").force(s).show(ui);
                Checkbox::new(&mut state.unchecked, "Off").force(s).show(ui);
            });
        });
        ui.horizontal(|ui| {
            ui.add_space(110.0 + ui.spacing().item_spacing.x);
            let mut invalid = "user name".to_owned();
            TextInput::new(&mut invalid)
                .id_salt("gallery-invalid")
                .invalid(true)
                .width(140.0)
                .show(ui);
            ui.add_space(space::S3);
            caption(ui, "Invalid value");
        });
    });

    section(ui, "Navigation", |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(520.0);
                caption(ui, "Tabs: active, inactive, hover, modified");
                egui::Frame::new().fill(p.bg_canvas).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (php, kind) = file_icon("UserController.php");
                        Tab::new("UserController.php")
                            .icon(php, p.file_color(kind))
                            .active(true)
                            .show(ui);
                        let (icon, kind) = file_icon("composer.json");
                        Tab::new("composer.json")
                            .icon(icon, p.file_color(kind))
                            .force(State::Rest)
                            .show(ui);
                        let (icon, kind) = file_icon("app.js");
                        Tab::new("app.js")
                            .icon(icon, p.file_color(kind))
                            .force(State::Hovered)
                            .show(ui);
                        let (icon, kind) = file_icon("routes.yaml");
                        Tab::new("routes.yaml")
                            .icon(icon, p.file_color(kind))
                            .modified(true)
                            .force(State::Rest)
                            .show(ui);
                    });
                });
                ui.add_space(space::S5);
                caption(ui, "Tree: selected+focused, selected, hover, Git colors");
                let rows: [DemoRow; 7] = [
                    DemoRow::folder("src", 0, true),
                    DemoRow::folder("Controller", 1, true),
                    DemoRow {
                        selected: true,
                        focused: true,
                        ..DemoRow::file("UserController.php", 2, Some(p.git_modified))
                    },
                    DemoRow {
                        forced: Some(State::Hovered),
                        ..DemoRow::file("ReportMailer.php", 2, Some(p.git_added))
                    },
                    DemoRow {
                        selected: true,
                        ..DemoRow::folder("tests", 0, false)
                    },
                    DemoRow::file("composer.lock", 0, Some(p.git_untracked)),
                    DemoRow {
                        color: Some(p.git_ignored),
                        ..DemoRow::folder("vendor", 0, false)
                    },
                ];
                for DemoRow {
                    name,
                    depth,
                    expanded,
                    color,
                    selected,
                    focused,
                    forced,
                } in rows
                {
                    let (icon, color_icon) = match expanded {
                        Some(true) => (IconId::FolderOpen, p.folder),
                        Some(false) => (IconId::Folder, p.folder),
                        None => {
                            let (icon, kind) = file_icon(name);
                            (icon, p.file_color(kind))
                        }
                    };
                    let mut row = TreeRow::new(name)
                        .depth(depth)
                        .expanded(expanded)
                        .icon(icon, color_icon)
                        .selected(selected)
                        .focused(focused);
                    if let Some(color) = color {
                        row = row.label_color(color);
                    }
                    if name == "src" {
                        row = row.detail("~/projects/shop");
                    }
                    if let Some(state) = forced {
                        row = row.force(state);
                    }
                    row.show(ui);
                }
            });
            ui.add_space(space::S6);
            ui.vertical(|ui| {
                ui.set_width(300.0);
                caption(ui, "List rows");
                ListRow::new("Find in Files")
                    .icon(IconId::Search, p.text_secondary)
                    .trailing("Ctrl+Shift+F")
                    .selected(true)
                    .show(ui);
                ListRow::new("Go to Line")
                    .icon(IconId::Command, p.text_secondary)
                    .detail("Navigate")
                    .trailing("Ctrl+G")
                    .force(State::Hovered)
                    .show(ui);
                ListRow::new("Run")
                    .icon(IconId::Run, p.success)
                    .trailing("Shift+F10")
                    .show(ui);
                ui.add_space(space::S5);
                caption(ui, "Menu");
                menu_frame(ui).show(ui, |ui| {
                    ui.set_width(260.0);
                    menu_item(ui, Some(IconId::FolderOpen), "Open…", "Ctrl+O", true);
                    menu_item(ui, Some(IconId::Save), "Save All", "Ctrl+S", true);
                    menu_separator(ui);
                    menu_item(ui, None, "Reload from Disk", "", true);
                    menu_item(ui, Some(IconId::Trash), "Delete", "Delete", false);
                });
            });
            ui.add_space(space::S6);
            ui.vertical(|ui| {
                caption(ui, "Tooltip");
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    tooltip_contents(ui, "Run 'UserTest'", Some("Shift+F10"));
                });
            });
        });
    });

    section(ui, "Surfaces", |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(360.0);
                caption(ui, "Tool window: header, body, splitters");
                egui::Frame::new()
                    .stroke(Stroke::new(1.0, p.border_subtle))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        PanelHeader::new("Project").show(ui, |ui| {
                            IconButton::new(IconId::More, "Options").show(ui);
                            IconButton::new(IconId::Refresh, "Refresh").show(ui);
                        });
                        panel_frame(ui).show(ui, |ui| {
                            EmptyState::new(IconId::Folder, "No project open")
                                .description("Open a folder to see its files.")
                                .action("Open Folder…")
                                .show(ui);
                        });
                    });
                ui.add_space(space::S4);
                ui.horizontal(|ui| {
                    for state in [State::Rest, State::Hovered, State::Pressed] {
                        splitter(ui, SplitAxis::Vertical, 40.0, Some(state));
                        caption(ui, state.label());
                    }
                });
                splitter(ui, SplitAxis::Horizontal, 340.0, Some(State::Rest));
            });
            ui.add_space(space::S6);
            ui.vertical(|ui| {
                caption(ui, "Dialog");
                Dialog::new("gallery-dialog", "Rename File")
                    .width(360.0)
                    .show_inline(
                        ui,
                        |ui| {
                            let mut name = "UserController.php".to_owned();
                            TextInput::new(&mut name)
                                .id_salt("gallery-dialog-input")
                                .show(ui);
                            ui.add_space(space::S3);
                            let mut refs = true;
                            Checkbox::new(&mut refs, "Search for references").show(ui);
                        },
                        |ui| {
                            Button::primary("Rename").show(ui);
                            Button::new("Cancel").show(ui);
                        },
                    );
            });
        });
    });

    section(ui, "Feedback", |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                caption(ui, "Toasts");
                toast_card(
                    ui,
                    &Toast::new(Tone::Success, "Tests passed").body("42 tests in 1.8 s"),
                );
                toast_card(
                    ui,
                    &Toast::new(Tone::Warning, "Composer is outdated")
                        .body("composer.lock was changed by another branch."),
                );
                toast_card(
                    ui,
                    &Toast::new(Tone::Error, "PHP not found")
                        .body("Set the interpreter in Settings → PHP."),
                );
            });
            ui.add_space(space::S6);
            ui.vertical(|ui| {
                caption(ui, "Status bar items");
                egui::Frame::new().fill(p.bg_panel).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        StatusItem::new("main").icon(IconId::GitBranch).show(ui);
                        StatusItem::new("2 errors")
                            .icon(IconId::Error)
                            .tone(Tone::Error)
                            .show(ui);
                        StatusItem::new("1 warning")
                            .icon(IconId::Warning)
                            .tone(Tone::Warning)
                            .show(ui);
                        StatusItem::new("PHP 8.3").force(State::Hovered).show(ui);
                        StatusItem::new("UTF-8").show(ui);
                    });
                });
                ui.add_space(space::S5);
                caption(ui, "Header widgets");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    ToolbarWidget::new("shop").icon(IconId::Project).show(ui);
                    ToolbarWidget::new("main")
                        .icon(IconId::GitBranch)
                        .force(State::Hovered)
                        .show(ui);
                    ToolbarWidget::new("index.php").show(ui);
                    ToolbarWidget::new("3")
                        .icon(IconId::Error)
                        .icon_tint(p.error)
                        .chevron(false)
                        .show(ui);
                });
                ui.add_space(space::S5);
                caption(ui, "Progress");
                ProgressBar::new(Some(0.62))
                    .label("Indexing… 62%")
                    .show(ui, 300.0);
                ui.add_space(space::S3);
                ProgressBar::new(None)
                    .label("Cloning repository")
                    .show(ui, 300.0);
                ui.add_space(space::S3);
                ui.horizontal(|ui| {
                    Spinner::show(ui, 16.0);
                    caption(ui, "Spinner (only while work runs)");
                });
            });
        });
    });

    section(ui, "Typography and syntax", |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(380.0);
                let scale = [
                    (
                        "Display 22",
                        egui::FontId::new(
                            typography::DISPLAY,
                            super::fonts::family(super::fonts::UI_SEMIBOLD),
                        ),
                    ),
                    (
                        "Title 16",
                        egui::FontId::new(
                            typography::TITLE,
                            super::fonts::family(super::fonts::UI_SEMIBOLD),
                        ),
                    ),
                    (
                        "Body 13 — Ação, naïve, Привет",
                        egui::FontId::proportional(typography::BODY),
                    ),
                    (
                        "Label 12",
                        egui::FontId::new(
                            typography::LABEL,
                            super::fonts::family(super::fonts::UI_MEDIUM),
                        ),
                    ),
                    ("Small 11", egui::FontId::proportional(typography::SMALL)),
                    (
                        "Mono 14 — -> => !== ===",
                        egui::FontId::monospace(typography::EDITOR),
                    ),
                ];
                for (text, font) in scale {
                    ui.label(RichText::new(text).font(font).color(p.text_primary));
                }
                ui.add_space(space::S4);
                ui.label(RichText::new("Secondary text").color(p.text_secondary));
                ui.label(RichText::new("Muted metadata").color(p.text_muted));
                ui.label(RichText::new("Disabled").color(p.text_disabled));
                ui.add_space(space::S4);
                swatches(ui, &p);
            });
            ui.add_space(space::S6);
            egui::Frame::new()
                .fill(p.bg_editor)
                .stroke(Stroke::new(1.0, p.border_subtle))
                .inner_margin(egui::Margin::same(space::S5 as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let job = phpide_php::highlight(SAMPLE_PHP, &theme.syntax_style());
                    ui.label(job);
                });
        });
    });

    section(ui, "Icons (Tabler)", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
            for id in IconId::catalog() {
                IconButton::new(*id, id.accessible_name())
                    .force(State::Rest)
                    .show(ui);
            }
        });
    });
    ui.add_space(space::S6);
    ui.cursor().top() - top
}

fn swatches(ui: &mut egui::Ui, p: &Palette) {
    let colors = [
        ("accent", p.accent),
        ("success", p.success),
        ("warning", p.warning),
        ("error", p.error),
        ("info", p.info),
        ("added", p.git_added),
        ("modified", p.git_modified),
        ("untracked", p.git_untracked),
        ("breakpoint", p.debug_breakpoint),
        ("selection", p.editor_selection_bg),
        ("match", p.search_match_bg),
        ("canvas", p.bg_canvas),
    ];
    egui::Grid::new("gallery-swatches")
        .num_columns(4)
        .spacing(egui::vec2(space::S4, space::S3))
        .show(ui, |ui| {
            for (index, (name, color)) in colors.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect(
                        rect,
                        CornerRadius::same(3),
                        *color,
                        Stroke::new(1.0, p.border_subtle),
                        egui::StrokeKind::Inside,
                    );
                    caption(ui, name);
                });
                if index % 4 == 3 {
                    ui.end_row();
                }
            }
        });
}

/// Stand-alone gallery window (`phpide --gallery`).
pub struct GalleryApp {
    state: GalleryState,
    theme: ThemeId,
    density: Density,
}

impl GalleryApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let app = GalleryApp {
            state: GalleryState::default(),
            theme: ThemeId::Dark,
            density: Density::Comfortable,
        };
        install(ctx, Theme::new(app.theme, app.density));
        app
    }
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let before = (self.theme, self.density);
        let p = current(&ctx).palette;
        egui::Panel::top("gallery-bar")
            .frame(
                egui::Frame::new()
                    .fill(p.bg_canvas)
                    .inner_margin(egui::Margin::same(space::S4 as i8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for id in ThemeId::ALL {
                        if Button::new(id.label())
                            .kind(if id == self.theme {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Ghost
                            })
                            .show(ui)
                            .clicked()
                        {
                            self.theme = id;
                        }
                    }
                    ui.add_space(space::S6);
                    for density in Density::ALL {
                        if Button::new(density.label())
                            .kind(if density == self.density {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Ghost
                            })
                            .show(ui)
                            .clicked()
                        {
                            self.density = density;
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg_canvas)
                    .inner_margin(egui::Margin::same(space::S6 as i8)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    show(ui, &mut self.state);
                });
            });
        if before != (self.theme, self.density) {
            install(&ctx, Theme::new(self.theme, self.density));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::snapshot;

    fn render(theme: ThemeId, density: Density) -> snapshot::Image {
        let ctx = egui::Context::default();
        install(&ctx, Theme::new(theme, density));
        let mut state = GalleryState::default();
        let mut height = 0.0;
        let size = egui::vec2(GALLERY_WIDTH + 2.0 * space::S6, 3200.0);
        let image = snapshot::render(&ctx, size, 3, |ui| {
            let p = current(ui.ctx()).palette;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(p.bg_canvas)
                        .inner_margin(egui::Margin::same(space::S6 as i8)),
                )
                .show(ui, |ui| height = show(ui, &mut state));
        });
        image.crop_height((height + 2.0 * space::S6).ceil() as usize)
    }

    /// Every theme × density matches its committed snapshot.
    #[test]
    fn gallery_snapshots() {
        for theme in ThemeId::ALL {
            for density in Density::ALL {
                let image = render(theme, density);
                assert!(image.height > 1000, "gallery too short: {}", image.height);
                let name = format!(
                    "gallery-{}-{}",
                    theme.name(),
                    density.label().to_ascii_lowercase()
                );
                snapshot::assert_snapshot(&name, &image);
            }
        }
    }
}
