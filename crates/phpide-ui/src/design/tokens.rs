//! Design tokens: the only place in `phpide-ui` with color literals
//! (enforced by `design::tests::no_color_literals_outside_tokens`).
//!
//! Values follow docs/DESIGN_TOKENS.md. Each theme has its own palette —
//! Light is not an inversion of Dark, and High Contrast favors legibility
//! and visible focus over decoration.

use eframe::egui::Color32;
use phpide_php::{SyntaxRole, SyntaxStyle};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `#RRGGBB` → color (user colors in Settings › Color Scheme).
pub fn parse_hex_color(text: &str) -> Option<Color32> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(hex)
}

/// Color → `#RRGGBB`.
pub fn hex_color_string(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

const fn hexa(rgb: u32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
}

/// Semantic shell colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg_canvas: Color32,
    pub bg_panel: Color32,
    pub bg_editor: Color32,
    pub bg_elevated: Color32,
    pub bg_inset: Color32,
    pub bg_hover: Color32,
    pub bg_pressed: Color32,
    pub bg_selected: Color32,
    pub bg_selected_focused: Color32,
    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_disabled: Color32,
    pub text_on_accent: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Fill of primary buttons and badges (text uses `text_on_accent`).
    pub accent_fill: Color32,
    pub accent_fill_hover: Color32,
    pub focus_ring: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub info: Color32,
    pub git_added: Color32,
    pub git_modified: Color32,
    pub git_deleted: Color32,
    pub git_conflict: Color32,
    pub git_untracked: Color32,
    pub git_ignored: Color32,
    pub debug_breakpoint: Color32,
    pub debug_current_line: Color32,
    pub coverage_full: Color32,
    pub coverage_partial: Color32,
    pub coverage_none: Color32,
    pub diff_added_bg: Color32,
    pub diff_removed_bg: Color32,
    pub search_match_bg: Color32,
    pub search_current_match_bg: Color32,
    pub editor_current_line_bg: Color32,
    pub editor_caret: Color32,
    pub editor_selection_bg: Color32,
    pub editor_indent_guide: Color32,
    pub editor_sticky_bg: Color32,
    pub editor_sticky_shadow: Color32,
    pub editor_gutter_text: Color32,
    pub editor_gutter_text_current: Color32,
    pub editor_whitespace: Color32,
    pub editor_fold_bg: Color32,
    pub shadow: Color32,
    pub file_php: Color32,
    pub file_script: Color32,
    pub file_style: Color32,
    pub file_markup: Color32,
    pub file_data: Color32,
    pub file_config: Color32,
    pub file_vcs: Color32,
    pub file_image: Color32,
    pub file_archive: Color32,
    pub file_text: Color32,
    pub folder: Color32,
}

impl Palette {
    /// Icon tint of a file category.
    pub fn file_color(&self, kind: phpide_icons::FileKind) -> Color32 {
        use phpide_icons::FileKind;
        match kind {
            FileKind::Php => self.file_php,
            FileKind::Script => self.file_script,
            FileKind::Style => self.file_style,
            FileKind::Markup => self.file_markup,
            FileKind::Data => self.file_data,
            FileKind::Text => self.file_text,
            FileKind::Config => self.file_config,
            FileKind::Vcs => self.file_vcs,
            FileKind::Image => self.file_image,
            FileKind::Archive => self.file_archive,
            FileKind::Other => self.text_secondary,
        }
    }
}

/// Syntax colors by semantic role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxPalette {
    pub plain: Color32,
    pub keyword: Color32,
    pub type_: Color32,
    pub function: Color32,
    pub method: Color32,
    pub variable: Color32,
    pub parameter: Color32,
    pub property: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub documentation: Color32,
    pub operator: Color32,
    pub punctuation: Color32,
    pub attribute: Color32,
    pub constant: Color32,
    pub namespace: Color32,
    pub tag: Color32,
    pub invalid: Color32,
    pub deprecated: Color32,
}

