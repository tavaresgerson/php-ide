//! Colors and fonts of the editor, filled by the application from its theme.

use eframe::egui::{Color32, FontId};
use phpide_php::SyntaxStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct EditorColors {
    pub background: Color32,
    pub gutter_background: Color32,
    pub gutter_text: Color32,
    pub gutter_text_current: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub caret: Color32,
    pub indent_guide: Color32,
    pub fold_placeholder: Color32,
    pub fold_placeholder_text: Color32,
    pub fold_marker: Color32,
    pub search_match: Color32,
    pub search_current: Color32,
    pub occurrence: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub hint: Color32,
    pub git_added: Color32,
    pub git_modified: Color32,
    pub git_deleted: Color32,
    pub breakpoint: Color32,
    pub breakpoint_check: Color32,
    pub bookmark: Color32,
    pub execution_line: Color32,
    pub coverage_full: Color32,
    pub coverage_partial: Color32,
    pub coverage_none: Color32,
    pub sticky_background: Color32,
    pub sticky_border: Color32,
    pub scrollbar_thumb: Color32,
    pub scrollbar_thumb_hover: Color32,
    pub inlay_background: Color32,
    pub inlay_text: Color32,
    pub blame_text: Color32,
    pub panel_background: Color32,
    pub panel_border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditorStyle {
    /// Font and syntax colors of the code.
    pub syntax: SyntaxStyle,
    pub gutter_font: FontId,
    /// UI font for the find bar and tooltips.
    pub ui_font: FontId,
    /// Row height as a multiple of the font height.
    pub line_spacing: f32,
    pub colors: EditorColors,
}

impl EditorStyle {
    pub fn font(&self) -> &FontId {
        &self.syntax.font
    }

    /// Hash of everything that changes how a line is laid out.
    pub(crate) fn layout_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.syntax.font.size.to_bits().hash(&mut hasher);
        format!("{:?}", self.syntax.font.family).hash(&mut hasher);
        for color in &self.syntax.colors {
            color.to_array().hash(&mut hasher);
        }
        format!("{:?}{:?}", self.syntax.attributes, self.syntax.bold_font).hash(&mut hasher);
        self.colors.inlay_background.to_array().hash(&mut hasher);
        self.colors.inlay_text.to_array().hash(&mut hasher);
        hasher.finish()
    }
}

impl EditorStyle {
    /// A style derived from egui's own visuals, for tests, benchmarks and
    /// embedders without a theme. The application builds its style from the
    /// design tokens instead.
    pub fn from_visuals(visuals: &eframe::egui::Visuals, font_size: f32) -> Self {
        let font = FontId::monospace(font_size);
        let text = visuals.text_color();
        let muted = visuals.weak_text_color();
        let accent = visuals.selection.stroke.color;
        let error = visuals.error_fg_color;
        let warning = visuals.warn_fg_color;
        let panel = visuals.panel_fill;
        let faint = visuals.faint_bg_color;
        let border = visuals.widgets.noninteractive.bg_stroke.color;
        let selection = visuals.selection.bg_fill;
        EditorStyle {
            syntax: SyntaxStyle::uniform(font.clone(), text),
            gutter_font: FontId::monospace(font_size * 0.9),
            ui_font: FontId::proportional(font_size),
            line_spacing: 1.4,
            colors: EditorColors {
                background: visuals.extreme_bg_color,
                gutter_background: visuals.extreme_bg_color,
                gutter_text: muted,
                gutter_text_current: text,
                current_line: faint,
                selection,
                caret: text,
                indent_guide: border,
                fold_placeholder: faint,
                fold_placeholder_text: muted,
                fold_marker: muted,
                search_match: selection.gamma_multiply(0.6),
                search_current: selection,
                occurrence: faint,
                error,
                warning,
                info: accent,
                hint: muted,
                git_added: accent,
                git_modified: warning,
                git_deleted: error,
                breakpoint: error,
                breakpoint_check: text,
                bookmark: accent,
                execution_line: selection.gamma_multiply(0.5),
                coverage_full: accent,
                coverage_partial: warning,
                coverage_none: error,
                sticky_background: panel,
                sticky_border: border,
                scrollbar_thumb: visuals.widgets.inactive.bg_fill,
                scrollbar_thumb_hover: visuals.widgets.hovered.bg_fill,
                inlay_background: faint,
                inlay_text: muted,
                blame_text: muted,
                panel_background: panel,
                panel_border: border,
                text,
                text_muted: muted,
                accent,
            },
        }
    }
}
