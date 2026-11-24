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

/// Where a tool window is shown: docked on a side or floating (RF-UI-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Docked(Anchor),
    Floating,
}

impl Placement {
    pub const ALL: [Placement; 4] = [
        Placement::Docked(Anchor::Left),
        Placement::Docked(Anchor::Right),
        Placement::Docked(Anchor::Bottom),
        Placement::Floating,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Placement::Docked(Anchor::Left) => "Left",
            Placement::Docked(Anchor::Right) => "Right",
            Placement::Docked(Anchor::Bottom) => "Bottom",
            Placement::Floating => "Floating",
        }
    }

    pub fn from_label(label: &str) -> Option<Placement> {
        Placement::ALL.into_iter().find(|p| p.label() == label)
    }
}

/// Shows a tool window's contents where its placement says: a resizable
/// side or bottom panel, or a floating window.
pub fn dock(
    ui: &mut egui::Ui,
    tw: ToolWindow,
    placement: Placement,
    default_size: f32,
    body: impl FnOnce(&mut egui::Ui),
) {
    let id = format!("tool-window-{}", tw.title());
    match placement {
        Placement::Docked(Anchor::Left) => {
            egui::Panel::left(id)
                .resizable(true)
                .default_size(default_size.min(320.0))
                .show(ui, body);
        }
        Placement::Docked(Anchor::Right) => {
            egui::Panel::right(id)
                .resizable(true)
                .default_size(default_size.min(320.0))
                .show(ui, body);
        }
        Placement::Docked(Anchor::Bottom) => {
            egui::Panel::bottom(id)
                .resizable(true)
                .default_size(default_size.clamp(160.0, 320.0))
                .show(ui, body);
        }
        Placement::Floating => {
            egui::Window::new(tw.title())
                .id(egui::Id::new(id))
                .title_bar(false)
                .resizable(true)
                .default_size([420.0, 320.0])
                .frame(crate::design::components::Dialog::frame(ui))
                .show(ui.ctx(), |ui| {
                    ui.set_min_size(egui::vec2(260.0, 160.0));
                    body(ui)
                });
        }
    }
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
    /// Last tool window shown (F12 brings it back).
    last: Option<ToolWindow>,
    /// Placements the user chose ("Move To"), by tool window.
    placements: [Option<Placement>; ToolWindow::COUNT],
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
            last: Some(ToolWindow::Project),
            placements: [None; ToolWindow::COUNT],
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
            self.last = Some(tw);
        } else if self.active == Some(tw) {
            self.active = None;
        }
    }

    pub fn placement(&self, tw: ToolWindow) -> Placement {
        self.placements[tw as usize].unwrap_or(Placement::Docked(tw.anchor()))
    }

    pub fn set_placement(&mut self, tw: ToolWindow, placement: Placement) {
        self.placements[tw as usize] =
            (placement != Placement::Docked(tw.anchor())).then_some(placement);
    }

    /// `(title, placement)` of every moved tool window (settings).
    pub fn placements(&self) -> Vec<(String, String)> {
        ToolWindow::ALL
            .iter()
            .filter_map(|tw| {
                self.placements[*tw as usize]
                    .map(|p| (tw.title().to_string(), p.label().to_string()))
            })
            .collect()
    }

    pub fn restore_placements(&mut self, saved: &[(String, String)]) {
        for (title, label) in saved {
            let tw = ToolWindow::ALL.iter().find(|tw| tw.title() == title);
            if let (Some(tw), Some(p)) = (tw, Placement::from_label(label)) {
                self.set_placement(*tw, p);
            }
        }
    }

    /// Whether `tw` lives on `side` (its rail or the bottom bar).
    fn on_side(&self, tw: ToolWindow, side: Anchor) -> bool {
        match self.placement(tw) {
            Placement::Docked(anchor) => anchor == side,
            Placement::Floating => tw.anchor() == side,
        }
    }

    /// Shows the last tool window (default Project) and returns it.
    pub fn show_last(&mut self) -> ToolWindow {
        let tw = self.last.unwrap_or(ToolWindow::Project);
        self.set_visible(tw, true);
        tw
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
        if !ToolWindow::ALL.iter().any(|&tw| self.on_side(tw, side)) || side == Anchor::Bottom {
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
                    if !self.on_side(tw, side) || !tw.is_available() {
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
        // Bottom tool windows (and the ones moved there), then "More".
        let primary: Vec<ToolWindow> = ToolWindow::ALL
            .into_iter()
            .filter(|tw| tw.is_available() && self.on_side(*tw, Anchor::Bottom))
            .filter(|tw| {
                matches!(
                    tw,
                    ToolWindow::Find
                        | ToolWindow::Run
                        | ToolWindow::Debug
                        | ToolWindow::Problems
                        | ToolWindow::Terminal
                ) || self.placements[*tw as usize].is_some()
            })
            .collect();
        egui::Panel::bottom("tw-bar-bottom")
            .exact_size(if compact { 24.0 } else { 30.0 })
            .show(ui, |ui| {
                crate::design::components::ghost_controls(ui);
                ui.horizontal_centered(|ui| {
                    for tw in primary.iter().copied() {
                        let selected = self.is_visible(tw);
                        let response = ui.selectable_label(selected, tw.title());
                        if response.clicked() {
                            self.toggle(tw);
                        }
                        response.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                    }
                    ui.menu_button("... More tool windows", |ui| {
                        for tw in ToolWindow::ALL {
                            if primary.contains(&tw) || !tw.is_available() {
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
