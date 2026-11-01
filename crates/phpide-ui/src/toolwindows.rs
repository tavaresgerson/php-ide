//! Tool windows (paridade REQUISITOS 1.2): nomes e atalhos idênticos ao
//! PhpStorm — Project `Alt+1`, Find `Alt+3`, Run `Alt+4`, Debug `Alt+5`,
//! Problems `Alt+6`, Structure `Alt+7`, Hide All `Ctrl+Shift+F12`.
//!
//! Uma tool window pode ser ancorada (left/right/bottom); a barra de ícones
//! espelha as tool window bars da New UI.

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolWindow {
    Project,
    Find,
    Run,
    Debug,
    Problems,
    Structure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Right,
    Bottom,
}

impl ToolWindow {
    pub const ALL: [ToolWindow; 6] = [
        ToolWindow::Project,
        ToolWindow::Find,
        ToolWindow::Run,
        ToolWindow::Debug,
        ToolWindow::Problems,
        ToolWindow::Structure,
    ];

    /// Nome oficial exibido no header da tool window.
    pub fn title(self) -> &'static str {
        match self {
            ToolWindow::Project => "Project",
            ToolWindow::Find => "Find",
            ToolWindow::Run => "Run",
            ToolWindow::Debug => "Debug",
            ToolWindow::Problems => "Problems",
            ToolWindow::Structure => "Structure",
        }
    }

    /// Ícone (emoji leve; substituído por SVGs quando houver asset pipeline).
    pub fn icon(self) -> &'static str {
        match self {
            ToolWindow::Project => "📁",
            ToolWindow::Find => "🔍",
            ToolWindow::Run => "▶",
            ToolWindow::Debug => "🐞",
            ToolWindow::Problems => "⚠",
            ToolWindow::Structure => "🧭",
        }
    }

    pub fn shortcut_label(self) -> &'static str {
        match self {
            ToolWindow::Project => "Alt+1",
            ToolWindow::Find => "Alt+3",
            ToolWindow::Run => "Alt+4",
            ToolWindow::Debug => "Alt+5",
            ToolWindow::Problems => "Alt+6",
            ToolWindow::Structure => "Alt+7",
        }
    }

    pub fn anchor(self) -> Anchor {
        match self {
            ToolWindow::Project => Anchor::Left,
            ToolWindow::Structure => Anchor::Right,
            ToolWindow::Find | ToolWindow::Run | ToolWindow::Debug | ToolWindow::Problems => {
                Anchor::Bottom
            }
        }
    }

    /// Atalho Alt+dígito associado.
    pub fn from_digit(digit: u8) -> Option<ToolWindow> {
        match digit {
            1 => Some(ToolWindow::Project),
            3 => Some(ToolWindow::Find),
            4 => Some(ToolWindow::Run),
            5 => Some(ToolWindow::Debug),
            6 => Some(ToolWindow::Problems),
            7 => Some(ToolWindow::Structure),
            _ => None,
        }
    }
}

/// Estado de visibilidade das tool windows + "Hide All".
#[derive(Debug)]
pub struct ToolWindows {
    visible: [bool; 6],
    /// Tudo oculto por Ctrl+Shift+F12? Restaura com o mesmo atalho.
    pub all_hidden: bool,
    /// Visibilidade salva antes do Hide All (para restaurar).
    saved: Option<[bool; 6]>,
}

impl Default for ToolWindows {
    fn default() -> Self {
        // Project aberta por padrão (comportamento do PhpStorm ao abrir).
        let mut visible = [false; 6];
        visible[ToolWindow::Project as usize] = true;
        ToolWindows {
            visible,
            all_hidden: false,
            saved: None,
        }
    }
}

impl ToolWindows {
    pub fn is_visible(&self, tw: ToolWindow) -> bool {
        self.visible[tw as usize]
    }

    pub fn set_visible(&mut self, tw: ToolWindow, vis: bool) {
        self.visible[tw as usize] = vis;
        self.all_hidden = false;
    }

    pub fn toggle(&mut self, tw: ToolWindow) {
        self.set_visible(tw, !self.is_visible(tw));
    }