impl SyntaxPalette {
    pub fn color(&self, role: SyntaxRole) -> Color32 {
        match role {
            SyntaxRole::Plain => self.plain,
            SyntaxRole::Keyword => self.keyword,
            SyntaxRole::Type => self.type_,
            SyntaxRole::Function => self.function,
            SyntaxRole::Method => self.method,
            SyntaxRole::Variable => self.variable,
            SyntaxRole::Parameter => self.parameter,
            SyntaxRole::Property => self.property,
            SyntaxRole::String => self.string,
            SyntaxRole::Number => self.number,
            SyntaxRole::Comment => self.comment,
            SyntaxRole::Documentation => self.documentation,
            SyntaxRole::Operator => self.operator,
            SyntaxRole::Punctuation => self.punctuation,
            SyntaxRole::Attribute => self.attribute,
            SyntaxRole::Constant => self.constant,
            SyntaxRole::Namespace => self.namespace,
            SyntaxRole::Tag => self.tag,
            SyntaxRole::Invalid => self.invalid,
            SyntaxRole::Deprecated => self.deprecated,
        }
    }

    pub fn style(&self, font: eframe::egui::FontId) -> SyntaxStyle {
        let mut style = SyntaxStyle::uniform(font, self.plain);
        for role in SyntaxRole::ALL {
            style.colors[role as usize] = self.color(role);
        }
        style
    }
}

pub const DARK: Palette = Palette {
    bg_canvas: hex(0x101216),
    bg_panel: hex(0x16191F),
    bg_editor: hex(0x111318),
    bg_elevated: hex(0x1D222B),
    bg_inset: hex(0x0D0F13),
    bg_hover: hex(0x222832),
    bg_pressed: hex(0x29313D),
    bg_selected: hex(0x26364D),
    bg_selected_focused: hex(0x2E4668),
    border_subtle: hex(0x282E38),
    border_strong: hex(0x3A4350),
    text_primary: hex(0xDCE2EA),
    text_secondary: hex(0xA5AFBC),
    text_muted: hex(0x808A97),
    text_disabled: hex(0x59616C),
    text_on_accent: hex(0xFFFFFF),
    accent: hex(0x6C9CFF),
    accent_hover: hex(0x82ACFF),
    accent_fill: hex(0x3B6FD8),
    accent_fill_hover: hex(0x4A7DE6),
    focus_ring: hex(0x8CB4FF),
    success: hex(0x63B995),
    warning: hex(0xE2B93D),
    error: hex(0xEF6A73),
    info: hex(0x65B5E8),
    git_added: hex(0x6DBE8A),
    git_modified: hex(0x79A8F5),
    git_deleted: hex(0x8B939E),
    git_conflict: hex(0xEF6A73),
    git_untracked: hex(0xD9826A),
    git_ignored: hex(0x6E7783),
    debug_breakpoint: hex(0xE5484D),
    debug_current_line: hex(0x24364A),
    coverage_full: hex(0x3E8A62),
    coverage_partial: hex(0x9A8433),
    coverage_none: hex(0x9A4249),
    diff_added_bg: hex(0x1C3326),
    diff_removed_bg: hex(0x3A1F25),
    search_match_bg: hex(0x2C4835),
    search_current_match_bg: hex(0x3E6B4A),
    editor_current_line_bg: hex(0x171A21),
    editor_caret: hex(0xDCE2EA),
    editor_selection_bg: hex(0x264065),
    editor_indent_guide: hex(0x242932),
    editor_sticky_bg: hex(0x161920),
    editor_sticky_shadow: hexa(0x000000, 110),
    editor_gutter_text: hex(0x5F6773),
    editor_gutter_text_current: hex(0xA5AFBC),
    editor_whitespace: hex(0x3A4350),
    editor_fold_bg: hex(0x222832),
    shadow: hexa(0x000000, 115),
    file_php: hex(0xA58BFF),
    file_script: hex(0xE2C15A),
    file_style: hex(0x5FA8E8),
    file_markup: hex(0xE3825A),
    file_data: hex(0x7FC8A9),
    file_config: hex(0x8E99A8),
    file_vcs: hex(0xE0704F),
    file_image: hex(0xC38FD9),
    file_archive: hex(0xB5A06A),
    file_text: hex(0xA5AFBC),
    folder: hex(0x7E9BC4),
};

