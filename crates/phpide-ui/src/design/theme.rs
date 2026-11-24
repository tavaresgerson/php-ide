//! Themes: Dark, Light and High Contrast, each with its own palette and
//! syntax colors, applied to `egui::Style` and kept in the context so any
//! code can read the active tokens with [`current`].

use std::sync::Arc;

use eframe::egui::{
    self, epaint::Shadow, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2,
};

use super::fonts;
use super::tokens::{self, radius, space, typography, Density, Metrics, Palette, SyntaxPalette};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeId {
    Dark,
    Light,
    HighContrast,
}

impl ThemeId {
    pub const ALL: [ThemeId; 3] = [ThemeId::Dark, ThemeId::Light, ThemeId::HighContrast];

    /// Reads a stored id. Ids of the old theme catalog (Islands, Darcula,
    /// classic) map to the closest current theme.
    pub fn from_name(name: &str) -> ThemeId {
        match name {
            "light" | "islands-light" | "light-classic" => ThemeId::Light,
            "high-contrast" => ThemeId::HighContrast,
            _ => ThemeId::Dark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Dark => "dark",
            ThemeId::Light => "light",
            ThemeId::HighContrast => "high-contrast",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Dark => "Dark",
            ThemeId::Light => "Light",
            ThemeId::HighContrast => "High Contrast",
        }
    }

    pub fn is_dark(self) -> bool {
        self != ThemeId::Light
    }

    pub fn palette(self) -> Palette {
        match self {
            ThemeId::Dark => tokens::DARK,
            ThemeId::Light => tokens::LIGHT,
            ThemeId::HighContrast => tokens::HIGH_CONTRAST,
        }
    }

    pub fn syntax(self) -> SyntaxPalette {
        match self {
            ThemeId::Dark => tokens::DARK_SYNTAX,
            ThemeId::Light => tokens::LIGHT_SYNTAX,
            ThemeId::HighContrast => tokens::HIGH_CONTRAST_SYNTAX,
        }
    }
}

/// Everything a component needs to draw itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub density: Density,
    pub palette: Palette,
    pub syntax: SyntaxPalette,
    pub metrics: Metrics,
    pub editor_font_size: f32,
    /// Code font family (empty: the bundled JetBrains Mono).
    pub editor_font_family: String,
    /// Row height as a multiple of the font height.
    pub editor_line_height: f32,
    /// User changes to the color scheme (see `crate::color_scheme`).
    pub color_changes: std::sync::Arc<serde_json::Value>,
    pub reduced_motion: bool,
}

impl Theme {
    pub fn new(id: ThemeId, density: Density) -> Theme {
        Theme {
            id,
            density,
            palette: id.palette(),
            syntax: id.syntax(),
            metrics: density.metrics(),
            editor_font_size: typography::EDITOR,
            editor_font_family: String::new(),
            editor_line_height: 1.2,
            color_changes: Default::default(),
            reduced_motion: false,
        }
    }

    pub fn editor_font(&self) -> FontId {
        FontId::monospace(self.editor_font_size)
    }

    pub fn gutter_font(&self) -> FontId {
        FontId::monospace(typography::GUTTER)
    }

    /// Theme colors of code, before the user's changes.
    pub fn base_syntax_style(&self) -> phpide_php::SyntaxStyle {
        let mut style = self.syntax.style(self.editor_font());
        style.bold_font = Some(FontId::new(
            self.editor_font_size,
            super::fonts::family(super::fonts::MONO_BOLD),
        ));
        style
    }

    /// Code colors for `language` (a file type id): theme, then the user's
    /// Language Defaults, then the user's changes for that language.
    pub fn syntax_style_for(&self, language: Option<&str>) -> phpide_php::SyntaxStyle {
        crate::color_scheme::resolve(
            &self.base_syntax_style(),
            &self.color_changes,
            self.id.name(),
            language,
        )
    }

    pub fn syntax_style(&self) -> phpide_php::SyntaxStyle {
        self.syntax_style_for(None)
    }

