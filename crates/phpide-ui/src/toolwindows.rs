//! Tool windows: catalog, anchors, shortcuts, side/bottom bars, More tool
//! windows, Hide Active and Hide All.

use eframe::egui;
use phpide_icons::IconId;

use crate::design::components::IconButton;

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

    pub fn icon(self) -> IconId {
        match self {
            ToolWindow::Project => IconId::Project,
            ToolWindow::Bookmarks => IconId::Bookmark,
            ToolWindow::Find => IconId::Search,
            ToolWindow::Run => IconId::Run,
            ToolWindow::Debug => IconId::Debug,
            ToolWindow::Problems => IconId::Problems,
            ToolWindow::Structure => IconId::Structure,
            ToolWindow::Services => IconId::Services,
            ToolWindow::VersionControl => IconId::VersionControl,
            ToolWindow::Commit => IconId::Commit,
            ToolWindow::Terminal => IconId::Terminal,
            ToolWindow::Database => IconId::Database,
            ToolWindow::Notifications => IconId::Notifications,
            ToolWindow::RemoteHost => IconId::RemoteHost,
        }
    }

    /// Shortcut text from the command registry (empty when none).
    pub fn shortcut_label(self) -> String {
        crate::commands::shortcut_text(crate::commands::Command::ToggleToolWindow(self))
    }

    pub fn anchor(self) -> Anchor {
        match self {
            ToolWindow::Project | ToolWindow::Bookmarks | ToolWindow::Services => Anchor::Left,
            ToolWindow::Structure | ToolWindow::RemoteHost => Anchor::Right,
            _ => Anchor::Bottom,
        }
    }

    /// Whether the tool window belongs to the v1.0 scope. Frozen windows are
    /// only reachable with the `experimental` feature.
    pub fn is_available(self) -> bool {
        match self {
            ToolWindow::Services | ToolWindow::Database | ToolWindow::RemoteHost => {
                crate::scope::EXPERIMENTAL
            }
            _ => true,
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
        if visible && !tw.is_available() {
            return;
        }
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

    pub fn bar(&mut self, ui: &mut egui::Ui, side: Anchor, compact: bool) {
        if !ToolWindow::ALL.iter().any(|&tw| tw.anchor() == side) || side == Anchor::Bottom {
            return;
        }
        let id = if side == Anchor::Left {
            "tw-bar-left"
        } else {
            "tw-bar-right"
        };
        let panel = if side == Anchor::Left {
            egui::Panel::left(id)
        } else {
            egui::Panel::right(id)
        };
        let rail = crate::design::current(ui.ctx()).metrics.tool_window_rail;
        panel.resizable(false).exact_size(rail).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(if compact { 4.0 } else { 6.0 });
                for tw in ToolWindow::ALL {
                    if tw.anchor() != side || !tw.is_available() {
                        continue;
                    }
                    let response = IconButton::new(tw.icon(), tw.title())
                        .shortcut(tw.shortcut_label())
                        .selected(self.is_visible(tw))
                        .show(ui);
                    if response.clicked() {
                        self.toggle(tw);
                    }
                }
                let more = IconButton::new(IconId::More, "More tool windows").show(ui);
                egui::Popup::menu(&more).show(|ui| {
                    ui.label(egui::RichText::new("More tool windows").weak().small());
                    for tw in ToolWindow::ALL {
                        if !tw.is_available() {
                            continue;
                        }
                        let mut visible = self.is_visible(tw);
                        if ui.checkbox(&mut visible, tw.title()).changed() {
                            self.set_visible(tw, visible);
                        }
                    }
                });
            });
        });
    }

    pub fn bottom_bar(&mut self, ui: &mut egui::Ui, compact: bool) {
        const PRIMARY: [ToolWindow; 5] = [
            ToolWindow::Find,
            ToolWindow::Run,
            ToolWindow::Debug,
            ToolWindow::Problems,
            ToolWindow::Terminal,
        ];
        egui::Panel::bottom("tw-bar-bottom")
            .exact_size(if compact { 24.0 } else { 30.0 })
            .show(ui, |ui| {
                crate::design::components::ghost_controls(ui);
                ui.horizontal_centered(|ui| {
                    for tw in PRIMARY {
                        let selected = self.is_visible(tw);
                        let response = ui.selectable_label(selected, tw.title());
                        if response.clicked() {
                            self.toggle(tw);
                        }
                        response.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                    }
                    ui.menu_button("... More tool windows", |ui| {
                        for tw in ToolWindow::ALL {
                            if PRIMARY.contains(&tw) || !tw.is_available() {
                                continue;
                            }
                            let mut visible = self.is_visible(tw);
                            let label = format!("{}\t{}", tw.title(), tw.shortcut_label());
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
    fn shortcut_labels_come_from_the_registry() {
        assert_eq!(ToolWindow::Project.shortcut_label(), "Alt+1");
        assert_eq!(ToolWindow::Commit.shortcut_label(), "Alt+0");
        assert_eq!(ToolWindow::Database.shortcut_label(), "Ctrl+Alt+3");
        assert_eq!(ToolWindow::Terminal.shortcut_label(), "Alt+F12");
        assert_eq!(ToolWindow::Notifications.shortcut_label(), "");
    }

    #[test]
    fn frozen_tool_windows_only_open_with_experimental_feature() {
        let mut tw = ToolWindows::default();
        for frozen in [
            ToolWindow::Services,
            ToolWindow::Database,
            ToolWindow::RemoteHost,
        ] {
            tw.set_visible(frozen, true);
            assert_eq!(tw.is_visible(frozen), crate::scope::EXPERIMENTAL);
        }
        tw.set_visible(ToolWindow::Terminal, true);
        assert!(tw.is_visible(ToolWindow::Terminal));
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
