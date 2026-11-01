//! Tool windows da New UI (REQUISITOS 1.2): catálogo, âncoras, atalhos,
//! barras laterais/inferior, More tool windows, Hide Active e Hide All.

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolWindow {
    Project,
    Bookmarks,
    Find,
    Run,
    Debug,
    Problems,
    Structure,
    Services,
    VersionControl,
    Commit,
    Terminal,
    Database,
    Notifications,
    RemoteHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Right,
    Bottom,
}

impl ToolWindow {
    pub const COUNT: usize = 14;
    pub const ALL: [ToolWindow; Self::COUNT] = [
        ToolWindow::Project,
        ToolWindow::Bookmarks,
        ToolWindow::Find,
        ToolWindow::Run,
        ToolWindow::Debug,
        ToolWindow::Problems,
        ToolWindow::Structure,
        ToolWindow::Services,
        ToolWindow::VersionControl,
        ToolWindow::Commit,
        ToolWindow::Terminal,
        ToolWindow::Database,
        ToolWindow::Notifications,
        ToolWindow::RemoteHost,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ToolWindow::Project => "Project",
            ToolWindow::Bookmarks => "Bookmarks",
            ToolWindow::Find => "Find",
            ToolWindow::Run => "Run",
            ToolWindow::Debug => "Debug",
            ToolWindow::Problems => "Problems",
            ToolWindow::Structure => "Structure",
            ToolWindow::Services => "Services",
            ToolWindow::VersionControl => "Version Control",
            ToolWindow::Commit => "Commit",
            ToolWindow::Terminal => "Terminal",
            ToolWindow::Database => "Database",
            ToolWindow::Notifications => "Notifications",
            ToolWindow::RemoteHost => "Remote Host",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ToolWindow::Project => "P",
            ToolWindow::Bookmarks => "B",
            ToolWindow::Find => "F",
            ToolWindow::Run => "R",
            ToolWindow::Debug => "D",
            ToolWindow::Problems => "!",
            ToolWindow::Structure => "S",
            ToolWindow::Services => "Sv",
            ToolWindow::VersionControl => "G",
            ToolWindow::Commit => "C",
            ToolWindow::Terminal => ">_",
            ToolWindow::Database => "DB",
            ToolWindow::Notifications => "N",
            ToolWindow::RemoteHost => "RH",
        }
    }

    pub fn shortcut_label(self) -> &'static str {
        match self {
            ToolWindow::Project => "Alt+1",
            ToolWindow::Bookmarks => "Alt+2",
            ToolWindow::Find => "Alt+3",
            ToolWindow::Run => "Alt+4",
            ToolWindow::Debug => "Alt+5",
            ToolWindow::Problems => "Alt+6",
            ToolWindow::Structure => "Alt+7",
            ToolWindow::Services => "Alt+8",
            ToolWindow::VersionControl => "Alt+9",
            ToolWindow::Commit => "Alt+0",
            ToolWindow::Terminal => "Alt+F12",
            ToolWindow::Database => "Ctrl+Alt+3",
            ToolWindow::Notifications | ToolWindow::RemoteHost => "—",
        }
    }

    pub fn anchor(self) -> Anchor {
        match self {
            ToolWindow::Project | ToolWindow::Bookmarks | ToolWindow::Services => Anchor::Left,
            ToolWindow::Structure | ToolWindow::RemoteHost => Anchor::Right,
            _ => Anchor::Bottom,
        }
    }

    pub fn from_digit(digit: u8) -> Option<ToolWindow> {
        match digit {
            0 => Some(ToolWindow::Commit),
            1 => Some(ToolWindow::Project),
            2 => Some(ToolWindow::Bookmarks),
            3 => Some(ToolWindow::Find),
            4 => Some(ToolWindow::Run),
            5 => Some(ToolWindow::Debug),
            6 => Some(ToolWindow::Problems),
            7 => Some(ToolWindow::Structure),
            8 => Some(ToolWindow::Services),
            9 => Some(ToolWindow::VersionControl),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ToolWindows {
    visible: [bool; ToolWindow::COUNT],
    pub all_hidden: bool,
    saved: Option<[bool; ToolWindow::COUNT]>,
    active: Option<ToolWindow>,
}

impl Default for ToolWindows {
    fn default() -> Self {
        let mut visible = [false; ToolWindow::COUNT];
        visible[ToolWindow::Project as usize] = true;
        ToolWindows {
            visible,
            all_hidden: false,
            saved: None,
            active: Some(ToolWindow::Project),
        }
    }
}

impl ToolWindows {
    pub fn is_visible(&self, tw: ToolWindow) -> bool {
        self.visible[tw as usize]
    }

    pub fn set_visible(&mut self, tw: ToolWindow, visible: bool) {
        self.visible[tw as usize] = visible;
        self.all_hidden = false;
        if visible {
            self.active = Some(tw);
        } else if self.active == Some(tw) {
            self.active = None;
        }
    }

    pub fn toggle(&mut self, tw: ToolWindow) {
        self.set_visible(tw, !self.is_visible(tw));
    }

    pub fn hide_active(&mut self) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        self.set_visible(active, false);
        true
    }

    pub fn toggle_hide_all(&mut self) {
        if self.all_hidden {
            if let Some(saved) = self.saved.take() {
                self.visible = saved;
            }
            self.all_hidden = false;
        } else {
            self.saved = Some(self.visible);
            self.visible = [false; ToolWindow::COUNT];
            self.active = None;
            self.all_hidden = true;
        }
    }

    pub fn bar(&mut self, ctx: &egui::Context, side: Anchor, compact: bool) {
        if !ToolWindow::ALL.iter().any(|&tw| tw.anchor() == side) || side == Anchor::Bottom {
            return;
        }
        let id = if side == Anchor::Left {
            "tw-bar-left"
        } else {
            "tw-bar-right"
        };
        egui::SidePanel::new(
            if side == Anchor::Left {
                egui::panel::Side::Left
            } else {
                egui::panel::Side::Right
            },
            id,
        )
        .resizable(false)
        .exact_width(if compact { 26.0 } else { 32.0 })
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(if compact { 2.0 } else { 5.0 });
                for tw in ToolWindow::ALL {
                    if tw.anchor() != side {
                        continue;
                    }
                    let selected = self.is_visible(tw);
                    let response = ui.selectable_label(selected, tw.icon());
                    if response.clicked() {
                        self.toggle(tw);
                    }
                    response.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                }
                ui.menu_button("...", |ui| {
                    ui.label(egui::RichText::new("More tool windows").weak().small());
                    for tw in ToolWindow::ALL {
                        let mut visible = self.is_visible(tw);
                        if ui.checkbox(&mut visible, tw.title()).changed() {
                            self.set_visible(tw, visible);
                        }
                    }
                });
            });
        });
    }

    pub fn bottom_bar(&mut self, ctx: &egui::Context, compact: bool) {
        const PRIMARY: [ToolWindow; 5] = [
            ToolWindow::Find,
            ToolWindow::Run,
            ToolWindow::Debug,
            ToolWindow::Problems,
            ToolWindow::Terminal,
        ];
        egui::TopBottomPanel::bottom("tw-bar-bottom")
            .exact_height(if compact { 24.0 } else { 30.0 })
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    for tw in PRIMARY {
                        let selected = self.is_visible(tw);
                        let response =
                            ui.selectable_label(selected, format!("{} {}", tw.icon(), tw.title()));
                        if response.clicked() {
                            self.toggle(tw);
                        }
                        response.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                    }
                    ui.menu_button("... More tool windows", |ui| {
                        for tw in ToolWindow::ALL {
                            if PRIMARY.contains(&tw) {
                                continue;
                            }
                            let mut visible = self.is_visible(tw);
                            let label =
                                format!("{} {}\t{}", tw.icon(), tw.title(), tw.shortcut_label());
                            if ui.checkbox(&mut visible, label).changed() {
                                self.set_visible(tw, visible);
                            }
                        }
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_match_phpstorm_catalog() {
        let expected = [
            ToolWindow::Commit,
            ToolWindow::Project,
            ToolWindow::Bookmarks,
            ToolWindow::Find,
            ToolWindow::Run,
            ToolWindow::Debug,
            ToolWindow::Problems,
            ToolWindow::Structure,
            ToolWindow::Services,
            ToolWindow::VersionControl,
        ];
        for (digit, tw) in expected.into_iter().enumerate() {
            assert_eq!(ToolWindow::from_digit(digit as u8), Some(tw));
        }
        assert_eq!(ToolWindow::Database.shortcut_label(), "Ctrl+Alt+3");
        assert_eq!(ToolWindow::Terminal.shortcut_label(), "Alt+F12");
    }

    #[test]
    fn hide_all_restores_previous_layout() {
        let mut tw = ToolWindows::default();
        tw.set_visible(ToolWindow::Problems, true);
        tw.toggle_hide_all();
        assert!(tw.all_hidden);
        assert!(!tw.is_visible(ToolWindow::Project));
        tw.toggle_hide_all();
        assert!(tw.is_visible(ToolWindow::Project));
        assert!(tw.is_visible(ToolWindow::Problems));
    }

    #[test]
    fn hide_active_only_closes_current_window() {
        let mut tw = ToolWindows::default();
        tw.set_visible(ToolWindow::Problems, true);
        assert!(tw.hide_active());
        assert!(!tw.is_visible(ToolWindow::Problems));
        assert!(tw.is_visible(ToolWindow::Project));
    }
}
