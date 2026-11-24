//! Design system of PHP IDE: tokens, themes, bundled fonts, components and
//! the component gallery (`phpide --gallery`).
//!
//! UI code takes colors and sizes from here: `design::current(ctx)` returns
//! the active [`Theme`] with its palette, syntax colors and density metrics.

// Part of the API is only used by the gallery until the shell is rebuilt
// with the components (Phase 4).
#[allow(dead_code, unused_imports)]
pub mod components;
pub mod fonts;
pub mod gallery;
pub mod theme;
// The complete token set of docs/DESIGN_TOKENS.md, used or not yet.
#[allow(dead_code)]
pub mod tokens;

#[cfg(test)]
pub(crate) mod snapshot;

pub use theme::{current, install, Theme, ThemeId};
pub use tokens::Density;

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Color literals are only allowed in `design/tokens.rs`, so every
    /// color comes from a theme (CI runs this test).
    #[test]
    fn no_color_literals_outside_tokens() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let forbidden = [
            "Color32::from_rgb",
            "Color32::from_rgba",
            "Color32::from_gray",
            "Color32::from_black_alpha",
            "Color32::from_white_alpha",
            "Color32::WHITE",
            "Color32::BLACK",
            "Color32::RED",
            "Color32::GREEN",
            "Color32::BLUE",
            "Color32::YELLOW",
            "Color32::GRAY",
            "Color32::DARK_GRAY",
            "Color32::LIGHT_GRAY",
            "Color32::LIGHT_GREEN",
            "Color32::LIGHT_RED",
            "Color32::LIGHT_BLUE",
            "Color32::LIGHT_YELLOW",
            "Color32::DARK_RED",
            "Color32::DARK_GREEN",
            "Color32::DARK_BLUE",
            "Color32::KHAKI",
            "Color32::GOLD",
            "Color32::BROWN",
            "Color32::ORANGE",
            "Rgba::from_rgb",
        ];
        let mut offenders = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(&src)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.extension().is_none_or(|e| e != "rs") || relative == "design/tokens.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Test modules may assert on literal colors.
                let code = text.split("#[cfg(test)]").next().unwrap_or("");
                for (number, line) in code.lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    if forbidden.iter().any(|f| line.contains(f)) {
                        offenders.push(format!("{relative}:{}: {}", number + 1, line.trim()));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "color literals outside design/tokens.rs:\n{}",
            offenders.join("\n")
        );
    }
}
