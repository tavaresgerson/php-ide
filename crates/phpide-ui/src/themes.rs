//! Temas da UI (paridade PhpStorm New UI — REQUISITOS 1.7).
//!
//! A "New UI" da JetBrains tem como padrão a família **Islands**; aqui os ids
//! seguem os nomes oficiais e cada tema ajusta o `egui::Style` (cor de acento
//! e arredondamento) além do modo claro/escuro de base.

use eframe::egui;

/// Azul "New UI" do PhpStorm (accent dos botões/seleção).
const ISLANDS_ACCENT: egui::Color32 = egui::Color32::from_rgb(0x35, 0x74, 0xF0);
/// Darcula clássico: acento laranja.
const DARCULA_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xCB, 0x7C, 0x3B);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    IslandsDark,
    IslandsLight,
    Darcula,
    Light,
}

impl ThemeId {
    pub const ALL: [ThemeId; 4] = [
        ThemeId::IslandsDark,
        ThemeId::IslandsLight,
        ThemeId::Darcula,
        ThemeId::Light,
    ];

    pub fn from_name(name: &str) -> ThemeId {
        match name {
            "islands-dark" => ThemeId::IslandsDark,
            "islands-light" => ThemeId::IslandsLight,
            "darcula" => ThemeId::Darcula,
            "light" => ThemeId::Light,
            _ => ThemeId::IslandsDark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::IslandsDark => "islands-dark",
            ThemeId::IslandsLight => "islands-light",
            ThemeId::Darcula => "darcula",
            ThemeId::Light => "light",
        }
    }

    /// Rótulo exibido no Quick Switch Scheme / Settings.
    pub fn label(self) -> &'static str {
        match self {
            ThemeId::IslandsDark => "Islands Dark",
            ThemeId::IslandsLight => "Islands Light",
            ThemeId::Darcula => "Islands Darcula",
            ThemeId::Light => "Light",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeId::IslandsDark | ThemeId::Darcula)
    }
}

/// Aplica o tema ao contexto: modo base + acento/arredondamento da família.
pub fn apply(ctx: &egui::Context, theme: ThemeId) {
    ctx.set_theme(if theme.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let accent = match theme {
        ThemeId::Darcula => DARCULA_ACCENT,
        _ => ISLANDS_ACCENT,
    };
    ctx.style_mut(|style| {
        style.visuals.selection.bg_fill = accent.gamma_multiply(0.45);
        style.visuals.hyperlink_color = accent;
        let widgets = &mut style.visuals.widgets;
        widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, accent.gamma_multiply(0.6));
        widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, accent);
        widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, accent);
        // New UI: cantos levemente arredondados.
        let r = egui::CornerRadius::same(4);
        widgets.inactive.corner_radius = r;
        widgets.hovered.corner_radius = r;
        widgets.active.corner_radius = r;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_roundtrip() {
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::from_name(t.name()), t);
        }
    }

    #[test]
    fn legacy_names_fall_back_to_islands_dark() {
        assert_eq!(ThemeId::from_name("dark"), ThemeId::IslandsDark);
        assert_eq!(ThemeId::from_name("?"), ThemeId::IslandsDark);
    }
}