pub const DARK_SYNTAX: SyntaxPalette = SyntaxPalette {
    plain: hex(0xD5DCE6),
    keyword: hex(0xC792EA),
    type_: hex(0xE8C07D),
    function: hex(0x6CB4F5),
    method: hex(0x6CB4F5),
    variable: hex(0xE88F98),
    parameter: hex(0xF0AAB1),
    property: hex(0x7DCFD8),
    string: hex(0x9CCB82),
    number: hex(0xE4A672),
    comment: hex(0x808B9A),
    documentation: hex(0x8FA08B),
    operator: hex(0x9AB3CC),
    punctuation: hex(0xA5AFBC),
    attribute: hex(0xD7B76A),
    constant: hex(0xE4A672),
    namespace: hex(0xB9C4D2),
    tag: hex(0xE88F98),
    invalid: hex(0xEF6A73),
    deprecated: hex(0x8B939E),
};

pub const LIGHT: Palette = Palette {
    bg_canvas: hex(0xEEF0F3),
    bg_panel: hex(0xF7F8FA),
    bg_editor: hex(0xFFFFFF),
    bg_elevated: hex(0xFFFFFF),
    bg_inset: hex(0xF1F2F5),
    bg_hover: hex(0xE7EAEE),
    bg_pressed: hex(0xDCE0E6),
    bg_selected: hex(0xDDE6F5),
    bg_selected_focused: hex(0xC9DAF8),
    border_subtle: hex(0xDFE2E7),
    border_strong: hex(0xB9C0CA),
    text_primary: hex(0x1E2329),
    text_secondary: hex(0x4C5561),
    text_muted: hex(0x646D79),
    text_disabled: hex(0xA2A9B2),
    text_on_accent: hex(0xFFFFFF),
    accent: hex(0x2763D6),
    accent_hover: hex(0x1F54B8),
    accent_fill: hex(0x2F6BDE),
    accent_fill_hover: hex(0x255BC2),
    focus_ring: hex(0x4C82EE),
    success: hex(0x23794A),
    warning: hex(0x95680A),
    error: hex(0xC9303C),
    info: hex(0x1F6FA8),
    git_added: hex(0x23794A),
    git_modified: hex(0x2763D6),
    git_deleted: hex(0x67707C),
    git_conflict: hex(0xC9303C),
    git_untracked: hex(0xA9502C),
    git_ignored: hex(0x8A929C),
    debug_breakpoint: hex(0xDD3F47),
    debug_current_line: hex(0xDCE9FB),
    coverage_full: hex(0x8FCCA3),
    coverage_partial: hex(0xE6CF7E),
    coverage_none: hex(0xEAA3A9),
    diff_added_bg: hex(0xE2F3E6),
    diff_removed_bg: hex(0xFBE4E6),
    search_match_bg: hex(0xDCEFC9),
    search_current_match_bg: hex(0xB4DC97),
    editor_current_line_bg: hex(0xF5F7FA),
    editor_caret: hex(0x1E2329),
    editor_selection_bg: hex(0xCCDDF9),
    editor_indent_guide: hex(0xE6E8EC),
    editor_sticky_bg: hex(0xF8F9FB),
    editor_sticky_shadow: hexa(0x000000, 30),
    editor_gutter_text: hex(0x9AA1AB),
    editor_gutter_text_current: hex(0x4C5561),
    editor_whitespace: hex(0xC3C8D0),
    editor_fold_bg: hex(0xE7EAEE),
    shadow: hexa(0x000000, 45),
    file_php: hex(0x6F4FD0),
    file_script: hex(0x9A7600),
    file_style: hex(0x2370B8),
    file_markup: hex(0xBC5226),
    file_data: hex(0x23795F),
    file_config: hex(0x67707C),
    file_vcs: hex(0xC4482A),
    file_image: hex(0x8E4FB3),
    file_archive: hex(0x806B2E),
    file_text: hex(0x4C5561),
    folder: hex(0x52739F),
};

