//! Revisão de documento e invalidação de estruturas derivadas — `T-NFR-002-04` (NFR-002).
//!
//! Toda mutação na rope incrementa a revisão do [`Document`]. Estruturas
//! anexadas ao texto — diagnósticos, folding, outline, breadcrumbs, blocos
//! do tree-sitter — devem ser reanexadas ou invalidadas quando a revisão
//! muda; um resultado calculado sobre o texto antigo nunca pode ser
//! apresentado como se descrevesse o texto atual.

use crate::Document;
use std::path::{Path, PathBuf};

/// Marcador de revisão: cresce a cada mutação do documento.
pub type Revision = u64;

impl Document {
    /// Revisão atual (incrementa a cada `insert`/`delete`, incluindo undo/redo).
    pub fn revision(&self) -> Revision {
        self.version
    }
}

/// Estrutura derivada anexada à revisão de um documento. `get` devolve
/// `None` assim que o documento muda — o chamador recalcula ou apresenta
/// estado "desatualizado" em vez de dados velhos.
#[derive(Debug)]
pub struct Revisioned<T> {
    revision: Revision,
    value: T,
    /// Invalidação explícita (ex.: documento fechado, provedor desligado).
    invalidated: bool,
}

impl<T> Revisioned<T> {
    pub fn new(revision: Revision, value: T) -> Self {
        Revisioned {
            revision,
            value,
            invalidated: false,
        }
    }

    /// Valor se a revisão bater e não houver invalidação explícita.
    pub fn get(&self, doc: &Document) -> Option<&T> {
        if self.invalidated || self.revision != doc.revision() {
            return None;
        }
        Some(&self.value)
    }

    /// `true` quando o valor ainda descreve o documento.
    pub fn is_current(&self, doc: &Document) -> bool {
        self.get(doc).is_some()
    }

    /// Invalida manualmente (fechamento de projeto, troca de provedor).
    pub fn invalidate(&mut self) {
        self.invalidated = true;
    }

    /// Reanexa um valor recalculado a uma nova revisão.
    pub fn update(&mut self, revision: Revision, value: T) {
        self.revision = revision;
        self.value = value;
        self.invalidated = false;
    }

    /// Revisão à qual o valor está anexado.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Acesso bruto ao valor (ignora revisão — só para inspeção/depuração).
    pub fn raw(&self) -> &T {
        &self.value
    }
}

/// Índice de estruturas por arquivo: guarda a revisão na qual cada arquivo
/// foi analisado e invalida entradas cujo arquivo mudou ou desapareceu.
/// Usado pela UI para folding/breadcrumbs/blocos estruturais (NFR-002:
/// "Diagnósticos, folding, breakpoints e breadcrumbs devem ser reanexados
/// ou invalidados após mudanças de texto").
#[derive(Debug, Default)]
pub struct PerFileRevision {
    entries: std::collections::HashMap<PathBuf, Revision>,
}

impl PerFileRevision {
    /// Marca `path` como analisado em `revision`.
    pub fn mark(&mut self, path: &Path, revision: Revision) {
        self.entries.insert(path.to_path_buf(), revision);
    }

    /// `true` se `path` estiver anexado exatamente à revisão de `doc`.
    pub fn is_current(&self, path: &Path, doc: &Document) -> bool {
        self.entries.get(path) == Some(&doc.revision())
    }

    /// Remove entradas de arquivos que não existem mais em `alive`
    /// (abas fechadas, troca de projeto).
    pub fn retain(&mut self, alive: impl Fn(&Path) -> bool) {
        self.entries.retain(|p, _| alive(p));
    }

    /// Limpa tudo (troca de projeto).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_bumps_on_every_mutation() {
        let mut doc = Document::new("a");
        let r0 = doc.revision();
        doc.insert(1, "b");
        let r1 = doc.revision();
        doc.delete(0, 1);
        assert!(doc.revision() > r1 && r1 > r0);
    }

    #[test]
    fn revisioned_get_invalidates_after_edit() {
        let mut doc = Document::new("abc");
        let blocks: Vec<&str> = vec!["block-a"];
        let mut cache = Revisioned::new(doc.revision(), blocks.clone());
        assert_eq!(cache.get(&doc), Some(&blocks));
        doc.insert(3, "!");
        assert!(cache.get(&doc).is_none(), "edição invalida a estrutura");
        let fresh: Vec<&str> = vec!["block-a2"];
        cache.update(doc.revision(), fresh.clone());
        assert_eq!(cache.get(&doc), Some(&fresh));
        cache.invalidate();
        assert!(cache.get(&doc).is_none(), "invalidação explícita");
    }

    #[test]
    fn revisioned_survives_undo_redo_boundaries() {
        let mut doc = Document::new("x");
        let cache = Revisioned::new(doc.revision(), 10u8);
        doc.insert(1, "y");
        assert!(cache.get(&doc).is_none());
        doc.delete(1, 2); // volta ao texto original, mas a REVISÃO não volta
        assert!(
            cache.get(&doc).is_none(),
            "mesmo texto antigo exige reanexo: a revisão mudou"
        );
    }

    #[test]
    fn per_file_revision_tracks_files() {
        let mut index = PerFileRevision::default();
        let mut doc = Document::new("<?php");
        index.mark(Path::new("/p/a.php"), doc.revision());
        assert!(index.is_current(Path::new("/p/a.php"), &doc));
        doc.insert(0, " ");
        assert!(!index.is_current(Path::new("/p/a.php"), &doc));
        index.mark(Path::new("/p/a.php"), doc.revision());
        assert!(index.is_current(Path::new("/p/a.php"), &doc));
        index.retain(|p| p.ends_with("a.php"));
        assert!(index.is_current(Path::new("/p/a.php"), &doc));
        index.retain(|_| false);
        assert!(!index.is_current(Path::new("/p/a.php"), &doc));
        index.mark(Path::new("/p/b.php"), doc.revision());
        index.clear();
        assert!(index.entries.is_empty());
    }
}
