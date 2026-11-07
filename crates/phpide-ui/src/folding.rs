//! Folding — `T-UI-ED-002-02/03/04` (REQUISITOS UI-ED-002).
//!
//! Três peças puras e testadas:
//! - [`FoldVisuals`]: mapa linha-do-documento ↔ linha-visual para os ranges
//!   dobrados — números de linha, sticky, stripes e hit tests passam por ele;
//! - [`FoldProjection`]: texto projetado exibido pelo editor quando há dobras
//!   (bandas `⋯` no lugar das linhas ocultas) com mapeamento bidirecional de
//!   edições — texto, caret e navegação realmente excluem as linhas dobradas;
//! - [`FoldState`]: estado por buffer ancorado por conteúdo da linha inicial
//!   (sobrevive a edições fora da região; ranges inválidos caem).
//!
//! Política de persistência (documentada, UI-ED-002-02): o estado é mantido
//! em memória por buffer e **descartado ao fechar o arquivo**; a reabertura
//! sempre vem desdobrada.

use std::collections::{BTreeMap, BTreeSet};

use phpide_syntax::folding::FoldRange;

/// Marca de início de linha-banda de fold no texto projetado. Caráter de uso
/// privado: não é digitável e identifica a banda de forma inequívoca.
pub const FOLD_MARKER: char = '\u{e000}';

/// Mapa doc↔visual para um conjunto de ranges dobrados.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldVisuals {
    /// start_line0 → end_line0 (inclusivo) das dobras efetivas (apura
    /// aninhamento: só a dobra mais externa esconde linhas).
    folds: BTreeMap<usize, usize>,
}

impl FoldVisuals {
    pub fn new(folded: &[(usize, usize)]) -> Self {
        // Efetividade: dobra aninhada dentro de outra dobrada não adiciona
        // linhas ocultas (a externa já as esconde).
        let mut folds: BTreeMap<usize, usize> = BTreeMap::new();
        for &(start, end) in folded {
            if end <= start {
                continue;
            }
            let swallowed = folds
                .range(..=start)
                .next_back()
                .is_some_and(|(s, e)| *s <= start && end <= *e);
            if !swallowed {
                folds.insert(start, end);
            }
        }
        FoldVisuals { folds }
    }

    pub fn is_empty(&self) -> bool {
        self.folds.is_empty()
    }

    pub fn fold_at(&self, doc_line0: usize) -> Option<(usize, usize)> {
        self.folds
            .range(..=doc_line0)
            .next_back()
            .filter(|(s, e)| **s <= doc_line0 && doc_line0 <= **e)
            .map(|(s, e)| (*s, *e))
    }

    #[allow(dead_code)]
    pub fn is_fold_start(&self, doc_line0: usize) -> bool {
        self.folds.contains_key(&doc_line0)
    }

    pub fn is_hidden(&self, doc_line0: usize) -> bool {
        self.fold_at(doc_line0).is_some_and(|(s, _)| s != doc_line0)
    }

    /// Total de linhas visíveis para `total_doc_lines` linhas no documento.
    /// Cada dobra efetiva: (end - start) linhas ocultas (a banda ocupa 1).
    #[allow(dead_code)]
    pub fn visual_line_count(&self, total_doc_lines: usize) -> usize {
        let hidden: usize = self.folds.iter().map(|(s, e)| e.saturating_sub(*s)).sum();
        total_doc_lines.saturating_sub(hidden)
    }

    /// Linha visual (0-based) da linha do documento. Linha oculta mapeia
    /// para a linha visual da sua dobra (a banda).
    #[allow(dead_code)]
    pub fn doc_to_visual(&self, doc_line0: usize) -> usize {
        let mut visual = doc_line0;
        for (&start, &end) in &self.folds {
            if start >= doc_line0 {
                break;
            }
            if doc_line0 <= end {
                // Dentro da dobra (inclusive a banda): conta o que veio antes.
                return visual - (end - start);
            }
            visual -= end - start;
        }
        visual
    }

    /// Linha do documento da linha visual. Fora de alcance devolve a última.
    #[allow(dead_code)]
    pub fn visual_to_doc(&self, visual_line0: usize, total_doc_lines: usize) -> usize {
        let mut hidden_before = 0usize;
        for (&start, &end) in &self.folds {
            let fold_visual_start = start - hidden_before;
            if visual_line0 <= fold_visual_start {
                return visual_line0 + hidden_before;
            }
            hidden_before += end - start;
        }
        (visual_line0 + hidden_before).min(total_doc_lines.saturating_sub(1))
    }
}