pub const LIGHT_SYNTAX: SyntaxPalette = SyntaxPalette {
    plain: hex(0x1F2328),
    keyword: hex(0xB4233A),
    type_: hex(0x8A4500),
    function: hex(0x6A3FBF),
    method: hex(0x6A3FBF),
    variable: hex(0x1D5794),
    parameter: hex(0x2A64A6),
    property: hex(0x0A6A6A),
    string: hex(0x0E6A2D),
    number: hex(0x0550AE),
    comment: hex(0x67707C),
    documentation: hex(0x4F6B48),
    operator: hex(0x3D4650),
    punctuation: hex(0x4C5561),
    attribute: hex(0x8A4500),
    constant: hex(0x0550AE),
    namespace: hex(0x4C5561),
    tag: hex(0x116329),
    invalid: hex(0xC9303C),
    deprecated: hex(0x6A727C),
};

pub const HIGH_CONTRAST: Palette = Palette {
    bg_canvas: hex(0x000000),
    bg_panel: hex(0x000000),
    bg_editor: hex(0x000000),
    bg_elevated: hex(0x0C0C0C),
    bg_inset: hex(0x000000),
    bg_hover: hex(0x1F1F1F),
    bg_pressed: hex(0x303030),
    bg_selected: hex(0x00396E),
    bg_selected_focused: hex(0x004E99),
    border_subtle: hex(0x7A7A7A),
    border_strong: hex(0xFFFFFF),
    text_primary: hex(0xFFFFFF),
    text_secondary: hex(0xE6E6E6),
    text_muted: hex(0xC8C8C8),
    text_disabled: hex(0x8C8C8C),
    text_on_accent: hex(0xFFFFFF),
    accent: hex(0x5CBBFF),
    accent_hover: hex(0x8ACEFF),
    accent_fill: hex(0x0058B0),
    accent_fill_hover: hex(0x006CD6),
    focus_ring: hex(0xFFD500),
    success: hex(0x4CE08A),
    warning: hex(0xFFD500),
    error: hex(0xFF7070),
    info: hex(0x5CBBFF),
    git_added: hex(0x4CE08A),
    git_modified: hex(0x5CBBFF),
    git_deleted: hex(0xC8C8C8),
    git_conflict: hex(0xFF7070),
    git_untracked: hex(0xFFA066),
    git_ignored: hex(0xA6A6A6),
    debug_breakpoint: hex(0xFF5252),
    debug_current_line: hex(0x00396E),
    coverage_full: hex(0x1F8A47),
    coverage_partial: hex(0x9A8200),
    coverage_none: hex(0xA02828),
    diff_added_bg: hex(0x003D1A),
    diff_removed_bg: hex(0x4D0012),
    search_match_bg: hex(0x5C4D00),
    search_current_match_bg: hex(0x8A7300),
    editor_current_line_bg: hex(0x141414),
    editor_caret: hex(0xFFFFFF),
    editor_selection_bg: hex(0x004E99),
    editor_indent_guide: hex(0x4D4D4D),
    editor_sticky_bg: hex(0x0C0C0C),
    editor_sticky_shadow: hexa(0xFFFFFF, 60),
    editor_gutter_text: hex(0xC8C8C8),
    editor_gutter_text_current: hex(0xFFFFFF),
    editor_whitespace: hex(0x7A7A7A),
    editor_fold_bg: hex(0x303030),
    shadow: hexa(0x000000, 0),
    file_php: hex(0xC9ADFF),
    file_script: hex(0xFFE066),
    file_style: hex(0x80C8FF),
    file_markup: hex(0xFFA27A),
    file_data: hex(0x8CF0C4),
    file_config: hex(0xD6DCE4),
    file_vcs: hex(0xFF9070),
    file_image: hex(0xE6B0FF),
    file_archive: hex(0xE6D095),
    file_text: hex(0xFFFFFF),
    folder: hex(0xA3C6FF),
};

