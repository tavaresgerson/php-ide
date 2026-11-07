//! Inlay hints e Code Vision — `T-UI-ED-005-01..06` (REQUISITOS UI-ED-005).
//!
//! - [`HintStore`]: provedor **assíncrono** por revisão — resposta antiga é
//!   descartada (aceite UI-ED-005), hints nunca entram no documento (são
//!   pintados à parte: save/busca/clipboard intactos);
//! - [`HintSettings`]: toggle global, All arguments, Pass by reference e
//!   **exclude list**;
//! - [`CodeVision`]: Usages (LSP references → resultados reais), Inheritors
//!   (type hierarchy quando o servidor oferecer; sem provedor ⇒ oculto, sem
//!   contador fictício) e Code author agregado do **blame real** do Git.

use std::collections::BTreeMap;

/// Um hint posicionado (fim de uma linha, após o código).
#[derive(Debug, Clone, PartialEq)]
pub struct InlayHint {
    pub line0: usize,
    /// Texto exibido (ex.: `name: `).
    pub label: String,
}

/// Revisão do documento que gerou os hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Revision(pub u64);

/// Resposta do provedor (LSP ou teste).
#[derive(Debug, Clone, PartialEq)]
pub struct HintResponse {
    pub revision: Revision,
    pub hints: Vec<InlayHint>,
    /// Mensagem de status para servidor ausente/erro (explicativa, aceite).
    pub status: Option<String>,
}

/// Store com descarte de respostas velhas (UI-ED-005-01).
#[derive(Debug, Clone, Default)]
pub struct HintStore {
    /// Revisão corrente (None = invalidado).
    pub current: Option<Revision>,
    hints: Vec<InlayHint>,
    pub status: Option<String>,
}

impl HintStore {
    /// Aplica a resposta se a revisão ainda for a corrente; senão descarta
    /// (resposta antiga de provedor lento).
    pub fn apply(&mut self, resp: HintResponse) -> bool {
        match self.current {
            Some(cur) if cur == resp.revision => {
                self.hints = resp.hints;
                self.status = resp.status;
                true
            }
            _ => false,
        }
    }

    pub fn begin(&mut self, revision: Revision) {
        self.current = Some(revision);
        self.hints.clear();
        self.status = None;
    }

    pub fn invalidate(&mut self) {
        self.current = None;
        self.hints.clear();
    }

    pub fn hints(&self) -> &[InlayHint] {
        &self.hints
    }
}

/// Configurações de hints (UI-ED-005-04).
#[derive(Debug, Clone, PartialEq)]
pub struct HintSettings {
    /// Toggle global.
    pub enabled: bool,
    /// Parameter name hints para todos os argumentos.
    pub all_arguments: bool,
    /// Mostrar hints de pass-by-reference (`&$x`).
    pub pass_by_reference: bool,
    /// Exclude list: substring do label (case-insensitive) some.
    pub exclude: Vec<String>,
}

impl Default for HintSettings {
    fn default() -> Self {
        HintSettings {
            enabled: true,
            all_arguments: true,
            pass_by_reference: false,
            exclude: Vec::new(),
        }
    }
}

impl HintSettings {
    /// Filtro aplicado sobre os hints do provedor.
    pub fn filter(&self, hints: &[InlayHint]) -> Vec<InlayHint> {
        if !self.enabled {
            return Vec::new();
        }
        hints
            .iter()
            .filter(|h| {
                if !self.all_arguments && h.label.trim_end_matches(':').trim().is_empty() {
                    return false;
                }
                if !self.pass_by_reference && h.label.contains('&') {
                    return false;
                }
                !self
                    .exclude
                    .iter()
                    .any(|e| !e.is_empty() && h.label.to_lowercase().contains(&e.to_lowercase()))
            })
            .cloned()
            .collect()
    }
}

/// Localização LSP convertida (arquivo + linha 0-based).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageLocation {
    pub path: String,
    pub line0: usize,
    pub preview: String,
}

/// Modelo de Code Vision acima das declarações.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodeVision {
    /// line0 da declaração → texto do lens.
    pub lenses: BTreeMap<usize, Vec<String>>,
}

impl CodeVision {
    /// Usages com resultados reais: `references` do LSP alimenta o lens e a
    /// navegação abre a lista (nunca contador estático sem destino).
    pub fn set_usages(&mut self, line0: usize, locations: &[UsageLocation]) {
        let label = if locations.is_empty() {
            "0 usages".to_string()
        } else {
            format!("{} usages ›", locations.len())
        };
        self.lenses.entry(line0).or_default().push(label);
    }