    /// Hide All Tool Windows / restaura (`Ctrl+Shift+F12`).
    pub fn toggle_hide_all(&mut self) {
        if self.all_hidden {
            if let Some(saved) = self.saved.take() {
                self.visible = saved;
            }
            self.all_hidden = false;
        } else {
            self.saved = Some(self.visible);
            self.visible = [false; 6];
            self.all_hidden = true;
        }
    }

    /// True se alguma tool window ancorada à esquerda está visível.
    pub fn any_left_visible(&self) -> bool {
        ToolWindow::ALL
            .iter()
            .any(|&t| t.anchor() == Anchor::Left && self.is_visible(t))
    }

    pub fn any_right_visible(&self) -> bool {
        ToolWindow::ALL
            .iter()
            .any(|&t| t.anchor() == Anchor::Right && self.is_visible(t))
    }

    /// Barra vertical de ícones estilo New UI (esquerda ou direita).
    pub fn bar(&mut self, ctx: &egui::Context, side: Anchor) {
        let any = match side {
            Anchor::Left => self.any_left_visible(),
            Anchor::Right => self.any_right_visible(),
            Anchor::Bottom => false,
        };
        // Só desenha a barra se houver ao menos uma tool window ancorada ali.
        let has_any = ToolWindow::ALL.iter().any(|&t| t.anchor() == side);
        if !has_any {
            return;
        }
        let id = match side {
            Anchor::Left => "tw-bar-left",
            Anchor::Right => "tw-bar-right",
            Anchor::Bottom => return,
        };
        egui::SidePanel::new(
            match side {
                Anchor::Left => egui::panel::Side::Left,
                _ => egui::panel::Side::Right,
            },
            id,
        )
        .resizable(false)
        .exact_width(30.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                for tw in ToolWindow::ALL {
                    if tw.anchor() != side {
                        continue;
                    }
                    let sel = self.is_visible(tw);
                    let resp = ui.selectable_label(sel, format!("{}", tw.icon()));
                    if resp.clicked() {
                        self.toggle(tw);
                    }
                    resp.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                }
                let _ = any;
            });
        });
    }

    /// Barra horizontal inferior (Run, Debug, Problems, Find) — base da New UI.
    /// Desenhada imediatamente acima da status bar.
    pub fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("tw-bar-bottom")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    for tw in ToolWindow::ALL {
                        if tw.anchor() != Anchor::Bottom {
                            continue;
                        }
                        let sel = self.is_visible(tw);
                        let resp = ui.selectable_label(
                            sel,
                            format!("{} {}", tw.icon(), tw.title()),
                        );
                        if resp.clicked() {
                            self.toggle(tw);
                        }
                        resp.on_hover_text(format!("{} ({})", tw.title(), tw.shortcut_label()));
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_map_to_tool_windows() {
        assert_eq!(ToolWindow::from_digit(1), Some(ToolWindow::Project));
        assert_eq!(ToolWindow::from_digit(7), Some(ToolWindow::Structure));
        assert_eq!(ToolWindow::from_digit(2), None);
    }

    #[test]
    fn hide_all_restores_previous_layout() {
        let mut tw = ToolWindows::default();
        tw.set_visible(ToolWindow::Project, true);
        tw.set_visible(ToolWindow::Problems, true);
        tw.toggle_hide_all();
        assert!(tw.all_hidden);
        assert!(!tw.is_visible(ToolWindow::Project));
        assert!(!tw.is_visible(ToolWindow::Problems));
        tw.toggle_hide_all();
        assert!(tw.is_visible(ToolWindow::Project));
        assert!(tw.is_visible(ToolWindow::Problems));
        assert!(!tw.all_hidden);
    }

    #[test]
    fn toggle_resets_all_hidden_flag() {
        let mut tw = ToolWindows::default();
        tw.set_visible(ToolWindow::Find, false);
        tw.toggle_hide_all();
        assert!(tw.all_hidden);
        tw.toggle(ToolWindow::Find);
        assert!(!tw.all_hidden);
        assert!(tw.is_visible(ToolWindow::Find));
    }
}