pub const HIGH_CONTRAST_SYNTAX: SyntaxPalette = SyntaxPalette {
    plain: hex(0xFFFFFF),
    keyword: hex(0xEAB0FF),
    type_: hex(0xFFE27A),
    function: hex(0x80D2FF),
    method: hex(0x80D2FF),
    variable: hex(0xFFB8B8),
    parameter: hex(0xFFCECE),
    property: hex(0x8CF0E8),
    string: hex(0xAAF28E),
    number: hex(0xFFC285),
    comment: hex(0xCFCFCF),
    documentation: hex(0xBDE6B5),
    operator: hex(0xFFFFFF),
    punctuation: hex(0xE6E6E6),
    attribute: hex(0xFFE27A),
    constant: hex(0xFFC285),
    namespace: hex(0xE6E6E6),
    tag: hex(0xFFB8B8),
    invalid: hex(0xFF7A7A),
    deprecated: hex(0xB0B0B0),
};

/// Spacing scale in points (`space.0` … `space.9`).
pub mod space {
    pub const S0: f32 = 0.0;
    pub const S1: f32 = 2.0;
    pub const S2: f32 = 4.0;
    pub const S3: f32 = 6.0;
    pub const S4: f32 = 8.0;
    pub const S5: f32 = 12.0;
    pub const S6: f32 = 16.0;
    pub const S7: f32 = 20.0;
    pub const S8: f32 = 24.0;
    pub const S9: f32 = 32.0;
}

/// Corner radii in points.
pub mod radius {
    pub const CONTROL: u8 = 4;
    pub const CARD: u8 = 6;
    pub const DIALOG: u8 = 8;
}

/// Type scale in points. Families are registered by `design::fonts`.
pub mod typography {
    pub const BODY: f32 = 13.0;
    pub const LABEL: f32 = 12.0;
    pub const SMALL: f32 = 11.0;
    pub const TITLE: f32 = 16.0;
    pub const DISPLAY: f32 = 22.0;
    pub const EDITOR: f32 = 14.0;
    pub const GUTTER: f32 = 12.0;
    pub const SHORTCUT: f32 = 11.0;
}

/// Durations in seconds; `0` when Reduced Motion is on.
pub mod motion {
    pub const HOVER: f32 = 0.08;
    pub const SELECTION: f32 = 0.12;
    pub const POPOVER: f32 = 0.14;
    pub const TOOL_WINDOW: f32 = 0.18;
}

/// Comfortable or Compact sizes for structural elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Density {
    Comfortable,
    Compact,
}

impl Density {
    pub const ALL: [Density; 2] = [Density::Comfortable, Density::Compact];

    pub fn label(self) -> &'static str {
        match self {
            Density::Comfortable => "Comfortable",
            Density::Compact => "Compact",
        }
    }

    pub fn metrics(self) -> Metrics {
        match self {
            Density::Comfortable => Metrics {
                window_header: 40.0,
                tab_strip: 34.0,
                tool_window_header: 32.0,
                tool_window_rail: 40.0,
                row: 28.0,
                control: 30.0,
                status_bar: 26.0,
                icon_button: 28.0,
                icon: 16.0,
                header_icon: 20.0,
                min_target: 28.0,
                panel_padding: space::S4,
                dialog_padding: space::S8,
                popover_padding: space::S4,
                gap: space::S4,
                group_gap: space::S6,
            },
            Density::Compact => Metrics {
                window_header: 36.0,
                tab_strip: 30.0,
                tool_window_header: 28.0,
                tool_window_rail: 36.0,
                row: 24.0,
                control: 28.0,
                status_bar: 24.0,
                icon_button: 24.0,
                icon: 16.0,
                header_icon: 18.0,
                min_target: 24.0,
                panel_padding: space::S4,
                dialog_padding: space::S7,
                popover_padding: space::S3,
                gap: space::S3,
                group_gap: space::S5,
            },
        }
    }
}