/// Uma banda de fold no texto projetado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldBand {
    pub visual_line0: usize,
    pub doc_start: usize,
    pub doc_end: usize,
    /// Texto exibido na banda (assinatura da região).
    pub signature: String,
}

/// Projeção texto↔documento. Quando não há dobras, é identidade (o editor
/// segue editando o texto real, zero risco).
#[derive(Debug, Clone, PartialEq)]
pub struct FoldProjection {
    pub text: String,
    pub bands: Vec<FoldBand>,
    /// visual_line0 → doc_line0 para linhas de conteúdo (bandas mapeiam para
    /// o doc_start mas são identificáveis por `FoldBand::visual_line0`).
    pub visual_to_doc: Vec<usize>,
    /// doc_line0 → visual_line0.
    pub doc_to_visual: Vec<usize>,
}

/// Projeta o documento: linhas ocultas viram uma banda única por dobra.
#[allow(clippy::needless_range_loop)]
pub fn project(doc_text: &str, visuals: &FoldVisuals) -> FoldProjection {
    let doc_lines: Vec<&str> = doc_text.lines().collect();
    let mut out = String::new();
    let mut bands = Vec::new();
    let mut visual_to_doc = Vec::new();
    let mut doc_to_visual = vec![0usize; doc_lines.len()];
    let mut v = 0usize;
    let mut d = 0usize;
    while d < doc_lines.len() {
        if let Some((start, end)) = visuals.fold_at(d) {
            if start == d {
                let sig = doc_lines
                    .get(d)
                    .map(|l| l.trim().chars().take(60).collect::<String>())
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{FOLD_MARKER} ⋮ {} … ({} linhas)\n",
                    sig,
                    end - start
                ));
                bands.push(FoldBand {
                    visual_line0: v,
                    doc_start: start,
                    doc_end: end,
                    signature: sig,
                });
                visual_to_doc.push(d);
                v += 1;
                for hidden in start..=end {
                    if hidden < doc_lines.len() {
                        doc_to_visual[hidden] = v - 1;
                    }
                }
                d = end + 1;
                continue;
            }
        }
        out.push_str(doc_lines[d]);
        out.push('\n');
        visual_to_doc.push(d);
        doc_to_visual[d] = v;
        v += 1;
        d += 1;
    }
    FoldProjection {
        text: out,
        bands,
        visual_to_doc,
        doc_to_visual,
    }
}

/// Resultado de commit de uma edição no texto projetado.
pub enum ProjectedCommit {
    /// Novo texto do documento (edição aceita).
    NewDocText(String),
    /// Edição só tocou bandas/sem mudança: documento intacto.
    Unchanged,
}

/// Aplica a edição do texto projetado de volta ao documento. Linhas-banda
/// são read-only: hunks que as alteram são descartados. `before`/`after`
/// são o texto projetado anterior e o editado.
pub fn commit(before: &FoldProjection, after_text: &str, doc_text: &str) -> ProjectedCommit {
    if after_text == before.text {
        return ProjectedCommit::Unchanged;
    }
    let before_lines: Vec<&str> = before.text.lines().collect();
    let after_lines: Vec<&str> = after_text.lines().collect();
    // Diff por linha (prefixo comum/sufixo) — suficiente para edições do
    // TextEdit egui, que reemite o texto inteiro.
    let mut pre = 0usize;
    while pre < before_lines.len()
        && pre < after_lines.len()
        && before_lines[pre] == after_lines[pre]
    {
        pre += 1;
    }
    let mut suf = 0usize;
    while suf < before_lines.len().saturating_sub(pre)
        && suf < after_lines.len().saturating_sub(pre)
        && before_lines[before_lines.len() - 1 - suf] == after_lines[after_lines.len() - 1 - suf]
    {
        suf += 1;
    }
    let before_mid = &before_lines[pre..before_lines.len() - suf];
    let after_mid = &after_lines[pre..after_lines.len() - suf];

    // Região do documento afetada: mapeia o início do hunk para doc_line.
    let Some(&first_visual) = before.visual_to_doc.get(pre) else {
        return ProjectedCommit::Unchanged;
    };
    // Hunk tocando banda? Descarta (read-only).
    let band_hit = before
        .bands
        .iter()
        .any(|b| (pre..=pre + before_mid.len().saturating_sub(1)).contains(&b.visual_line0));
    if band_hit {
        return ProjectedCommit::Unchanged;
    }
    let doc_lines: Vec<&str> = doc_text.lines().collect();
    let mut new_doc: Vec<String> = doc_lines.iter().map(|s| s.to_string()).collect();
    let start_doc = first_visual;
    let remove_count = before_mid.len();
    let inserted: Vec<String> = after_mid.iter().map(|s| s.to_string()).collect();
    if start_doc > new_doc.len() {
        return ProjectedCommit::Unchanged;
    }
    new_doc.splice(
        start_doc..(start_doc + remove_count).min(new_doc.len()),
        inserted,
    );
    ProjectedCommit::NewDocText(new_doc.join("\n"))
}

