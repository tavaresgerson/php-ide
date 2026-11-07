//! Line markers derivados do diff — `T-GIT-004-01` (REQUISITOS GIT-004).
//!
//! Marcadores de gutter por linha (1-based no arquivo **novo**):
//! - **Added**: linha adicionada;
//! - **Modified**: par removido+adicionado no mesmo trecho (del seguida de
//!   add no hunk) — o gutter da IDE pinta de azul;
//! - **Removed**: remoção sem par; a âncora é a linha do arquivo novo onde a
//!   remoção ocorreu (a próxima linha existente, ou a última vista no hunk).
//!
//! Os markers derivam sempre do diff corrente: a UI recalcula em save,
//! stage e commit e o mapa reflete o estado real (não há cache a invalidar).

use std::collections::BTreeMap;
use std::path::Path;

use phpide_core::IdeError;

use crate::diff::{diff_head, DiffLineKind, FileDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMarker {
    Added,
    Modified,
    Removed,
}

impl LineMarker {
    pub const fn as_str(self) -> &'static str {
        match self {
            LineMarker::Added => "added",
            LineMarker::Modified => "modified",
            LineMarker::Removed => "removed",
        }
    }
}

/// Mapa linha (1-based, arquivo novo) → marcador, ordenado por linha.
pub fn markers(diff: &FileDiff) -> BTreeMap<u32, LineMarker> {
    let mut out = BTreeMap::new();
    for hunk in &diff.hunks {
        let lines = &hunk.lines;
        let mut i = 0;
        // Última linha nova vista (contexto/add) para âncora de remoções
        // no fim do hunk.
        let mut last_new: Option<u32> = None;
        while i < lines.len() {
            if lines[i].kind == DiffLineKind::Context {
                last_new = lines[i].new_lineno.or(last_new);
                i += 1;
                continue;
            }
            // Trecho de mudança: junta as del/add consecutivas.
            let start = i;
            while i < lines.len() && lines[i].kind != DiffLineKind::Context {
                i += 1;
            }
            let seg = &lines[start..i];
            let dels: Vec<_> = seg.iter().filter(|l| l.kind == DiffLineKind::Del).collect();
            let adds: Vec<_> = seg.iter().filter(|l| l.kind == DiffLineKind::Add).collect();
            // Pares del+add (na ordem) = Modified; excedente vira Added.
            let paired = dels.len().min(adds.len());
            for (k, a) in adds.iter().enumerate() {
                let kind = if k < paired {
                    LineMarker::Modified
                } else {
                    LineMarker::Added
                };
                if let Some(nl) = a.new_lineno {
                    out.insert(nl, kind);
                    last_new = Some(nl);
                }
            }
            // Excedente de remoções: âncora na próxima linha nova existente
            // depois do trecho; se não houver, na última vista (ou no início
            // do hunk) — remoção no fim do arquivo marca a linha anterior.
            let leftover = dels.len().saturating_sub(paired);
            if leftover > 0 {
                let anchor = lines[i..]
                    .iter()
                    .find_map(|l| l.new_lineno)
                    .or(last_new)
                    .unwrap_or_else(|| hunk.new_start.max(1));
                out.insert(anchor, LineMarker::Removed);
            }
        }
    }
    out
}

/// Markers correntes de um arquivo vs HEAD (working tree + index; untracked
/// vira arquivo 100% adicionado). Mapa vazio = arquivo sem mudanças. A UI
/// recalcula após save/stage/commit — o mapa reflete o estado real, sem
/// cache a invalidar.
pub fn markers_for_file(root: &Path, path: &Path) -> Result<BTreeMap<u32, LineMarker>, IdeError> {
    Ok(diff_head(root, path)?
        .map(|d| markers(&d))
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{parse_unified, Hunk};

    #[test]
    fn added_lines_map_to_new_lines() {
        let text = "--- a/f.php\n+++ b/f.php\n@@ -1,1 +1,3 @@\n ctx\n+novo1\n+novo2\n";
        let files = parse_unified(text);
        let m = markers(&files[0]);
        assert_eq!(m.get(&2), Some(&LineMarker::Added));
        assert_eq!(m.get(&3), Some(&LineMarker::Added));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn del_plus_add_pairs_become_modified() {
        let text = "--- a/f.php\n+++ b/f.php\n@@ -1,1 +1,1 @@\n-velho\n+novo\n";
        let files = parse_unified(text);
        let m = markers(&files[0]);
        assert_eq!(m.get(&1), Some(&LineMarker::Modified));
    }

    #[test]
    fn pure_deletion_anchors_on_next_existing_line() {
        // Remove 1 linha entre contextos: a próxima linha existente no novo
        // arquivo é a 2 ("tres") — é onde o marcador de remoção ancora.
        let text = "--- a/f.php\n+++ b/f.php\n@@ -1,3 +1,2 @@\n um\n-dois\n tres\n";
        let files = parse_unified(text);
        let m = markers(&files[0]);
        assert_eq!(m.get(&2), Some(&LineMarker::Removed));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn deletion_at_eof_anchors_on_last_seen_line() {
        let text = "--- a/f.php\n+++ b/f.php\n@@ -1,3 +1,2 @@\n um\n dois\n-tres\n";
        let files = parse_unified(text);
        let m = markers(&files[0]);
        assert_eq!(m.get(&2), Some(&LineMarker::Removed));
    }

    #[test]
    fn replace_two_with_one_leaves_removed_and_modified() {
        // ctx, -a, -b, +c, depois ⇒ c Modified (par), remoção extra ancorada
        // na próxima linha existente ("depois", novo 3).
        let text = "--- a/f.php\n+++ b/f.php\n@@ -1,3 +1,2 @@\n ctx\n-a\n-b\n+c\n depois\n";
        let files = parse_unified(text);
        let m = markers(&files[0]);
        assert_eq!(m.get(&2), Some(&LineMarker::Modified));
        assert_eq!(m.get(&3), Some(&LineMarker::Removed));
    }

    #[test]
    fn empty_diff_has_no_markers() {
        let d = FileDiff {
            old_path: "x".into(),
            new_path: "x".into(),
            hunks: vec![Hunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                lines: vec![crate::diff::DiffLine {
                    kind: DiffLineKind::Context,
                    content: "igual".into(),
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                }],
            }],
        };
        assert!(markers(&d).is_empty());
    }
}