/// Dimensions of the density table (points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub window_header: f32,
    pub tab_strip: f32,
    pub tool_window_header: f32,
    pub tool_window_rail: f32,
    pub row: f32,
    pub control: f32,
    pub status_bar: f32,
    pub icon_button: f32,
    pub icon: f32,
    pub header_icon: f32,
    pub min_target: f32,
    pub panel_padding: f32,
    pub dialog_padding: f32,
    pub popover_padding: f32,
    pub gap: f32,
    pub group_gap: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f32 {
        let channel = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    /// WCAG 2.x contrast ratio.
    pub(crate) fn contrast(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    const THEMES: [(&str, Palette, SyntaxPalette, f32); 3] = [
        ("dark", DARK, DARK_SYNTAX, 4.5),
        ("light", LIGHT, LIGHT_SYNTAX, 4.5),
        ("high contrast", HIGH_CONTRAST, HIGH_CONTRAST_SYNTAX, 7.0),
    ];

    #[test]
    fn text_contrast_meets_wcag() {
        for (name, p, _, body) in THEMES {
            for bg in [p.bg_panel, p.bg_editor, p.bg_elevated, p.bg_canvas] {
                assert!(contrast(p.text_primary, bg) >= 7.0, "{name} primary");
                assert!(contrast(p.text_secondary, bg) >= body, "{name} secondary");
                assert!(contrast(p.text_muted, bg) >= 4.5, "{name} muted");
            }
            for bg in [p.bg_selected, p.bg_selected_focused, p.bg_hover] {
                assert!(
                    contrast(p.text_primary, bg) >= body,
                    "{name} text on selection"
                );
            }
            assert!(
                contrast(p.text_on_accent, p.accent_fill) >= 4.5,
                "{name} on accent"
            );
            assert!(
                contrast(p.text_disabled, p.bg_panel) >= 2.0,
                "{name} disabled"
            );
            for (label, color) in [
                ("error", p.error),
                ("warning", p.warning),
                ("success", p.success),
                ("info", p.info),
                ("accent", p.accent),
            ] {
                assert!(contrast(color, p.bg_panel) >= 4.5, "{name} {label}");
            }
        }
    }

    #[test]
    fn syntax_is_legible_on_editor_and_selection() {
        for (name, p, syntax, body) in THEMES {
            for role in SyntaxRole::ALL {
                let color = syntax.color(role);
                let ratio = contrast(color, p.bg_editor);
                assert!(ratio >= body, "{name} {role:?}: {ratio:.2}");
                let on_current_line = contrast(color, p.editor_current_line_bg);
                assert!(on_current_line >= body, "{name} {role:?} on current line");
                assert!(
                    contrast(color, p.editor_selection_bg) >= 3.0,
                    "{name} {role:?} selected"
                );
            }
            // Strings, keywords and types stay distinguishable.
            assert_ne!(syntax.string, syntax.keyword);
            assert_ne!(syntax.keyword, syntax.type_);
            assert_ne!(syntax.string, syntax.type_);
        }
    }

    #[test]
    fn light_is_not_an_inversion_of_dark() {
        assert_ne!(LIGHT.bg_editor, DARK.text_primary);
        assert!(luminance(LIGHT.bg_panel) > 0.8 && luminance(DARK.bg_panel) < 0.02);
        assert_ne!(LIGHT_SYNTAX, DARK_SYNTAX);
    }

    #[test]
    fn density_table_matches_the_spec() {
        let c = Density::Comfortable.metrics();
        let k = Density::Compact.metrics();
        assert_eq!((c.row, k.row), (28.0, 24.0));
        assert_eq!((c.control, k.control), (30.0, 28.0));
        assert_eq!((c.status_bar, k.status_bar), (26.0, 24.0));
        assert_eq!((c.min_target, k.min_target), (28.0, 24.0));
        assert!(k.icon_button >= k.min_target && c.icon_button >= c.min_target);
    }
}
