//! Orquestração da inteligência PHP — T-PHP-001/002/004/005.
//!
//! Funções puras e testáveis da camada de integração LSP/Composer:
//! planejamento de sincronização versionada, parsing de localizações,
//! decodificação de semantic tokens, ocorrências e navegação de problemas.
//! A UI (`main.rs`) consome estas funções; o estado volátil fica no app.

use phpide_core::{PositionEncoding, Severity};
use serde_json::Value;
use std::path::PathBuf;

/// Passo de sincronização a enviar ao servidor, na ordem exata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOp {
    Open(PathBuf, u64),
    Change(PathBuf, u64),
    Close(PathBuf),
}

/// Planeja a sincronização do frame (T-PHP-001-03): didOpen antes de
/// didChange, didChange só quando a versão avança, didClose por último.
/// Nunca reprogride versão — `opened` reflete o que o servidor conhece.
pub fn plan_sync(
    buffers: &[(PathBuf, u64)],
    opened: &std::collections::HashMap<PathBuf, u64>,
) -> Vec<SyncOp> {
    let mut ops = Vec::new();
    for (path, version) in buffers {
        match opened.get(path) {
            None => ops.push(SyncOp::Open(path.clone(), *version)),
            Some(v) if v != version => ops.push(SyncOp::Change(path.clone(), *version)),
            _ => {}
        }
    }
    let open_paths: std::collections::HashSet<&PathBuf> = buffers.iter().map(|(p, _)| p).collect();
    for path in opened.keys() {
        if !open_paths.contains(path) {
            ops.push(SyncOp::Close(path.clone()));
        }
    }
    ops
}

/// Uma localização (arquivo + linha/coluna 0-based, como o LSP envia).
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub line0: usize,
    pub col0: usize,
}

/// Converte a resposta de definition/implementation/typeDefinition/
/// references em localizações (aceita Location único, array ou null).
pub fn parse_locations(value: &Value) -> Vec<Location> {
    let array: Vec<&Value> = if value.is_array() {
        value.as_array().unwrap().iter().collect()
    } else if value.is_object() && value.get("uri").is_some() {
        vec![value]
    } else {
        Vec::new()
    };
    array
        .into_iter()
        .filter_map(|loc| {
            let uri = loc["uri"].as_str()?;
            let path = phpide_lsp::uri_to_path(uri)?;
            Some(Location {
                path,
                line0: loc["range"]["start"]["line"].as_u64()? as usize,
                col0: loc["range"]["start"]["character"].as_u64()? as usize,
            })
        })
        .collect()
}

