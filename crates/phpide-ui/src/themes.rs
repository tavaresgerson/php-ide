//! Temas da UI (paridade PhpStorm New UI — REQUISITOS 1.7).
//!
//! A família Islands é o padrão. Os ids são persistentes e os rótulos exibidos
//! seguem os nomes oficiais; cada tema ajusta modo base, acento, contraste e
//! arredondamento do `egui::Style`.

use eframe::egui;

const ISLANDS_ACCENT: egui::Color32 = egui::Color32::from_rgb(0x35, 0x74, 0xF0);
const DARCULA_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xCB, 0x7C, 0x3B);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    IslandsDark,
    IslandsLight,
    IslandsDarcula,
    Dark,
    Light,
    HighContrast,
    Darcula,
}

impl ThemeId {
    pub const ALL: [ThemeId; 7] = [
        ThemeId::IslandsDark,
        ThemeId::IslandsLight,
        ThemeId::IslandsDarcula,
        ThemeId::Dark,
        ThemeId::Light,
        ThemeId::HighContrast,
        ThemeId::Darcula,
    ];

    pub fn from_name(name: &str) -> ThemeId {
        match name {
            "islands-dark" => ThemeId::IslandsDark,
            "islands-light" => ThemeId::IslandsLight,
            "islands-darcula" => ThemeId::IslandsDarcula,
            "dark-classic" => ThemeId::Dark,
            "light-classic" | "light" => ThemeId::Light,
            "high-contrast" => ThemeId::HighContrast,
            "darcula" => ThemeId::Darcula,
            _ => ThemeId::IslandsDark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::IslandsDark => "islands-dark",
            ThemeId::IslandsLight => "islands-light",
            ThemeId::IslandsDarcula => "islands-darcula",
            ThemeId::Dark => "dark-classic",
            ThemeId::Light => "light-classic",
            ThemeId::HighContrast => "high-contrast",
            ThemeId::Darcula => "darcula",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::IslandsDark => "Islands Dark",
            ThemeId::IslandsLight => "Islands Light",
            ThemeId::IslandsDarcula => "Islands Darcula",
            ThemeId::Dark => "Dark",
            ThemeId::Light => "Light",
            ThemeId::HighContrast => "High Contrast",
            ThemeId::Darcula => "Darcula",
        }
    }

    pub fn is_dark(self) -> bool {
        !matches!(self, ThemeId::IslandsLight | ThemeId::Light)
    }
}

pub fn apply(ctx: &egui::Context, theme: ThemeId) {
    ctx.set_theme(if theme.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let accent = match theme {
        ThemeId::IslandsDarcula | ThemeId::Darcula => DARCULA_ACCENT,
        ThemeId::HighContrast => egui::Color32::YELLOW,
        _ => ISLANDS_ACCENT,
    };
    ctx.style_mut(|style| {
        style.visuals.selection.bg_fill = accent.gamma_multiply(0.45);
        style.visuals.hyperlink_color = accent;
        if theme == ThemeId::HighContrast {
            style.visuals.panel_fill = egui::Color32::BLACK;
            style.visuals.window_fill = egui::Color32::BLACK;
            style.visuals.extreme_bg_color = egui::Color32::BLACK;
            style.visuals.override_text_color = Some(egui::Color32::WHITE);
        }
        let widgets = &mut style.visuals.widgets;
        widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, accent.gamma_multiply(0.6));
        widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, accent);
        widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, accent);
        let radius = if matches!(
            theme,
            ThemeId::IslandsDark | ThemeId::IslandsLight | ThemeId::IslandsDarcula
        ) {
            4
        } else {
            2
        };
        let r = egui::CornerRadius::same(radius);
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
        for theme in ThemeId::ALL {
            assert_eq!(ThemeId::from_name(theme.name()), theme);
        }
    }

    #[test]
    fn official_theme_catalog_is_complete() {
        let labels: Vec<_> = ThemeId::ALL.iter().map(|theme| theme.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Islands Dark",
                "Islands Light",
                "Islands Darcula",
                "Dark",
                "Light",
                "High Contrast",
                "Darcula",
            ]
        );
    }

    #[test]
    fn unknown_names_fall_back_to_islands_dark() {
        assert_eq!(ThemeId::from_name("?"), ThemeId::IslandsDark);
    }
}