/// Estado de folding de um buffer (UI-ED-002-02).
#[derive(Debug, Clone, Default)]
pub struct FoldState {
    /// ranges da revisão vigente (ordem estável do syntax)
    pub ranges: Vec<FoldRange>,
    /// índices em `ranges` atualmente dobrados
    folded: BTreeSet<usize>,
    /// Âncora de reanexo: (kind, prefixo da linha inicial) — sem número de
    /// linha, que muda a cada edição; o prefixo identifica a região.
    anchors: Vec<(&'static str, String)>,
}

impl FoldState {
    /// Atualiza os ranges para uma nova revisão, reanexando dobras pela
    /// âncora (kind + prefixo da linha inicial). Dobra cuja âncora sumiu cai
    /// (nunca mantém range inválido).
    pub fn update(&mut self, ranges: Vec<FoldRange>, doc_lines: &[String]) {
        let old_anchors = std::mem::take(&mut self.anchors);
        let was_folded: std::collections::HashSet<(&'static str, String)> = self
            .folded
            .iter()
            .filter_map(|&i| old_anchors.get(i).cloned())
            .collect();
        self.ranges = ranges;
        self.anchors = self
            .ranges
            .iter()
            .map(|r| {
                let prefix = doc_lines
                    .get(r.start_line0)
                    .map(|l| l.trim().chars().take(24).collect::<String>())
                    .unwrap_or_default();
                (r.kind, prefix)
            })
            .collect();
        self.folded = self
            .anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| was_folded.contains(*a))
            .map(|(i, _)| i)
            .collect();
    }

    pub fn toggle(&mut self, range_idx: usize) {
        if !self.folded.remove(&range_idx) {
            self.folded.insert(range_idx);
        }
    }

    pub fn is_folded(&self, range_idx: usize) -> bool {
        self.folded.contains(&range_idx)
    }

    /// Visuals correntes (só dobras com range válido na revisão).
    pub fn visuals(&self) -> FoldVisuals {
        let folded: Vec<(usize, usize)> = self
            .folded
            .iter()
            .filter_map(|&i| self.ranges.get(i))
            .map(|r| (r.start_line0, r.end_line0))
            .collect();
        FoldVisuals::new(&folded)
    }