    /// Colors and fonts of the code editor.
    pub fn editor_style(&self) -> phpide_editor::EditorStyle {
        let p = &self.palette;
        phpide_editor::EditorStyle {
            syntax: self.syntax_style(),
            gutter_font: self.gutter_font(),
            ui_font: FontId::proportional(typography::BODY),
            line_spacing: self.editor_line_height.clamp(1.0, 2.0),
            colors: phpide_editor::EditorColors {
                background: p.bg_editor,
                gutter_background: p.bg_editor,
                gutter_text: p.editor_gutter_text,
                gutter_text_current: p.editor_gutter_text_current,
                current_line: p.editor_current_line_bg,
                selection: p.editor_selection_bg,
                caret: p.editor_caret,
                indent_guide: p.editor_indent_guide,
                fold_placeholder: p.editor_fold_bg,
                fold_placeholder_text: p.text_secondary,
                fold_marker: p.text_muted,
                search_match: p.search_match_bg,
                search_current: p.search_current_match_bg,
                occurrence: p.search_match_bg,
                error: p.error,
                warning: p.warning,
                info: p.info,
                hint: p.text_muted,
                git_added: p.git_added,
                git_modified: p.git_modified,
                git_deleted: p.git_deleted,
                breakpoint: p.debug_breakpoint,
                breakpoint_check: p.text_on_accent,
                bookmark: p.warning,
                execution_line: p.debug_current_line,
                coverage_full: p.coverage_full,
                coverage_partial: p.coverage_partial,
                coverage_none: p.coverage_none,
                sticky_background: p.editor_sticky_bg,
                sticky_border: p.border_subtle,
                scrollbar_thumb: p.border_strong,
                scrollbar_thumb_hover: p.text_muted,
                inlay_background: p.bg_hover,
                inlay_text: p.text_muted,
                blame_text: p.text_muted,
                panel_background: p.bg_elevated,
                panel_border: p.border_subtle,
                text: p.text_primary,
                text_muted: p.text_muted,
                accent: p.accent,
            },
        }
    }

