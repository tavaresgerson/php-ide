//! Switcher `Ctrl+Tab` com ordem MRU — `T-UI-ED-003-02` (REQUISITOS UI-ED-003).
//!
//! Modelo puro (testável) + popup egui. O switcher lista os buffers abertos
//! do mais recentemente usado para o menos; segurar Ctrl e repetir Tab anda
//! na lista; soltar Ctrl confirma o foco (padrão IntelliJ).

use std::collections::HashMap;

use eframe::egui;

/// Ordem MRU dos buffers (por chave estável — path ou id).
#[derive(Debug, Clone, Default)]
pub struct MruSwitcher {
    /// Mais recente primeiro.
    stack: Vec<String>,
    /// Posição do cursor durante o ciclo (None = switcher fechado).
    cycling: Option<usize>,
}

impl MruSwitcher {
    /// Registra uso de um buffer (abertura, foco, edição).
    pub fn touch(&mut self, key: &str) {
        self.stack.retain(|k| k != key);
        self.stack.insert(0, key.to_string());
        self.cycling = None;
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, key: &str) {
        self.stack.retain(|k| k != key);
        if let Some(pos) = self.cycling {
            if pos >= self.stack.len() {
                self.cycling = None;
            }
        }
    }

    /// Abre o ciclo (Ctrl+Tab pressionado pela primeira vez).
    pub fn begin_cycle(&mut self) {
        if self.cycling.is_none() {
            self.cycling = Some(0);
        }
    }

    /// Próximo item do ciclo (Tab repetido com Ctrl segurado).
    pub fn cycle_next(&mut self) {
        if self.stack.is_empty() {
            return;
        }
        let cur = self.cycling.unwrap_or(0);
        self.cycling = Some((cur + 1).min(self.stack.len() - 1));
    }

    /// Item selecionado no ciclo atual.
    pub fn selected(&self) -> Option<&str> {
        self.cycling
            .and_then(|i| self.stack.get(i).map(String::as_str))
    }

    /// Confirma (Ctrl solto): devolve o item selecionado e fecha o ciclo.
    pub fn confirm(&mut self) -> Option<String> {
        let sel = self.selected().map(str::to_string);
        self.cycling = None;
        sel
    }

    #[allow(dead_code)]
    pub fn cancel(&mut self) {
        self.cycling = None;
    }

    pub fn is_cycling(&self) -> bool {
        self.cycling.is_some()
    }

    /// Lista na ordem MRU (para o popup).
    pub fn ordered(&self) -> &[String] {
        &self.stack
    }
}

/// Popup do switcher sobre o editor. `labels` traduz chave → rótulo.
pub fn popup(
    ctx: &egui::Context,
    switcher: &MruSwitcher,
    labels: &HashMap<String, String>,
    selected_visual: usize,
) {
    if !switcher.is_cycling() {
        return;
    }
    egui::Area::new(egui::Id::new("tab-switcher"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.label(egui::RichText::new("Switcher (Ctrl+Tab)").strong().small());
                ui.separator();
                for (i, key) in switcher.ordered().iter().enumerate() {
                    let label = labels.get(key).cloned().unwrap_or_else(|| key.clone());
                    let text = if Some(i) == switcher.cycling {
                        egui::RichText::new(label).strong()
                    } else {
                        egui::RichText::new(label).weak()
                    };
                    ui.label(text);
                }
            });
        });
    let _ = selected_visual;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mru_order_and_cycle() {
        let mut s = MruSwitcher::default();
        s.touch("a");
        s.touch("b");
        s.touch("c");
        assert_eq!(s.ordered(), &["c", "b", "a"]);
        // Re-toca a: vai ao topo.
        s.touch("a");
        assert_eq!(s.ordered(), &["a", "c", "b"]);
        // Ciclo: começa no mais recente, anda e para no fim.
        s.begin_cycle();
        assert_eq!(s.selected(), Some("a"));
        s.cycle_next();
        assert_eq!(s.selected(), Some("c"));
        s.cycle_next();
        assert_eq!(s.selected(), Some("b"));
        s.cycle_next(); // no limite: não passa
        assert_eq!(s.selected(), Some("b"));
        // Confirma.
        assert_eq!(s.confirm().as_deref(), Some("b"));
        assert!(!s.is_cycling());
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn remove_keeps_cycle_valid() {
        let mut s = MruSwitcher::default();
        s.touch("x");
        s.touch("y");
        s.begin_cycle();
        s.cycle_next();
        s.remove("x");
        s.remove("y");
        assert_eq!(s.selected(), None);
        s.cancel();
        assert!(!s.is_cycling());
    }
}
