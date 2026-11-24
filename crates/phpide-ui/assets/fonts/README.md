# Bundled fonts

| File | Font | Version | Source | License |
|---|---|---|---|---|
| `Inter-Regular.otf`, `Inter-Medium.otf`, `Inter-SemiBold.otf` | Inter | 4.0 | <https://github.com/rsms/inter> | SIL OFL 1.1 (`Inter-OFL.txt`) |
| `JetBrainsMono-Regular.ttf`, `JetBrainsMono-Bold.ttf` | JetBrains Mono | 2.211 | <https://github.com/JetBrains/JetBrainsMono> | SIL OFL 1.1 (`JetBrainsMono-OFL.txt`) |

The JetBrains Mono files have the `calt` feature removed, so code renders
without ligatures (`!==` stays three characters), like the upstream
"JetBrains Mono NL" build. egui has no switch to turn font features off.

Loaded by `crates/phpide-ui/src/design/fonts.rs`, with egui's default fonts
as fallback for glyphs these faces lack.