    /// Animation time, or zero with Reduced Motion.
    pub fn motion(&self, seconds: f32) -> f32 {
        if self.reduced_motion {
            0.0
        } else {
            seconds
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::new(ThemeId::Dark, Density::Comfortable)
    }
}

fn storage_id() -> egui::Id {
    egui::Id::new("phpide-design-theme")
}

/// Active theme (Dark/Comfortable until [`install`] runs).
pub fn current(ctx: &egui::Context) -> Arc<Theme> {
    ctx.data(|data| data.get_temp::<Arc<Theme>>(storage_id()))
        .unwrap_or_default()
}

/// Installs fonts once and applies the theme to the context style.
pub fn install(ctx: &egui::Context, theme: Theme) {
    let fonts_installed = egui::Id::new("phpide-design-fonts");
    let installed: Option<String> = ctx.data(|data| data.get_temp(fonts_installed));
    if installed.as_deref() != Some(theme.editor_font_family.as_str()) {
        fonts::install_with_editor_family(ctx, &theme.editor_font_family);
        ctx.data_mut(|data| data.insert_temp(fonts_installed, theme.editor_font_family.clone()));
    }
    let egui_theme = if theme.id.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(egui_theme);
    let style = style_for(&theme, &ctx.style_of(egui_theme));
    ctx.set_style_of(egui_theme, style);
    ctx.data_mut(|data| data.insert_temp(storage_id(), Arc::new(theme)));
}

/// The egui style of a theme, starting from `base` (keeps unrelated
/// options such as debug flags).
pub fn style_for(theme: &Theme, base: &egui::Style) -> egui::Style {
    let p = &theme.palette;
    let m = &theme.metrics;
    let mut style = base.clone();
    let mut visuals = if theme.id.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let control = CornerRadius::same(radius::CONTROL);
    visuals.override_text_color = None;
    visuals.weak_text_color = Some(p.text_muted);
    visuals.hyperlink_color = p.accent;
    visuals.faint_bg_color = p.bg_hover;
    visuals.extreme_bg_color = p.bg_inset;
    visuals.text_edit_bg_color = Some(p.bg_editor);
    visuals.code_bg_color = p.bg_inset;
    visuals.warn_fg_color = p.warning;
    visuals.error_fg_color = p.error;
    visuals.panel_fill = p.bg_panel;
    visuals.window_fill = p.bg_elevated;
    visuals.window_stroke = Stroke::new(1.0, p.border_subtle);
    visuals.window_corner_radius = CornerRadius::same(radius::DIALOG);
    visuals.menu_corner_radius = CornerRadius::same(radius::CARD);
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: p.shadow,
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: p.shadow,
    };
    visuals.selection.bg_fill = p.editor_selection_bg;
    visuals.selection.stroke = Stroke::new(1.0, p.text_primary);
    visuals.text_cursor.stroke = Stroke::new(2.0, p.editor_caret);
    visuals.striped = false;
    visuals.indent_has_left_vline = false;
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.slider_trailing_fill = true;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = p.bg_panel;
    widgets.noninteractive.weak_bg_fill = p.bg_panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border_subtle);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_primary);
    widgets.noninteractive.corner_radius = control;
    // Resting controls: quiet fill, subtle border — never the accent.
    widgets.inactive.bg_fill = p.bg_inset;
    widgets.inactive.weak_bg_fill = p.bg_elevated;
    widgets.inactive.bg_stroke = Stroke::new(1.0, p.border_subtle);
    widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    widgets.inactive.corner_radius = control;
    widgets.inactive.expansion = 0.0;
    widgets.hovered.bg_fill = p.bg_hover;
    widgets.hovered.weak_bg_fill = p.bg_hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    widgets.hovered.fg_stroke = Stroke::new(1.0, p.text_primary);
    widgets.hovered.corner_radius = control;
    widgets.hovered.expansion = 0.0;
    widgets.active.bg_fill = p.bg_pressed;
    widgets.active.weak_bg_fill = p.bg_pressed;
    widgets.active.bg_stroke = Stroke::new(1.0, p.border_strong);
    widgets.active.fg_stroke = Stroke::new(1.0, p.text_primary);
    widgets.active.corner_radius = control;
    widgets.active.expansion = 0.0;
    widgets.open.bg_fill = p.bg_selected;
    widgets.open.weak_bg_fill = p.bg_selected;
    widgets.open.bg_stroke = Stroke::new(1.0, p.border_strong);
    widgets.open.fg_stroke = Stroke::new(1.0, p.text_primary);
    widgets.open.corner_radius = control;
    style.visuals = visuals;

    let spacing = &mut style.spacing;
    spacing.item_spacing = Vec2::new(m.gap, m.gap * 0.5 + space::S1);
    spacing.button_padding = Vec2::new(space::S4, space::S1);
    spacing.interact_size = Vec2::new(m.min_target, m.min_target);
    spacing.menu_margin = Margin::same(m.popover_padding as i8);
    spacing.window_margin = Margin::same(m.dialog_padding as i8);
    spacing.indent = if theme.density == Density::Compact {
        14.0
    } else {
        16.0
    };
    spacing.icon_width = 14.0;
    spacing.icon_width_inner = 8.0;
    spacing.icon_spacing = space::S3;
    spacing.combo_height = 320.0;

    style.text_styles = [
        (TextStyle::Small, FontId::proportional(typography::SMALL)),
        (TextStyle::Body, FontId::proportional(typography::BODY)),
        (TextStyle::Button, FontId::proportional(typography::BODY)),
        (
            TextStyle::Heading,
            FontId::new(typography::TITLE, fonts::family(fonts::UI_SEMIBOLD)),
        ),
        (TextStyle::Monospace, theme.editor_font()),
        (
            TextStyle::Name("label".into()),
            FontId::new(typography::LABEL, fonts::family(fonts::UI_MEDIUM)),
        ),
        (
            TextStyle::Name("display".into()),
            FontId::new(typography::DISPLAY, fonts::family(fonts::UI_SEMIBOLD)),
        ),
        (
            TextStyle::Name("shortcut".into()),
            FontId::new(typography::SHORTCUT, FontFamily::Proportional),
        ),
    ]
    .into();
    style.animation_time = theme.motion(tokens::motion::SELECTION);
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids_map_to_current_themes() {
        for (old, new) in [
            ("islands-dark", ThemeId::Dark),
            ("islands-darcula", ThemeId::Dark),
            ("darcula", ThemeId::Dark),
            ("dark-classic", ThemeId::Dark),
            ("islands-light", ThemeId::Light),
            ("light-classic", ThemeId::Light),
            ("light", ThemeId::Light),
            ("high-contrast", ThemeId::HighContrast),
            ("?", ThemeId::Dark),
        ] {
            assert_eq!(ThemeId::from_name(old), new, "{old}");
        }
        for id in ThemeId::ALL {
            assert_eq!(ThemeId::from_name(id.name()), id);
        }
    }

    #[test]
    fn resting_widgets_do_not_use_the_accent() {
        for id in ThemeId::ALL {
            let theme = Theme::new(id, Density::Comfortable);
            let style = style_for(&theme, &egui::Style::default());
            let w = &style.visuals.widgets;
            for stroke in [w.inactive.bg_stroke, w.noninteractive.bg_stroke] {
                assert_ne!(stroke.color, theme.palette.accent, "{id:?}");
                assert_ne!(stroke.color, theme.palette.focus_ring, "{id:?}");
            }
            assert_eq!(style.visuals.panel_fill, theme.palette.bg_panel);
        }
    }

    #[test]
    fn install_stores_the_theme_in_the_context() {
        let ctx = egui::Context::default();
        assert_eq!(current(&ctx).id, ThemeId::Dark);
        install(&ctx, Theme::new(ThemeId::Light, Density::Compact));
        let theme = current(&ctx);
        assert_eq!(
            (theme.id, theme.density),
            (ThemeId::Light, Density::Compact)
        );
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            tokens::LIGHT.bg_panel
        );
        assert!(!ctx.global_style().visuals.dark_mode);
    }

    #[test]
    fn reduced_motion_disables_animations() {
        let theme = Theme {
            reduced_motion: true,
            ..Theme::default()
        };
        assert_eq!(
            style_for(&theme, &egui::Style::default()).animation_time,
            0.0
        );
    }
}
