//! Lens mode, markers da scrollbar e Inspections — `T-UI-ED-006-01/02/03`
//! (REQUISITOS UI-ED-006).
//!
//! - [`aggregate_scrollbar_markers`]: faixas de diagnósticos **e** mudanças
//!   Git agregadas e limitadas em arquivos grandes (aceite: markers
//!   limitados/agregados; erros de diagnóstico não bloqueiam scroll);
//! - [`lens_preview`]: modelo do preview ancorado à região indicada pelo
//!   hover da scrollbar (a UI pinta o popup com o tema vigente);
//! - [`inspections_summary`]: contagens por severidade → widget que abre
//!   Problems no escopo certo.

use phpide_core::Severity;

/// Faixa agregada na scrollbar: fração 0..1 + cor por categoria.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMarker {
    /// Posição 0..1 na scrollbar.
    pub frac: f32,
    /// 0 = diagnóstico (error/warning), 1 = mudança git (added), 2 = modified/removed.
    pub category: u8,
    pub color: (u8, u8, u8),
}

/// Teto de faixas por categoria em arquivos grandes (UI-ED-006-01).
pub const MAX_MARKERS_PER_CATEGORY: usize = 200;

/// Agrega diagnósticos + markers Git por linha em faixas da scrollbar.
/// `git_markers`: (line1, added?) — modified/removed entram na categoria 2.
pub fn aggregate_scrollbar_markers(
    diagnostics: &[(u32, Severity)],
    git_markers: &[(u32, bool)],
    total_lines: usize,
) -> Vec<ScrollMarker> {
    if total_lines == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut per_cat = [0usize; 3];
    let mut push = |frac: f32, category: u8, color: (u8, u8, u8), per_cat: &mut [usize; 3]| {
        if per_cat[category as usize] >= MAX_MARKERS_PER_CATEGORY {
            return; // limite por categoria em arquivo grande
        }
        per_cat[category as usize] += 1;
        out.push(ScrollMarker {
            frac,
            category,
            color,
        });
    };
    for (line1, sev) in diagnostics {
        if *line1 == 0 {
            continue;
        }
        let frac = (*line1 as f32 - 0.5) / total_lines as f32;
        if !(0.0..=1.0).contains(&frac) {
            continue; // diagnóstico fora do arquivo não quebra o scroll
        }
        let color = match sev {
            Severity::Error => (0xE5, 0x53, 0x4B),
            Severity::Warning => (0xDB, 0xA9, 0x3C),
            Severity::Information => (0x35, 0x74, 0xF0),
            Severity::Hint => (0x6E, 0x6E, 0x6E),
        };
        push(frac, 0, color, &mut per_cat);
    }
    for (line1, added) in git_markers {
        if *line1 == 0 {
            continue;
        }
        let frac = (*line1 as f32 - 0.5) / total_lines as f32;
        if !(0.0..=1.0).contains(&frac) {
            continue;
        }
        if *added {
            push(frac, 1, (0x2E, 0x7D, 0x32), &mut per_cat);
        } else {
            push(frac, 2, (0x21, 0x66, 0xA3), &mut per_cat);
        }
    }
    out.sort_by(|a, b| a.frac.total_cmp(&b.frac));
    out
}

/// Janela do lens preview: linhas centradas no alvo, limitadas.
#[derive(Debug, Clone, PartialEq)]
pub struct LensPreview {
    pub center_line0: usize,
    pub lines: Vec<String>,
}

/// Monta o preview ancorado à região (UI-ED-006-02): até `radius` linhas
/// ao redor de `center_line0` (clamped ao documento).
pub fn lens_preview(doc_lines: &[String], center_line0: usize, radius: usize) -> LensPreview {
    let start = center_line0.saturating_sub(radius);
    let end = (center_line0 + radius).min(doc_lines.len().saturating_sub(1));
    let lines = if doc_lines.is_empty() {
        Vec::new()
    } else {
        doc_lines[start..=end].to_vec()
    };
    LensPreview {
        center_line0,
        lines,
    }
}

/// Linha do documento sob o ponto da scrollbar (0..1 → linha).
pub fn line_at_scrollbar_frac(frac: f32, total_lines: usize) -> usize {
    if total_lines == 0 {
        return 0;
    }
    ((frac.clamp(0.0, 1.0) * total_lines as f32) as usize).min(total_lines - 1)
}

/// Contagens do widget Inspections por severidade.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionsSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub hints: usize,
}

/// Agrega diagnósticos do arquivo ativo (escopo do widget — abrir Problems
/// filtra por esse arquivo; UI-ED-006-03).
pub fn inspections_summary(diagnostics: &[(u32, Severity)]) -> InspectionsSummary {
    let mut s = InspectionsSummary::default();
    for (_, sev) in diagnostics {
        match sev {
            Severity::Error => s.errors += 1,
            Severity::Warning => s.warnings += 1,
            Severity::Information => s.infos += 1,
            Severity::Hint => s.hints += 1,
        }
    }
    s
}

impl InspectionsSummary {
    #[allow(dead_code)]
    pub fn total(&self) -> usize {
        self.errors + self.warnings + self.infos + self.hints
    }

    /// Rótulo compacto da status bar (ex.: `⚠ 2  ✗ 1`); vazio = limpo.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.errors > 0 {
            parts.push(format!("✗ {}", self.errors));
        }
        if self.warnings > 0 {
            parts.push(format!("⚠ {}", self.warnings));
        }
        if self.infos > 0 {
            parts.push(format!("ⓘ {}", self.infos));
        }
        parts.join("  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_diagnostics_and_git_with_limits() {
        let diags = vec![(10u32, Severity::Error), (20, Severity::Warning)];
        let git = vec![(11u32, true), (12, false)];
        let m = aggregate_scrollbar_markers(&diags, &git, 100);
        assert_eq!(m.len(), 4);
        assert_eq!(m[0].category, 0);
        // Fora do arquivo: ignorado sem quebrar.
        let diags_bad = vec![(500u32, Severity::Error), (5, Severity::Hint)];
        let m = aggregate_scrollbar_markers(&diags_bad, &[], 100);
        assert_eq!(m.len(), 1, "diagnóstico inválido não bloqueia os demais");
        // Limite por categoria em arquivo grande.
        let many: Vec<(u32, Severity)> = (1..=500).map(|l| (l, Severity::Warning)).collect();
        let m = aggregate_scrollbar_markers(&many, &[], 10_000);
        assert_eq!(
            m.iter().filter(|x| x.category == 0).count(),
            MAX_MARKERS_PER_CATEGORY
        );
    }

    #[test]
    fn lens_preview_anchors_and_clamps() {
        let doc: Vec<String> = (0..50).map(|i| format!("l{i}")).collect();
        let p = lens_preview(&doc, 25, 3);
        assert_eq!(p.lines.len(), 7);
        assert_eq!(p.lines[0], "l22");
        let p = lens_preview(&doc, 1, 3);
        assert_eq!(p.lines[0], "l0", "clamp no início");
        assert_eq!(line_at_scrollbar_frac(0.5, 50), 25);
        assert_eq!(line_at_scrollbar_frac(1.5, 50), 49, "clamp no fim");
    }

    #[test]
    fn inspections_summary_labels() {
        let s = inspections_summary(&[
            (1, Severity::Error),
            (2, Severity::Error),
            (3, Severity::Warning),
        ]);
        assert_eq!(s.total(), 3);
        assert_eq!(s.label(), "✗ 2  ⚠ 1");
        let clean = inspections_summary(&[]);
        assert_eq!(clean.label(), "");
        assert_eq!(clean.total(), 0);
    }
}
