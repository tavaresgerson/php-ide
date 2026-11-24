//! A small Markdown renderer for documentation popups (hover, completion,
//! parameter info): fenced code blocks, headings, bullet lists, paragraphs
//! with `code`, **bold**, *italic* and links (shown as their text).

use eframe::egui::{self, text::LayoutJob, FontId, TextFormat};

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Code(String),
    Heading(String),
    Bullet(String),
    Paragraph(String),
    Rule,
}

/// Splits Markdown into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut code: Option<Vec<String>> = None;
    let flush = |paragraph: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(paragraph.join(" ")));
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(lines) = code.as_mut() {
            if trimmed.starts_with("```") {
                // `<?php` openers only make PHP blocks parse; hide them.
                let body: Vec<String> = lines
                    .iter()
                    .filter(|l| l.trim() != "<?php")
                    .cloned()
                    .collect();
                blocks.push(Block::Code(body.join("\n")));
                code = None;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            code = Some(Vec::new());
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else if trimmed == "---" || trimmed == "___" || trimmed == "***" {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
        } else if let Some(heading) = trimmed.strip_prefix('#') {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(
                heading.trim_start_matches('#').trim().to_string(),
            ));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Bullet(item.to_string()));
        } else if trimmed.starts_with('@') {
            // PHPDoc tags read better one per line.
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Paragraph(trimmed.to_string()));
        } else {
            paragraph.push(trimmed.to_string());
        }
    }
    if let Some(lines) = code {
        blocks.push(Block::Code(lines.join("\n")));
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// Inline spans of a paragraph: (text, code, bold, italic).
pub fn inline(text: &str) -> Vec<(String, bool, bool, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, bool, bool, bool)> = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let mut current = String::new();
    let mut i = 0;
    let push =
        |out: &mut Vec<(String, bool, bool, bool)>, text: &mut String, code, bold, italic| {
            if !text.is_empty() {
                out.push((std::mem::take(text), code, bold, italic));
            }
        };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            current.push(chars[i + 1]);
            i += 2;
        } else if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '`') {
                push(&mut out, &mut current, false, bold, italic);
                let code: String = chars[i + 1..i + 1 + end].iter().collect();
                out.push((code, true, bold, italic));
                i += end + 2;
            } else {
                current.push(c);
                i += 1;
            }
        } else if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            push(&mut out, &mut current, false, bold, italic);
            bold = !bold;
            i += 2;
        } else if c == '*' || (c == '_' && (i == 0 || !chars[i - 1].is_alphanumeric())) {
            push(&mut out, &mut current, false, bold, italic);
            italic = !italic;
            i += 1;
        } else if c == '[' {
            // [text](url) → text
            let close = chars[i..].iter().position(|c| *c == ']').map(|p| p + i);
            match close {
                Some(close) if chars.get(close + 1) == Some(&'(') => {
                    let end = chars[close..]
                        .iter()
                        .position(|c| *c == ')')
                        .map(|p| p + close);
                    current.extend(&chars[i + 1..close]);
                    i = end.map(|e| e + 1).unwrap_or(chars.len());
                }
                _ => {
                    current.push(c);
                    i += 1;
                }
            }
        } else {
            current.push(c);
            i += 1;
        }
    }
    push(&mut out, &mut current, false, bold, italic);
    out
}

/// Renders Markdown into `ui`, wrapping at `width`.
pub fn show(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.vertical(|ui| blocks(ui, text, width));
}

fn blocks(ui: &mut egui::Ui, text: &str, width: f32) {
    let theme = crate::design::theme::current(ui.ctx());
    let p = theme.palette;
    let body = crate::design::tokens::typography::BODY;
    let mono = FontId::monospace(body);
    ui.spacing_mut().item_spacing.y = 6.0;
    for block in parse(text) {
        match block {
            Block::Code(code) => {
                egui::Frame::new()
                    .fill(p.bg_hover)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.set_max_width(width);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(code)
                                    .font(mono.clone())
                                    .color(p.text_primary),
                            )
                            .wrap(),
                        );
                    });
            }
            Block::Heading(heading) => {
                ui.label(egui::RichText::new(heading).strong().color(p.text_primary));
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Bullet(item) => {
                ui.horizontal_top(|ui| {
                    ui.label(egui::RichText::new("•").color(p.text_secondary));
                    paragraph(ui, &item, width - 16.0, &mono, body);
                });
            }
            Block::Paragraph(text) => paragraph(ui, &text, width, &mono, body),
        }
    }
}

fn paragraph(ui: &mut egui::Ui, text: &str, width: f32, mono: &FontId, body: f32) {
    let p = crate::design::theme::current(ui.ctx()).palette;
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for (span, code, bold, italic) in inline(text) {
        let font = if code {
            mono.clone()
        } else if bold {
            FontId::new(
                body,
                crate::design::fonts::family(crate::design::fonts::UI_SEMIBOLD),
            )
        } else {
            FontId::proportional(body)
        };
        job.append(
            &span,
            0.0,
            TextFormat {
                font_id: font,
                color: if code {
                    p.text_primary
                } else {
                    p.text_secondary
                },
                background: if code { p.bg_hover } else { Default::default() },
                italics: italic,
                ..Default::default()
            },
        );
    }
    ui.add(egui::Label::new(job).wrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_inline_spans() {
        let blocks = parse(
            "```php\n<?php\nfunction add(int $a): int\n```\nAdds **two** numbers, see `sum()`.\n\n@param int $a\n- one\n# Title",
        );
        assert_eq!(blocks[0], Block::Code("function add(int $a): int".into()));
        assert_eq!(
            blocks[1],
            Block::Paragraph("Adds **two** numbers, see `sum()`.".into())
        );
        assert_eq!(blocks[2], Block::Paragraph("@param int $a".into()));
        assert_eq!(blocks[3], Block::Bullet("one".into()));
        assert_eq!(blocks[4], Block::Heading("Title".into()));

        let spans = inline("Adds **two** numbers, see `sum()` [docs](http://x).");
        assert!(spans.contains(&("two".into(), false, true, false)));
        assert!(spans.contains(&("sum()".into(), true, false, false)));
        assert!(spans.iter().any(|s| s.0.contains("docs")));
        assert!(!spans.iter().any(|s| s.0.contains("http")));
    }
}