    /// Inheritors: só entra lens quando há provedor (type hierarchy); sem
    /// provedor o item simplesmente não aparece (aceite UI-ED-005).
    #[allow(dead_code)]
    pub fn set_inheritors(&mut self, line0: usize, count: usize) {
        if count > 0 {
            self.lenses
                .entry(line0)
                .or_default()
                .push(format!("{count} inheritors ›"));
        }
    }

    /// Code author por declaração a partir do blame **real** (blame entries
    /// cobrindo as linhas do bloco). Sem dados ⇒ sem lens.
    pub fn set_code_author<'a>(
        &mut self,
        line0: usize,
        block_start: usize,
        block_end: usize,
        mut blame_in_range: impl FnMut(usize) -> Option<(&'a str, i64)>,
    ) {
        let mut authors: Vec<&str> = Vec::new();
        let mut latest: i64 = i64::MIN;
        let mut latest_author: Option<&str> = None;
        for line in block_start..=block_end {
            if let Some((author, ts)) = blame_in_range(line) {
                if !authors.contains(&author) {
                    authors.push(author);
                }
                if ts > latest {
                    latest = ts;
                    latest_author = Some(author);
                }
            }
        }
        if let Some(who) = latest_author {
            let label = if authors.len() > 1 {
                format!("{who} +{} autores", authors.len() - 1)
            } else {
                who.to_string()
            };
            self.lenses.entry(line0).or_default().push(label);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_discards_stale_responses() {
        let mut s = HintStore::default();
        s.begin(Revision(7));
        // Resposta da revisão 6 (antiga): descartada.
        assert!(!s.apply(HintResponse {
            revision: Revision(6),
            hints: vec![InlayHint {
                line0: 1,
                label: "velho".into()
            }],
            status: None,
        }));
        assert!(s.hints().is_empty());
        // Resposta corrente: aplicada.
        assert!(s.apply(HintResponse {
            revision: Revision(7),
            hints: vec![InlayHint {
                line0: 2,
                label: "name: ".into()
            }],
            status: None,
        }));
        assert_eq!(s.hints().len(), 1);
        // Nova revisão sem resposta ainda: hints limpos (não exibe dados velhos).
        s.begin(Revision(8));
        assert!(s.hints().is_empty());
    }

    #[test]
    fn settings_filter_all_byref_and_exclude() {
        let hs = HintSettings::default();
        let hints = vec![
            InlayHint {
                line0: 0,
                label: "name: ".into(),
            },
            InlayHint {
                line0: 1,
                label: "&ref: ".into(),
            },
            InlayHint {
                line0: 2,
                label: "internal: ".into(),
            },
        ];
        let out = hs.filter(&hints);
        assert_eq!(out.len(), 2, "pass-by-reference some por default: {out:?}");
        let hs = HintSettings {
            pass_by_reference: true,
            exclude: vec!["internal".into()],
            ..Default::default()
        };
        let out = hs.filter(&hints);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.iter().any(|h| h.label.contains('&')));
        assert!(!out.iter().any(|h| h.label.contains("internal")));
        // Toggle global: nada.
        let hs = HintSettings {
            enabled: false,
            ..Default::default()
        };
        assert!(hs.filter(&hints).is_empty());
    }

    #[test]
    fn code_vision_real_data_only() {
        let mut cv = CodeVision::default();
        // Sem provedor: nada de inheritors.
        cv.set_inheritors(3, 0);
        assert!(!cv.lenses.contains_key(&3));
        // Usages reais.
        let locs = vec![
            UsageLocation {
                path: "a.php".into(),
                line0: 9,
                preview: "x".into(),
            },
            UsageLocation {
                path: "b.php".into(),
                line0: 4,
                preview: "y".into(),
            },
        ];
        cv.set_usages(3, &locs);
        assert!(cv.lenses[&3][0].starts_with("2 usages"));
        // Code author do blame real: bloco linhas 10..12 → Ana (mais recente).
        let blame = |l: usize| match l {
            10 => Some(("Ana", 100)),
            11 => Some(("Beto", 200)),
            12 => Some(("Ana", 300)),
            _ => None,
        };
        cv.set_code_author(5, 10, 12, blame);
        assert_eq!(cv.lenses[&5], vec!["Ana +1 autores"]);
        // Blame vazio (sem provedor/git): sem lens — ausência não é sucesso.
        let mut cv2 = CodeVision::default();
        cv2.set_code_author(1, 0, 3, |_| None::<(&str, i64)>);
        assert!(!cv2.lenses.contains_key(&1));
    }
}
