//! Monospaced fonts installed on the system, through fontconfig's
//! `fc-list` (RF-UI-05). The bundled JetBrains Mono needs none of this.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

/// A monospaced family with its regular and bold files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFont {
    pub family: String,
    pub regular: PathBuf,
    pub bold: Option<PathBuf>,
}

/// Formats egui can load (TrueType/OpenType outlines).
fn loadable(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf"))
}

/// Parses `fc-list :spacing=mono family style file` output.
pub fn parse_fc_list(output: &str) -> Vec<SystemFont> {
    let mut families: BTreeMap<String, SystemFont> = BTreeMap::new();
    for line in output.lines() {
        let Some((file, rest)) = line.split_once(": ") else {
            continue;
        };
        let path = PathBuf::from(file.trim());
        if !loadable(&path) {
            continue;
        }
        let (family, style) = match rest.split_once(":style=") {
            Some((family, style)) => (family, style),
            None => (rest, "Regular"),
        };
        // `family` may list localized names: keep the first.
        let family = family
            .split(',')
            .next()
            .unwrap_or(family)
            .trim()
            .to_string();
        let style = style
            .split(',')
            .next()
            .unwrap_or(style)
            .trim()
            .to_ascii_lowercase();
        let entry = families
            .entry(family.clone())
            .or_insert_with(|| SystemFont {
                family: family.clone(),
                regular: PathBuf::new(),
                bold: None,
            });
        match style.as_str() {
            "regular" | "book" | "normal" | "roman" => entry.regular = path,
            "bold" => entry.bold = Some(path),
            _ => {}
        }
    }
    families
        .into_values()
        .filter(|f| !f.regular.as_os_str().is_empty())
        .collect()
}

/// Installed monospaced families (empty without fontconfig).
pub fn monospace_families() -> Vec<SystemFont> {
    Command::new("fc-list")
        .args([":spacing=mono", "family", "style", "file"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_fc_list(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// The installed family named `family`.
pub fn find(family: &str) -> Option<SystemFont> {
    monospace_families()
        .into_iter()
        .find(|f| f.family == family)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fc_list_output_groups_regular_and_bold() {
        let out = "/f/DejaVuSansMono.ttf: DejaVu Sans Mono:style=Book\n\
/f/DejaVuSansMono-Bold.ttf: DejaVu Sans Mono:style=Bold\n\
/f/DejaVuSansMono-Oblique.ttf: DejaVu Sans Mono:style=Oblique\n\
/f/c0419bt_.pfb: Courier 10 Pitch:style=Regular\n\
/f/FiraCode-Regular.otf: Fira Code,Fira Code Light:style=Regular,Light\n";
        let fonts = parse_fc_list(out);
        assert_eq!(fonts.len(), 2, "Type 1 fonts are skipped: {fonts:?}");
        assert_eq!(fonts[0].family, "DejaVu Sans Mono");
        assert_eq!(
            fonts[0].bold,
            Some(PathBuf::from("/f/DejaVuSansMono-Bold.ttf"))
        );
        assert_eq!(fonts[1].family, "Fira Code");
        assert_eq!(fonts[1].bold, None);
    }
}
