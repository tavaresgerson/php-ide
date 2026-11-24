//! Lens preview and Inspections widget helpers (REQUISITOS UI-ED-006).
//!
//! Scrollbar markers moved into `phpide-editor`, which paints them with the
//! rest of the editor surface.

use eframe::egui;

pub fn popup_position(pointer: egui::Pos2, viewport: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    let x = (viewport.right() - size.x - 12.0).max(viewport.left());
    let max_y = (viewport.bottom() - size.y).max(viewport.top());
    let y = (pointer.y + 12.0).clamp(viewport.top(), max_y);
    egui::pos2(x, y)
}

pub fn markers_are_current(cached_revision: u64, current_revision: u64, dirty: bool) -> bool {
    !dirty && cached_revision == current_revision
}

/// Contagens do widget Inspections por severidade.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionsSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub hints: usize,
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
    fn popup_stays_inside_the_viewport() {
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(500.0, 200.0));
        let size = egui::vec2(300.0, 100.0);
        assert_eq!(
            popup_position(egui::pos2(490.0, 50.0), viewport, size).y,
            62.0
        );
        assert_eq!(
            popup_position(egui::pos2(490.0, 215.0), viewport, size).y,
            120.0
        );
        assert_eq!(
            popup_position(egui::pos2(490.0, 0.0), viewport, size).y,
            20.0
        );
        assert!(markers_are_current(4, 4, false));
        assert!(!markers_are_current(4, 5, false));
        assert!(!markers_are_current(4, 4, true));
    }

    #[test]
    fn inspections_summary_labels() {
        let s = InspectionsSummary {
            errors: 2,
            warnings: 1,
            ..Default::default()
        };
        assert_eq!(s.total(), 3);
        assert_eq!(s.label(), "✗ 2  ⚠ 1");
        let clean = InspectionsSummary::default();
        assert_eq!(clean.label(), "");
        assert_eq!(clean.total(), 0);
    }
}