/// Converte a resposta de workspace/symbol em localizações com nome.
pub fn parse_workspace_symbols(value: &Value) -> Vec<(String, Location)> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|sym| {
                    let name = sym["name"].as_str()?.to_string();
                    let loc = sym["location"].as_object()?;
                    let path = phpide_lsp::uri_to_path(loc["uri"].as_str()?)?;
                    Some((
                        name,
                        Location {
                            path,
                            line0: loc["range"]["start"]["line"].as_u64()? as usize,
                            col0: loc["range"]["start"]["character"].as_u64()? as usize,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Token semântico decodificado: (line0, start_char, len_chars, tipo).
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticToken {
    pub line0: usize,
    pub start_char: usize,
    pub len_chars: usize,
    pub token_type: String,
}

/// Decodifica `textDocument/semanticTokens/full` (T-PHP-005-01): o campo
/// `data` é um array flat de 5-u32 (deltaLine, deltaStart, length,
/// tokenType, modifiers), posições em UTF-16. Requer o documento para
/// converter (linha, coluna UTF-16) → chars, pelas conversões centrais.
pub fn decode_semantic_tokens(
    doc: &phpide_core::Document,
    legend_types: &[String],
    data: &[u32],
) -> Vec<SemanticToken> {
    let mut out = Vec::new();
    let mut line0: u32 = 0;
    let mut start_utf16: u32 = 0;
    let mut it = data.iter();
    while let (Some(&delta_line), Some(&delta_start), Some(&len), Some(&type_idx), Some(&_mods)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        // Tupla incompleta (dados truncados) encerra a decodificação.
        if delta_line > 0 {
            line0 += delta_line;
            start_utf16 = delta_start;
        } else {
            start_utf16 += delta_start;
        }
        let Some(token_type) = legend_types.get(type_idx as usize) else {
            continue;
        };
        let line = line0 as usize;
        let line_start = doc.line_start(line.min(doc.line_count().saturating_sub(1)));
        let start_char = line_start
            + doc.lsp_position_to_char(
                line.min(doc.line_count().saturating_sub(1)),
                start_utf16 as usize,
                PositionEncoding::Utf16,
            )
            - line_start;
        // len em unidades UTF-16: converte contando chars da faixa.
        let start_in_line = doc.lsp_position_to_char(
            line.min(doc.line_count().saturating_sub(1)),
            start_utf16 as usize,
            PositionEncoding::Utf16,
        );
        let mut units = 0u32;
        let mut len_chars = 0usize;
        for c in doc
            .line(line.min(doc.line_count().saturating_sub(1)))
            .chars()
            .skip(start_in_line - line_start)
        {
            if units >= len || c == '\n' || c == '\r' {
                break;
            }
            units += c.len_utf16() as u32;
            len_chars += 1;
        }
        out.push(SemanticToken {
            line0: line,
            start_char,
            len_chars: len_chars.max(1),
            token_type: token_type.clone(),
        });
    }
    out
}

/// Intervalo de ocorrência (chars absolutos) para pintura no editor.
pub type CharRange = (usize, usize);

/// Resposta de documentHighlight → ranges absolutos de chars.
pub fn parse_highlights(doc: &phpide_core::Document, value: &Value) -> Vec<CharRange> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let range = &h["range"];
                    let line = range["start"]["line"].as_u64()? as usize;
                    let start_u = range["start"]["character"].as_u64()? as usize;
                    let end_u = range["end"]["character"].as_u64()? as usize;
                    let line = line.min(doc.line_count().saturating_sub(1));
                    let start = doc.lsp_position_to_char(line, start_u, PositionEncoding::Utf16);
                    let end = doc.lsp_position_to_char(line, end_u, PositionEncoding::Utf16);
                    if end > start {
                        Some((start, end))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fallback sintático offline (T-PHP-005-02): ocorrências da palavra sob o
/// cursor por casamento exato de identificador (mesmo critério de `word_at`).
pub fn local_occurrences(text: &[char], word: &str, cursor: usize) -> Vec<CharRange> {
    if word.len() < 2 {
        return Vec::new();
    }
    let is_id = |c: char| c.is_alphanumeric() || matches!(c, '_' | '$' | '\\');
    let n = word.chars().count();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= text.len() {
        let candidate: String = text[i..i + n].iter().collect();
        if candidate == word {
            let before_ok = i == 0 || !is_id(text[i - 1]);
            let after_ok = i + n >= text.len() || !is_id(text[i + n]);
            if before_ok && after_ok {
                out.push((i, i + n));
            }
        }
        i += 1;
    }
    let _ = cursor;
    out
}

/// Índice do próximo problema a visitar (T-PHP-002-03): avança a partir da
/// linha atual, com wrap-around no fim/início.
pub fn next_problem_index(count: usize, current_line1: usize, dir: i32) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(if dir >= 0 {
        (0..count)
            .find(|&i| problem_line(i) > current_line1)
            .unwrap_or(0)
    } else {
        (0..count)
            .rev()
            .find(|&i| problem_line(i) < current_line1)
            .unwrap_or(count - 1)
    })
}

// Helper estável para testes: linha 1-based a partir do índice.
fn problem_line(i: usize) -> usize {
    i + 1
}

/// Resumo de problemas por origem (T-PHP-002-02): (fonte, erros+avisos, total).
pub fn problems_by_source(diags: &[&phpide_core::Diagnostic]) -> Vec<(String, usize, usize)> {
    let mut map: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for d in diags {
        let entry = map.entry(d.source.clone()).or_default();
        entry.1 += 1;
        if matches!(d.severity, Severity::Error | Severity::Warning) {
            entry.0 += 1;
        }
    }
    map.into_iter()
        .map(|(k, (critical, total))| (k, critical, total))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::Document;

    #[test]
    fn sync_plans_open_change_close_in_order() {
        let path_a = PathBuf::from("/p/a.php");
        let path_b = PathBuf::from("/p/b.php");
        let mut opened = std::collections::HashMap::new();
        opened.insert(path_b.clone(), 3);
        opened.insert(PathBuf::from("/p/old.php"), 1);
        let buffers = vec![(path_a.clone(), 1), (path_b.clone(), 5)];
        let ops = plan_sync(&buffers, &opened);
        // didOpen do novo antes de didChange; close por último.
        assert_eq!(
            ops,
            vec![
                SyncOp::Open(path_a.clone(), 1),
                SyncOp::Change(path_b.clone(), 5),
                SyncOp::Close(PathBuf::from("/p/old.php")),
            ]
        );
        // Nenhuma operação quando nada mudou.
        let mut opened2 = std::collections::HashMap::new();
        opened2.insert(path_a.clone(), 1);
        opened2.insert(path_b.clone(), 5);
        assert!(plan_sync(&buffers, &opened2).is_empty());
    }

    #[test]
    fn locations_parse_single_array_and_null() {
        let single = serde_json::json!({
            "uri": "file:///p/a.php",
            "range": { "start": { "line": 2, "character": 4 }, "end": { "line": 2, "character": 8 } }
        });
        let locs = parse_locations(&single);
        assert_eq!(locs.len(), 1);
        assert_eq!(
            locs[0],
            Location {
                path: PathBuf::from("/p/a.php"),
                line0: 2,
                col0: 4
            }
        );
        let array = serde_json::json!([single]);
        assert_eq!(parse_locations(&array).len(), 1);
        assert!(parse_locations(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn workspace_symbols_parse_with_locations() {
        let value = serde_json::json!([
            { "name": "Product", "kind": 5,
              "location": { "uri": "file:///p/src/Product.php",
                            "range": { "start": { "line": 2, "character": 0 } } } },
            { "name": "sem location" }
        ]);
        let syms = parse_workspace_symbols(&value);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].0, "Product");
        assert_eq!(syms[0].1.line0, 2);
    }

    #[test]
    fn semantic_tokens_decode_delta_encoding_with_unicode() {
        // Linha 0: "ab" (0..2); linha 1: "😀x" — 😀 = 2 unidades.
        // Tokens: (0,0,2,'class'), (1,0,2,'variable'), (1,2,1,'variable').
        let doc = Document::new("ab\n😀x\n");
        let types = vec!["class".to_string(), "variable".to_string()];
        let data = [
            0, 0, 2, 0, 0, // linha 0, col 0, len 2, tipo class
            1, 0, 2, 1, 0, // linha 1, col 0, len 2 (😀), tipo variable
            0, 2, 1, 1, 0, // mesma linha, +2 unidades, len 1, variable
        ];
        let tokens = decode_semantic_tokens(&doc, &types, &data);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].line0, 0);
        assert_eq!((tokens[0].start_char, tokens[0].len_chars), (0, 2));
        assert_eq!(tokens[0].token_type, "class");
        // 😀 ocupa chars 3..4 (absoluto): linha 1 começa em char 3.
        assert_eq!((tokens[1].start_char, tokens[1].len_chars), (3, 1));
        assert_eq!((tokens[2].start_char, tokens[2].len_chars), (4, 1));
    }

    #[test]
    fn highlights_parse_to_absolute_char_ranges() {
        let doc = Document::new("<?php\n$nome = 1;\n$nome = 2;\n");
        let value = serde_json::json!([
            { "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 5 } } },
            { "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 2, "character": 5 } } }
        ]);
        let ranges = parse_highlights(&doc, &value);
        assert_eq!(ranges.len(), 2);
        // Linha 1 começa em char 6 ('$nome' = 5 chars); linha 2 em 17
        // (linha 1 tem 10 chars + '\n').
        assert_eq!(ranges[0], (6, 11));
        assert_eq!(ranges[1], (17, 22));
    }

    #[test]
    fn local_occurrences_match_whole_identifiers_only() {
        let text: Vec<char> = "$nome e renomear $nome;".chars().collect();
        let occ = local_occurrences(&text, "$nome", 0);
        assert_eq!(occ.len(), 2, "'renomear' não casa (borda identificador)");
        assert_eq!(occ[0], (0, 5));
    }

    #[test]
    fn problem_navigation_wraps_in_both_directions() {
        assert_eq!(next_problem_index(0, 5, 1), None);
        // 3 problemas nas linhas 1,2,3; cursor na 2.
        assert_eq!(next_problem_index(3, 2, 1), Some(2), "próximo = linha 3");
        assert_eq!(next_problem_index(3, 3, 1), Some(0), "wrap para o primeiro");
        assert_eq!(
            next_problem_index(3, 1, -1),
            Some(2),
            "anterior wrapa para o último"
        );
        assert_eq!(next_problem_index(3, 2, -1), Some(0));
    }

    #[test]
    fn problems_summary_groups_by_source() {
        fn mk(source: &str, sev: Severity) -> phpide_core::Diagnostic {
            phpide_core::Diagnostic {
                path: PathBuf::from("/p/a.php"),
                line: 1,
                col: 1,
                message: "m".into(),
                severity: sev,
                source: source.into(),
            }
        }
        let d1 = mk("intelephense", Severity::Error);
        let d2 = mk("intelephense", Severity::Hint);
        let d3 = mk("tree-sitter", Severity::Warning);
        let diags = vec![&d1, &d2, &d3];
        let summary = problems_by_source(&diags);
        assert_eq!(summary.len(), 2);
        assert!(summary
            .iter()
            .any(|(s, critical, total)| s == "intelephense" && *critical == 1 && *total == 2));
        assert!(summary
            .iter()
            .any(|(s, critical, _)| s == "tree-sitter" && *critical == 1));
    }
}