    /// Linha do documento cujo caret caiu em região oculta: devolve a linha
    /// da dobra para revelação (unfold), conforme aceite UI-ED-002.
    #[allow(dead_code)]
    pub fn reveal_target(&self, doc_line0: usize) -> Option<usize> {
        let v = self.visuals();
        if v.is_hidden(doc_line0) {
            v.fold_at(doc_line0).map(|(s, _)| s)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(pairs: &[(usize, usize)]) -> Vec<FoldRange> {
        pairs
            .iter()
            .map(|&(s, e)| FoldRange {
                start_line0: s,
                end_line0: e,
                kind: "block",
            })
            .collect()
    }

    #[test]
    fn visuals_map_doc_and_back_with_nested_folds() {
        // Dobra externa [2..5] e aninhada [3..4]: a interna é engolida.
        let v = FoldVisuals::new(&[(2, 5), (3, 4)]);
        assert_eq!(
            v.visual_line_count(10),
            10 - (5 - 2),
            "só a externa esconde"
        );
        assert!(v.is_hidden(4));
        assert!(!v.is_hidden(2), "linha da banda fica visível");
        assert_eq!(v.doc_to_visual(0), 0);
        assert_eq!(v.doc_to_visual(6), 3, "3 linhas ocultas antes da 6");
        assert_eq!(v.visual_to_doc(2, 10), 2, "a banda é a linha 2 do doc");
        assert_eq!(v.visual_to_doc(3, 10), 6);
    }

    #[test]
    fn projection_hides_lines_and_commit_maps_edits() {
        let doc = "l0\nl1\nclass A {\n  m1\n  m2\n}\nl6\n";
        let v = FoldVisuals::new(&[(2, 5)]);
        let proj = project(doc, &v);
        assert!(proj.text.lines().count() == 4, "{}", proj.text);
        assert!(proj.text.lines().nth(2).unwrap().starts_with(FOLD_MARKER));
        assert_eq!(proj.bands.len(), 1);
        assert_eq!(proj.bands[0].doc_start, 2);
        assert_eq!(proj.bands[0].doc_end, 5);
        // Digita na linha 1 (visual 1 == doc 1): edita "l1" → "l1x".
        let edited = proj.text.replacen("l1\n", "l1x\n", 1);
        match commit(&proj, &edited, doc) {
            ProjectedCommit::NewDocText(t) => {
                assert!(t.lines().nth(1).unwrap() == "l1x", "{t}");
                assert_eq!(t.lines().count(), 7);
            }
            ProjectedCommit::Unchanged => panic!("edição em linha visível foi descartada"),
        }
        // Editar a banda é read-only.
        let edited_band = proj.text.replacen(FOLD_MARKER, "X", 1);
        assert!(
            matches!(commit(&proj, &edited_band, doc), ProjectedCommit::Unchanged),
            "banda é read-only"
        );
        // Unfold (projeção identidade) não é usado aqui; visuals vazio = doc cru.
        let id = project(doc, &FoldVisuals::default());
        assert_eq!(id.text, doc);
    }

    #[test]
    fn state_reanchors_after_edit_and_drops_invalid() {
        let doc: Vec<String> = (0..12).map(|i| format!("l{i}")).collect();
        let mut st = FoldState::default();
        st.update(ranges(&[(3, 6), (8, 9)]), &doc);
        st.toggle(0);
        st.toggle(1);
        assert!(st.is_folded(0) && st.is_folded(1));
        // Edita antes: insere uma linha no topo → ranges viram [(4,7),(9,10)]
        // com os MESMOS prefixos ("l3"→agora na linha 4? não: prefixo muda se
        // a linha mudar; aqui a linha 3 vira 4 com o mesmo texto "l3").
        let doc2: Vec<String> = (0..13usize)
            .map(|i| format!("l{}", i.saturating_sub(1)))
            .collect();
        // doc2[4] == "l3" (a linha "l3" desceu de 3 para 4).
        st.update(ranges(&[(4, 7), (9, 10)]), &doc2);
        assert!(st.is_folded(0), "reanexada pela âncora de conteúdo");
        assert!(st.is_folded(1), "{:?}", st.folded);
        // Âncora some (região apagada): dobra cai.
        let doc3: Vec<String> = (0..4).map(|i| format!("n{i}")).collect();
        st.update(ranges(&[(0, 3)]), &doc3);
        assert!(
            st.folded.is_empty(),
            "range inválido não persiste: {:?}",
            st.folded
        );
    }

    #[test]
    fn reveal_target_for_caret_in_hidden_region() {
        let doc: Vec<String> = (0..10).map(|i| format!("l{i}")).collect();
        let mut st = FoldState::default();
        st.update(ranges(&[(2, 5)]), &doc);
        st.toggle(0);
        assert_eq!(
            st.reveal_target(4),
            Some(2),
            "caret na linha oculta revela a dobra"
        );
        assert_eq!(st.reveal_target(2), None, "banda não é oculta");
        assert_eq!(st.reveal_target(7), None);
    }

    #[test]
    fn projection_commit_insert_inside_visible_region() {
        let doc = "a\nb\nc\n";
        let id = project(doc, &FoldVisuals::default());
        // Insere linha nova no meio (hunk de 0→1 linhas na posição 1).
        let edited = "a\nb\nNOVO\nc\n";
        match commit(&id, edited, doc) {
            ProjectedCommit::NewDocText(t) => {
                assert_eq!(t.lines().nth(2).unwrap(), "NOVO");
                assert_eq!(t.lines().count(), 4);
            }
            ProjectedCommit::Unchanged => panic!("inserção descartada"),
        }
    }
}
