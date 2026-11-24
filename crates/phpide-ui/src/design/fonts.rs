//! Bundled fonts: Inter for the shell and JetBrains Mono for code (both SIL
//! OFL 1.1, see `assets/fonts/*-OFL.txt`). egui's default fonts stay in
//! every family as fallback, so box drawing, symbols and emoji never render
//! as empty boxes.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Inter Medium: labels and interactive metadata.
pub const UI_MEDIUM: &str = "ui-medium";
/// Inter SemiBold: titles and the display size.
pub const UI_SEMIBOLD: &str = "ui-semibold";
/// JetBrains Mono Bold.
pub const MONO_BOLD: &str = "mono-bold";

const INTER: &str = "Inter-Regular";
const INTER_MEDIUM: &str = "Inter-Medium";
const INTER_SEMIBOLD: &str = "Inter-SemiBold";
const MONO: &str = "JetBrainsMono-Regular";
const MONO_BOLD_DATA: &str = "JetBrainsMono-Bold";

pub fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// A font chosen for the editor instead of JetBrains Mono.
pub struct EditorFont {
    pub regular: Vec<u8>,
    pub bold: Option<Vec<u8>>,
}

const CUSTOM: &str = "editor-custom";
const CUSTOM_BOLD: &str = "editor-custom-bold";

/// Font definitions with the bundled faces first and egui's defaults after.
#[cfg(test)]
pub fn definitions() -> FontDefinitions {
    definitions_with(None)
}

/// [`definitions`] with another face first in the code families. Without a
/// bold file, bold code uses the regular face so columns stay aligned.
pub fn definitions_with(editor: Option<EditorFont>) -> FontDefinitions {
    let mut fonts = base_definitions();
    if let Some(editor) = editor {
        fonts.font_data.insert(
            CUSTOM.to_owned(),
            Arc::new(FontData::from_owned(editor.regular)),
        );
        let bold = match editor.bold {
            Some(bytes) => {
                fonts.font_data.insert(
                    CUSTOM_BOLD.to_owned(),
                    Arc::new(FontData::from_owned(bytes)),
                );
                CUSTOM_BOLD
            }
            None => CUSTOM,
        };
        if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
            list.insert(0, CUSTOM.to_owned());
        }
        if let Some(list) = fonts.families.get_mut(&family(MONO_BOLD)) {
            list.insert(0, bold.to_owned());
        }
    }
    fonts
}

fn base_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let bundled: [(&str, &'static [u8]); 5] = [
        (
            INTER,
            include_bytes!("../../assets/fonts/Inter-Regular.otf"),
        ),
        (
            INTER_MEDIUM,
            include_bytes!("../../assets/fonts/Inter-Medium.otf"),
        ),
        (
            INTER_SEMIBOLD,
            include_bytes!("../../assets/fonts/Inter-SemiBold.otf"),
        ),
        (
            MONO,
            include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
        ),
        (
            MONO_BOLD_DATA,
            include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
        ),
    ];
    for (name, bytes) in bundled {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let defaults_proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let defaults_monospace = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    // Inter has no box-drawing glyphs, so the UI families end with Hack.
    let chain = |first: &str, extra: &[&str], rest: &[String], last: &[&str]| -> Vec<String> {
        let mut out = vec![first.to_owned()];
        out.extend(extra.iter().map(|name| (*name).to_owned()));
        out.extend(rest.iter().cloned());
        out.extend(last.iter().map(|name| (*name).to_owned()));
        out
    };
    let p = &defaults_proportional;
    let m = &defaults_monospace;
    let families = [
        (FontFamily::Proportional, chain(INTER, &[], p, &["Hack"])),
        (
            family(UI_MEDIUM),
            chain(INTER_MEDIUM, &[INTER], p, &["Hack"]),
        ),
        (
            family(UI_SEMIBOLD),
            chain(INTER_SEMIBOLD, &[INTER], p, &["Hack"]),
        ),
        (FontFamily::Monospace, chain(MONO, &[], m, &[INTER])),
        (
            family(MONO_BOLD),
            chain(MONO_BOLD_DATA, &[MONO], m, &[INTER]),
        ),
    ];
    for (name, list) in families {
        fonts.families.insert(name, list);
    }
    fonts
}

#[cfg(test)]
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

/// Name of the bundled code font.
pub const BUNDLED_EDITOR_FAMILY: &str = "JetBrains Mono";

/// Installs the fonts with `family` as the code font (an installed
/// monospaced family; the bundled one when empty, bundled or missing).
pub fn install_with_editor_family(ctx: &egui::Context, family: &str) {
    let editor = if family.is_empty() || family == BUNDLED_EDITOR_FAMILY {
        None
    } else {
        crate::services::system_fonts::find(family).and_then(|font| {
            Some(EditorFont {
                regular: std::fs::read(&font.regular).ok()?,
                bold: font.bold.and_then(|p| std::fs::read(p).ok()),
            })
        })
    };
    ctx.set_fonts(definitions_with(editor));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text the IDE must render without missing glyphs: accents in paths,
    /// Greek/Cyrillic identifiers, menu symbols and tree/box drawing.
    const SAMPLE: &str = "Ação São Paulo naïve façade Ærø ß €£¥ — … “” ‘’ • → ← ↑ ↓ \
        αβγ Привет ĄŁŞİŻ ─│┌┐└┘├┤ ✓ ✗ ⚠ ⏎";

    #[test]
    fn every_family_covers_the_sample_text() {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx.run_ui(Default::default(), |_| {})
            .drop_without_applying_deltas();
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            family(UI_MEDIUM),
            family(UI_SEMIBOLD),
            family(MONO_BOLD),
        ] {
            let font = egui::FontId::new(13.0, family.clone());
            let missing: String = ctx.fonts_mut(|fonts| {
                SAMPLE
                    .chars()
                    .filter(|c| !c.is_whitespace() && !fonts.has_glyph(&font, *c))
                    .collect()
            });
            assert!(missing.is_empty(), "{family:?} lacks {missing:?}");
        }
    }

    #[test]
    fn bundled_faces_come_first() {
        let fonts = definitions();
        assert_eq!(fonts.families[&FontFamily::Proportional][0], INTER);
        assert_eq!(fonts.families[&FontFamily::Monospace][0], MONO);
        assert!(fonts.families[&FontFamily::Monospace].contains(&"Hack".to_owned()));
    }
}
