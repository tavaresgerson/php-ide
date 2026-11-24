//! Núcleo do phpide.
//!
//! Camadas (espelham a arquitetura clássica de IDEs — ver `docs/como-construir-um-ide.md`):
//! - [`Document`]: o texto. Usa `rope` para edições O(log n) sem copiar o arquivo inteiro.
//! - [`Buffer`]: um arquivo aberto = Document + caminho + dirty + pilhas undo/redo.
//! - [`Workspace`]: raiz do projeto, conjunto de buffers abertos (abas) e índice da árvore.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ropey::Rope;

pub mod error;
pub mod fixtures;
pub mod fsutil;
pub mod history;
pub mod jobs;
pub mod log;
pub mod offsets;
pub mod redact;
pub mod revision;
pub mod settings;
pub mod text;

pub use error::{Component, ErrorKind, IdeError};
pub use history::{Carets, EditKind};
pub use jobs::{JobHandle, JobId, JobOutcome, JobRegistry, JobState, JobSystem};
pub use log::{Correlation, Level, Logger};
pub use offsets::{negotiate_encoding, PositionEncoding};
pub use revision::{PerFileRevision, Revision, Revisioned};
pub use text::{Assoc, Change, ChangeSet};

/// Um documento de texto editável. Equivalente ao `Document` da IntelliJ Platform.
///
/// Every mutation goes through [`Document::apply`]: the revision grows by one
/// per applied change set (undo included, so it never goes back), and the
/// last changes stay in a short log that incremental consumers (syntax tree,
/// other editor views, LSP) replay with [`Document::changes_since`].
#[derive(Debug)]
pub struct Document {
    rope: Rope,
    /// Incrementa a cada mutação; útil para invalidar caches futuros (highlight, LSP).
    pub version: u64,
    log: std::collections::VecDeque<(Revision, std::sync::Arc<text::AppliedChange>)>,
}

/// How many applied change sets the document remembers.
const CHANGE_LOG_CAP: usize = 512;

impl Document {
    pub fn new(text: &str) -> Self {
        Document {
            rope: Rope::from_str(text),
            version: 0,
            log: std::collections::VecDeque::new(),
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self::new(&text))
    }

    /// The text as a rope (cheap to clone, O(log n) slicing).
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Converte deslocamento em caracteres para (linha, coluna), ambas 1-based.
    pub fn line_col(&self, char_offset: usize) -> (usize, usize) {
        let line0 = self.rope.char_to_line(char_offset);
        let line_start = self.rope.line_to_char(line0);
        (line0 + 1, char_offset - line_start + 1)
    }

    /// Deslocamento em caracteres do início da linha (0-based).
    pub fn line_start(&self, line0: usize) -> usize {
        self.rope.line_to_char(line0)
    }

    /// Applies a change set; returns it with its byte edits, or `None` when
    /// it changes nothing.
    pub fn apply(
        &mut self,
        changes: &text::ChangeSet,
    ) -> Option<std::sync::Arc<text::AppliedChange>> {
        if changes.is_empty() {
            return None;
        }
        let byte_edits = text::apply(&mut self.rope, changes);
        self.version += 1;
        let applied = std::sync::Arc::new(text::AppliedChange {
            changes: changes.clone(),
            byte_edits,
        });
        self.log.push_back((self.version, applied.clone()));
        if self.log.len() > CHANGE_LOG_CAP {
            self.log.pop_front();
        }
        Some(applied)
    }

    /// Change sets applied after `revision`, oldest first. `None` when the
    /// log no longer reaches back that far (the caller must resync fully).
    pub fn changes_since(
        &self,
        revision: Revision,
    ) -> Option<Vec<std::sync::Arc<text::AppliedChange>>> {
        if revision == self.version {
            return Some(Vec::new());
        }
        if revision > self.version {
            return None;
        }
        let first = self.log.front()?.0;
        if first > revision + 1 {
            return None;
        }
        Some(
            self.log
                .iter()
                .filter(|(rev, _)| *rev > revision)
                .map(|(_, change)| change.clone())
                .collect(),
        )
    }

    /// Insere `text` no deslocamento em caracteres `at`.
    pub fn insert(&mut self, at: usize, text: &str) {
        self.apply(&text::ChangeSet::single(at, at, text));
    }

    /// Remove o intervalo em caracteres `[start, end)`.
    pub fn delete(&mut self, start: usize, end: usize) {
        self.apply(&text::ChangeSet::single(start, end, ""));
    }

    /// Texto da linha `line0` (0-based), incluindo o `\n` final se houver.
    pub fn line(&self, line0: usize) -> String {
        self.rope.line(line0).to_string()
    }

    pub fn char(&self, offset: usize) -> char {
        self.rope.char(offset)
    }

    /// Smallest change set turning this text into `new_text` (common prefix
    /// and suffix kept), so carets, folds and the syntax tree survive
    /// whole-text replacements such as formatting.
    pub fn diff_to(&self, new_text: &str) -> text::ChangeSet {
        let old: Vec<char> = self.rope.chars().collect();
        let new: Vec<char> = new_text.chars().collect();
        let mut start = 0;
        while start < old.len() && start < new.len() && old[start] == new[start] {
            start += 1;
        }
        let (mut old_end, mut new_end) = (old.len(), new.len());
        while old_end > start && new_end > start && old[old_end - 1] == new[new_end - 1] {
            old_end -= 1;
            new_end -= 1;
        }
        text::ChangeSet::single(
            start,
            old_end,
            new[start..new_end].iter().collect::<String>(),
        )
    }
}

static NEXT_BUFFER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Um arquivo aberto na IDE: documento + metadados + histórico.
pub struct Buffer {
    /// Unique for the process lifetime (editor views and syntax state key on it).
    pub id: u64,
    pub document: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Aba de pré-visualização (itálico): aberta por clique na árvore/palette e
    /// reutilizada pela próxima abertura até ser editada (Closing Policy do PhpStorm).
    pub preview: bool,
    /// Aba fixada: nunca entra na política de fechamento automático.
    pub pinned: bool,
    /// Somente leitura (cadeado na status bar): edições são bloqueadas.
    pub readonly: bool,
    history: history::History,
    /// History state of the text on disk (see [`history::History::state_id`]).
    saved_state: u64,
    /// Salto de cursor pendente (line0, col0), aplicado quando o editor desenhar.
    pending_cursor: Option<(usize, usize)>,
}

impl Buffer {
    fn with_document(document: Document, path: Option<PathBuf>) -> Self {
        Buffer {
            id: NEXT_BUFFER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            document,
            path,
            dirty: false,
            preview: false,
            pinned: false,
            readonly: false,
            history: history::History::new(),
            saved_state: 0,
            pending_cursor: None,
        }
    }

    pub fn untitled() -> Self {
        Buffer::with_document(Document::new("<?php\n\n"), None)
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Buffer::with_document(
            Document::open(path)?,
            Some(path.to_path_buf()),
        ))
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.php".to_string())
    }

    /// Applies an edit and records it for undo. `before`/`after` are the
    /// carets of the editor view that made it (empty for external edits).
    pub fn edit(
        &mut self,
        changes: text::ChangeSet,
        kind: history::EditKind,
        before: history::Carets,
        after: history::Carets,
    ) -> Option<std::sync::Arc<text::AppliedChange>> {
        if changes.is_empty() {
            return None;
        }
        let inverse = changes.invert(self.document.rope());
        let ends_word = kind == history::EditKind::Typing
            && changes.changes().iter().any(|c| c.text.ends_with('\n'));
        let applied = self.document.apply(&changes)?;
        self.history.record(
            changes,
            inverse,
            kind,
            before,
            after,
            std::time::Instant::now(),
            ends_word,
        );
        self.dirty = self.history.state_id() != self.saved_state;
        // Editar promove a aba de pré-visualização a permanente.
        self.preview = false;
        Some(applied)
    }

    pub fn insert(&mut self, at: usize, text: &str) {
        self.edit(
            text::ChangeSet::single(at, at, text),
            history::EditKind::Other,
            Vec::new(),
            Vec::new(),
        );
    }

    pub fn delete(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.edit(
            text::ChangeSet::single(start, end, ""),
            history::EditKind::Other,
            Vec::new(),
            Vec::new(),
        );
    }

    fn replay(&mut self, replay: history::Replay) -> history::Carets {
        for changes in &replay.changes {
            self.document.apply(changes);
        }
        self.dirty = self.history.state_id() != self.saved_state;
        replay.carets
    }

    /// Undoes the last step; returns the carets to restore (empty when the
    /// step came from outside an editor view).
    pub fn undo(&mut self) -> Option<history::Carets> {
        let replay = self.history.undo()?;
        Some(self.replay(replay))
    }

    pub fn redo(&mut self) -> Option<history::Carets> {
        let replay = self.history.redo()?;
        Some(self.replay(replay))
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The next edit starts a new undo step (caret moved, focus lost…).
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Salva o buffer de forma atômica (NFR-007): o arquivo nunca fica
    /// parcialmente escrito; em falha, o buffer permanece intacto e sujo
    /// (nada de perda silenciosa de edição).
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no path"))?;
        fsutil::write_atomic(&path, self.document.text().as_bytes())?;
        self.dirty = false;
        self.saved_state = self.history.state_id();
        self.history.break_group();
        Ok(())
    }

    /// Como `save`, mas com erro padronizado (componente/operação/ação, NFR-005).
    pub fn try_save(&mut self) -> Result<(), IdeError> {
        self.save().map_err(|e| {
            let what = self
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "untitled".to_string());
            IdeError::from_io(Component::Core, "save", &what, &e)
                .with_detail("your edits are kept in memory and the file stays modified")
        })
    }

    /// Replaces the whole text as one undo step, changing only the part
    /// that differs.
    pub fn replace_all(&mut self, text: &str) {
        let changes = self.document.diff_to(text);
        self.edit(changes, history::EditKind::Other, Vec::new(), Vec::new());
    }
}

/// Entrada achatada da árvore de arquivos (ordem de travessia, com profundidade).
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
}

/// Pastas ignoradas na varredura (dependências e metadados, não código do usuário).
const SKIP_DIRS: [&str; 6] = [".git", ".idea", "target", "node_modules", "vendor", "build"];

/// Safety net against pathological trees; symlinked folders are not
/// followed, so loops cannot happen through links.
const MAX_SCAN_DEPTH: usize = 64;

fn scan_into(root: &Path, dir: &Path, depth: usize, out: &mut Vec<FileEntry>) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for item in items {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        // file_type() does not follow symlinks: a linked folder is listed
        // but not descended into.
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && (SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.')) {
            continue;
        }
        out.push(FileEntry {
            path: path.clone(),
            depth,
            is_dir,
        });
        if is_dir {
            scan_into(root, &path, depth + 1, out);
        }
    }
    let _ = root;
}

/// Varredura determinística da raiz do projeto.
pub fn scan(root: &Path) -> Vec<FileEntry> {
    let mut out = Vec::new();
    scan_into(root, root, 0, &mut out);
    out
}

/// Estado global da sessão: raiz, abas e índice de arquivos.
pub struct Workspace {
    pub root: PathBuf,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub files: Vec<FileEntry>,
    /// Diagnósticos agregados de todos os arquivos (LSP + sintaxe offline).
    pub diagnostics: Vec<Diagnostic>,
    /// Revisão do documento à qual cada conjunto de diagnósticos se aplica
    /// (invalidação por revisão, NFR-002).
    diag_revisions: std::collections::HashMap<PathBuf, Revision>,
    /// Outline do arquivo ativo (LSP documentSymbol).
    pub outline: Vec<OutlineEntry>,
    /// Painel de problemas visível?
    pub problems_visible: bool,
    /// Mensagem transitória para a barra de status (erros, confirmações).
    pub message: String,
    /// Pilha "Back" de navegação (Alt+←): (path, line0, col0).
    pub nav_back: Vec<(PathBuf, usize, usize)>,
    /// Pilha "Forward" (Alt+→), esvaziada a cada navegação nova.
    pub nav_forward: Vec<(PathBuf, usize, usize)>,
    /// Arquivos recentes (MRU) para Search Everywhere / Recent Files (Ctrl+E).
    pub recent_files: Vec<PathBuf>,
    /// Limite da Closing Policy (UI-ED-003): máximo de abas antes de fechar a
    /// mais antiga não fixada. Default: [`TAB_LIMIT`]; configurável via
    /// Settings (`tab_limit`).
    pub tab_limit: usize,
    /// New tabs go to the end instead of next to the active tab.
    pub open_tabs_at_end: bool,
    /// Recently closed files, most recent last (Reopen Closed Tab).
    pub closed_tabs: Vec<PathBuf>,
    /// Files edited in this session, most recent first (Recent Files with
    /// "show edited only").
    pub edited_files: Vec<PathBuf>,
    /// Where the last edits happened, most recent last: (path, line0, col0).
    pub edit_locations: Vec<(PathBuf, usize, usize)>,
}

/// Máximo de abas antes da Closing Policy fechar a mais antiga não fixada
/// (mesmo default do PhpStorm: Editor Tabs | Closing Policy | 10).
pub const TAB_LIMIT: usize = 10;

impl Workspace {
    /// Opens a project: scans the file tree. No buffer is opened.
    pub fn open(root: PathBuf) -> Self {
        let mut ws = Self::unscanned(root);
        ws.files = scan(&ws.root);
        ws
    }

    /// Workspace without a file-tree scan (used while the Welcome screen is
    /// open, before any project is chosen).
    pub fn unscanned(root: PathBuf) -> Self {
        Workspace {
            root,
            buffers: Vec::new(),
            active: 0,
            files: Vec::new(),
            diagnostics: Vec::new(),
            diag_revisions: std::collections::HashMap::new(),
            outline: Vec::new(),
            problems_visible: false,
            message: String::new(),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            recent_files: Vec::new(),
            tab_limit: TAB_LIMIT,
            open_tabs_at_end: false,
            closed_tabs: Vec::new(),
            edited_files: Vec::new(),
            edit_locations: Vec::new(),
        }
    }

    pub fn refresh_files(&mut self) {
        self.files = scan(&self.root);
    }

    pub fn new_file(&mut self) {
        self.buffers.push(Buffer::untitled());
        self.active = self.buffers.len() - 1;
    }

    /// Abre o arquivo; se já estiver aberto, apenas foca a aba. Aberturas pela
    /// árvore/palette marcam a aba como pré-visualização: a próxima abertura
    /// reutiliza a aba de preview limpa (sem edição), como no PhpStorm.
    pub fn open_file(&mut self, path: &Path) {
        self.open_file_ex(path, true);
    }

    /// [`open_file`] com controle de preview (aberturas internas de navegação,
    /// ex.: F2/F12, também viram preview; edições promovem a aba a permanente).
    pub fn open_file_ex(&mut self, path: &Path, preview: bool) {
        if let Some(pos) = self
            .buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(path))
        {
            self.active = pos;
            self.touch_recent(path);
            return;
        }
        match Buffer::open(path) {
            Ok(mut buf) => {
                buf.preview = preview;
                // Reutiliza a aba de preview limpa e não fixada.
                if preview {
                    if let Some(pos) = self
                        .buffers
                        .iter()
                        .position(|b| b.preview && !b.pinned && !b.dirty && b.path.is_some())
                    {
                        self.buffers[pos] = buf;
                        self.active = pos;
                        self.touch_recent(path);
                        return;
                    }
                }
                let at = if self.open_tabs_at_end || self.buffers.is_empty() {
                    self.buffers.len()
                } else {
                    (self.active + 1).min(self.buffers.len())
                };
                self.buffers.insert(at, buf);
                self.active = at;
                self.enforce_tab_limit();
                self.touch_recent(path);
                self.message.clear();
            }
            Err(e) => self.message = format!("could not open {}: {e}", path.display()),
        }
    }

    /// Closing Policy: excedido o limite configurável, fecha a aba mais
    /// antiga não fixada e sem edição (suja só fecha se for a única
    /// candidata — evita perda).
    fn enforce_tab_limit(&mut self) {
        while self.buffers.len() > self.tab_limit {
            // The tab opened longest ago (lowest id), never the active one.
            let active = self.active;
            let oldest = |dirty_ok: bool| {
                self.buffers
                    .iter()
                    .enumerate()
                    .filter(|(i, b)| *i != active && !b.pinned && (dirty_ok || !b.dirty))
                    .min_by_key(|(_, b)| b.id)
                    .map(|(i, _)| i)
            };
            let victim = oldest(false).or_else(|| oldest(true));
            match victim {
                Some(i) => {
                    let was_active = i == self.active;
                    self.buffers.remove(i);
                    if was_active {
                        self.active = i
                            .saturating_sub(1)
                            .min(self.buffers.len().saturating_sub(1));
                    } else if i < self.active {
                        self.active -= 1;
                    }
                }
                None => break,
            }
        }
    }

    /// Records an edit at `line0:col0` of `path`. Edits on the same line
    /// as the previous one replace it, so the list keeps distinct places.
    pub fn record_edit(&mut self, path: &Path, line0: usize, col0: usize) {
        self.edited_files.retain(|p| p != path);
        self.edited_files.insert(0, path.to_path_buf());
        self.edited_files.truncate(50);
        if let Some(last) = self.edit_locations.last_mut() {
            if last.0 == path && last.1 == line0 {
                last.2 = col0;
                return;
            }
        }
        self.edit_locations.push((path.to_path_buf(), line0, col0));
        if self.edit_locations.len() > 50 {
            self.edit_locations.remove(0);
        }
    }

    fn touch_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(50);
    }

    /// Closes tab `i` and adjusts the active index. Returns true if tabs remain.
    /// Closing the last tab leaves the editor empty (no automatic untitled tab).
    pub fn close_tab(&mut self, i: usize) -> bool {
        if i >= self.buffers.len() {
            return !self.buffers.is_empty();
        }
        let was_active = i == self.active;
        let closed = self.buffers.remove(i);
        self.remember_closed(closed.path);
        if was_active {
            self.active = i
                .saturating_sub(1)
                .min(self.buffers.len().saturating_sub(1));
        } else if i < self.active {
            self.active -= 1;
        }
        !self.buffers.is_empty()
    }

    fn remember_closed(&mut self, path: Option<PathBuf>) {
        if let Some(path) = path {
            self.closed_tabs.retain(|p| *p != path);
            self.closed_tabs.push(path);
            if self.closed_tabs.len() > 30 {
                self.closed_tabs.remove(0);
            }
        }
    }

    /// Reopens the most recently closed file that is not open (RF-TAB-15).
    pub fn reopen_closed_tab(&mut self) -> bool {
        while let Some(path) = self.closed_tabs.pop() {
            let open = self
                .buffers
                .iter()
                .any(|b| b.path.as_deref() == Some(&path));
            if !open && path.is_file() {
                self.open_file_ex(&path, false);
                return true;
            }
        }
        false
    }

    /// Closes every tab except pinned ones (`keep_pinned`).
    pub fn close_all_tabs(&mut self, keep_pinned: bool) {
        let mut i = 0;
        while i < self.buffers.len() {
            if keep_pinned && self.buffers[i].pinned {
                i += 1;
            } else {
                self.close_tab(i);
            }
        }
        self.active = self.active.min(self.buffers.len().saturating_sub(1));
    }

    /// Fecha todas exceto a aba `i` (Close Others do menu de contexto);
    /// abas fixadas são preservadas.
    pub fn close_other_tabs(&mut self, keep: usize) {
        if keep >= self.buffers.len() {
            return;
        }
        let keep_path = self.buffers[keep].path.clone();
        let mut i = 0;
        let mut closed = Vec::new();
        self.buffers.retain(|b| {
            let keep_me = i == keep || b.pinned;
            if !keep_me {
                closed.push(b.path.clone());
            }
            i += 1;
            keep_me
        });
        for path in closed {
            self.remember_closed(path);
        }
        self.active = keep_path
            .and_then(|p| {
                self.buffers
                    .iter()
                    .position(|b| b.path.as_deref() == Some(p.as_path()))
            })
            .unwrap_or(0);
    }

    pub fn close_active(&mut self) -> bool {
        let active = self.active;
        self.close_tab(active)
    }

    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_skips_node_modules() {
        // FE-001-04: node_modules é biblioteca (indexada pelo tsserver), não
        // parte da árvore do projeto — sem varredura redundante.
        assert!(SKIP_DIRS.contains(&"node_modules"));
    }

    use super::*;

    #[test]
    fn document_insert_delete_line_col() {
        let mut doc = Document::new("abc\ndef\n");
        assert_eq!(doc.line_count(), 3);
        doc.insert(0, "X");
        assert_eq!(doc.text(), "Xabc\ndef\n");
        doc.insert(doc.len_chars(), "!");
        assert_eq!(doc.line_col(0), (1, 1));
        // 'd' está no offset 5 após o insert no início
        assert_eq!(doc.line_col(5), (2, 1));
        doc.delete(0, 1);
        assert_eq!(doc.text(), "abc\ndef\n!");
        assert_eq!(doc.line(1), "def\n");
    }

    #[test]
    fn buffer_undo_redo() {
        let mut buf = Buffer::untitled();
        let base = buf.document.len_chars();
        buf.insert(base, "echo 1;");
        assert!(buf.dirty);
        assert!(buf.undo().is_some());
        assert_eq!(buf.document.text(), "<?php\n\n");
        assert!(buf.redo().is_some());
        assert!(buf.document.text().ends_with("echo 1;"));
    }

    #[test]
    fn revision_only_grows_and_the_log_replays_changes() {
        let mut buf = Buffer::untitled();
        let r0 = buf.document.revision();
        buf.insert(0, "a");
        buf.insert(1, "b");
        assert_eq!(buf.document.revision(), r0 + 2);
        buf.undo();
        buf.undo();
        assert_eq!(buf.document.text(), "<?php\n\n");
        assert_eq!(buf.document.revision(), r0 + 4, "undo is a new revision");
        let log = buf.document.changes_since(r0).expect("log reaches back");
        assert_eq!(log.len(), 4);
        assert_eq!(buf.document.changes_since(r0 + 4), Some(Vec::new()));
        assert!(buf.document.changes_since(r0 + 99).is_none());
    }

    #[test]
    fn undo_back_to_the_saved_text_is_clean() {
        let dir = std::env::temp_dir().join(format!("phpide-dirty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.php");
        fs::write(&path, "<?php\n").unwrap();
        let mut buf = Buffer::open(&path).unwrap();
        buf.insert(0, "x");
        assert!(buf.dirty);
        buf.undo();
        assert!(!buf.dirty, "back to the file content");
        buf.redo();
        buf.save().unwrap();
        assert!(!buf.dirty);
        buf.undo();
        assert!(buf.dirty, "differs from the saved file");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn typing_steps_merge_into_one_undo() {
        let mut buf = Buffer::untitled();
        let end = buf.document.len_chars();
        for (i, ch) in "abc".chars().enumerate() {
            let at = end + i;
            buf.edit(
                ChangeSet::single(at, at, ch.to_string()),
                EditKind::Typing,
                vec![(at, at)],
                vec![(at + 1, at + 1)],
            );
        }
        assert!(buf.document.text().ends_with("abc"));
        assert_eq!(buf.undo(), Some(vec![(end, end)]));
        assert_eq!(buf.document.text(), "<?php\n\n");
    }

    #[test]
    fn replace_all_changes_only_the_difference() {
        let mut buf = Buffer::untitled();
        buf.replace_all("<?php\necho 1;\n");
        let log = buf.document.changes_since(0).unwrap();
        assert_eq!(
            log[0].changes.changes(),
            &[Change::new(6, 6, "echo 1;")][..]
        );
        assert_eq!(buf.document.text(), "<?php\necho 1;\n");
    }

    #[test]
    fn untitled_starts_clean_and_becomes_dirty_only_after_editing() {
        let mut buffer = Buffer::untitled();
        assert!(!buffer.dirty);
        buffer.insert(0, "// mudança\n");
        assert!(buffer.dirty);
    }

    #[test]
    fn buffer_save_marks_clean() {
        let dir = std::env::temp_dir().join(format!("phpide-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.php");
        let mut buf = Buffer::untitled();
        buf.path = Some(file.clone());
        buf.save().unwrap();
        assert!(!buf.dirty);
        assert_eq!(fs::read_to_string(&file).unwrap(), "<?php\n\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_skips_vendor_and_hidden() {
        let dir = std::env::temp_dir().join(format!("phpide-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("vendor/pkg")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join("src/index.php"), "<?php").unwrap();
        fs::write(dir.join("composer.json"), "{}").unwrap();
        fs::write(dir.join("vendor/pkg/x.php"), "<?php").unwrap();
        let files = scan(&dir);
        let names: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        assert!(names.contains(&dir.join("src/index.php")));
        assert!(names.contains(&dir.join("composer.json")));
        assert!(!names.iter().any(|p| p.to_string_lossy().contains("vendor")));
        assert!(!names.iter().any(|p| p.to_string_lossy().contains(".git")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_tab_is_reused_until_edited() {
        let dir = std::env::temp_dir().join(format!("phpide-tabs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.php"), "<?php // a").unwrap();
        fs::write(dir.join("b.php"), "<?php // b").unwrap();
        fs::write(dir.join("c.php"), "<?php // c").unwrap();

        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.open_file_ex(&dir.join("a.php"), true);
        assert!(ws.buffers[0].preview, "a.php abre como preview");
        // Abrir b.php deve REUTILIZAR a aba de preview limpa.
        ws.open_file_ex(&dir.join("b.php"), true);
        assert_eq!(ws.buffers.len(), 1, "preview limpa é reutilizada");
        assert_eq!(
            ws.buffers[0].path.as_deref(),
            Some(dir.join("b.php").as_path())
        );
        // Editar promove a permanente: abrir c.php cria segunda aba.
        let base = ws.buffers[0].document.len_chars();
        ws.buffers[0].insert(base, " ");
        assert!(!ws.buffers[0].preview, "edição promove a aba");
        ws.open_file_ex(&dir.join("c.php"), true);
        assert_eq!(ws.buffers.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn closing_policy_keeps_at_most_10_unpinned() {
        let dir = std::env::temp_dir().join(format!("phpide-limit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..12 {
            fs::write(dir.join(format!("f{i:02}.php")), "<?php").unwrap();
        }
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        for i in 0..12 {
            ws.open_file_ex(&dir.join(format!("f{i:02}.php")), false);
        }
        assert!(ws.buffers.len() <= TAB_LIMIT, "limite de abas respeitado");
        // Fixar a primeira: ela nunca pode ser fechada pela política.
        let first = ws.buffers[0].path.clone().unwrap();
        ws.buffers[0].pinned = true;
        for i in 12..20 {
            fs::write(dir.join(format!("f{i:02}.php")), "<?php").unwrap();
            ws.open_file_ex(&dir.join(format!("f{i:02}.php")), false);
        }
        assert!(
            ws.buffers.iter().any(|b| b.path == Some(first.clone())),
            "aba fixada sobrevive à política"
        );
        assert!(ws.buffers.len() <= TAB_LIMIT);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tabs_open_next_to_the_active_one_and_the_oldest_closes_first() {
        let root = std::env::temp_dir().join(format!("phpide-tabs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for name in ["a", "b", "c", "d"] {
            fs::write(root.join(format!("{name}.php")), "<?php").unwrap();
        }
        let mut ws = Workspace::unscanned(root.clone());
        let open = |ws: &mut Workspace, name: &str| {
            ws.open_file_ex(&root.join(format!("{name}.php")), false)
        };
        let names =
            |ws: &Workspace| -> Vec<String> { ws.buffers.iter().map(|b| b.name()).collect() };
        open(&mut ws, "a");
        open(&mut ws, "b");
        ws.active = 0;
        open(&mut ws, "c");
        assert_eq!(
            names(&ws),
            ["a.php", "c.php", "b.php"],
            "c opens right after a"
        );
        assert_eq!(ws.active, 1);

        ws.tab_limit = 3;
        open(&mut ws, "d");
        assert_eq!(
            names(&ws),
            ["c.php", "d.php", "b.php"],
            "a was opened first, so it closes first"
        );

        ws.open_tabs_at_end = true;
        ws.close_tab(0);
        assert_eq!(ws.closed_tabs.last(), Some(&root.join("c.php")));
        assert!(ws.reopen_closed_tab());
        assert_eq!(
            names(&ws).last().map(String::as_str),
            Some("c.php"),
            "reopened at the end"
        );
        ws.buffers[0].pinned = true;
        ws.close_all_tabs(true);
        assert_eq!(names(&ws), ["d.php"], "the pinned tab stays");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn edit_locations_keep_one_entry_per_line() {
        let mut ws = Workspace::unscanned(PathBuf::from("/p"));
        let a = Path::new("/p/a.php");
        ws.record_edit(a, 3, 1);
        ws.record_edit(a, 3, 9);
        ws.record_edit(a, 10, 0);
        assert_eq!(
            ws.edit_locations,
            vec![(a.to_path_buf(), 3, 9), (a.to_path_buf(), 10, 0)]
        );
        assert_eq!(ws.edited_files, vec![a.to_path_buf()]);
    }

    #[test]
    fn closing_policy_uses_configured_tab_limit() {
        // UI-ED-003: o limite é configurável via Workspace::tab_limit.
        let dir = std::env::temp_dir().join(format!("phpide-limit2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            fs::write(dir.join(format!("f{i:02}.php")), "<?php").unwrap();
        }
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.tab_limit = 3;
        for i in 0..8 {
            ws.open_file_ex(&dir.join(format!("f{i:02}.php")), false);
        }
        assert!(
            ws.buffers.len() <= 3,
            "limite configurado respeitado: {}",
            ws.buffers.len()
        );
        ws.tab_limit = 6;
        ws.open_file_ex(&dir.join("f00.php"), false);
        for i in 1..8 {
            ws.open_file_ex(&dir.join(format!("f{i:02}.php")), false);
        }
        assert!(ws.buffers.len() <= 6, "novo limite aplicado");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_tab_and_close_others() {
        let dir = std::env::temp_dir().join(format!("phpide-close-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for n in ["x.php", "y.php", "z.php"] {
            fs::write(dir.join(n), "<?php").unwrap();
        }
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.open_file_ex(&dir.join("x.php"), false);
        ws.open_file_ex(&dir.join("y.php"), false);
        ws.open_file_ex(&dir.join("z.php"), false);
        assert_eq!(ws.active, 2);
        ws.close_tab(2);
        assert_eq!(ws.buffers.len(), 2);
        assert_eq!(ws.active, 1, "índice ativo ajustado após fechar a ativa");
        ws.close_other_tabs(0);
        assert_eq!(ws.buffers.len(), 1);
        assert_eq!(
            ws.buffers[0].path.as_deref(),
            Some(dir.join("x.php").as_path())
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_a_project_does_not_create_an_untitled_tab() {
        let dir = std::env::temp_dir().join(format!("phpide-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.php"), "<?php").unwrap();
        let mut ws = Workspace::open(dir.clone());
        assert!(ws.buffers.is_empty());
        assert!(ws.active_buffer().is_none());
        assert!(!ws.files.is_empty(), "open() still scans the tree");
        ws.open_file_ex(&dir.join("a.php"), false);
        assert!(!ws.close_tab(0), "closing the last tab leaves no tabs");
        assert!(ws.buffers.is_empty());
        assert!(ws.active_buffer().is_none());
        assert!(Workspace::unscanned(dir.clone()).files.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reload_from_disk_only_replaces_changed_content() {
        let dir = std::env::temp_dir().join(format!("phpide-reload-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.php");
        fs::write(&file, "<?php echo 1;").unwrap();
        let mut buf = Buffer::open(&file).unwrap();
        let revision = buf.document.revision();
        assert!(
            !buf.reload_from_disk().unwrap(),
            "same content is not a reload"
        );
        assert_eq!(buf.document.revision(), revision);
        fs::write(&file, "<?php echo 2;").unwrap();
        assert!(buf.reload_from_disk().unwrap());
        assert_eq!(buf.document.text(), "<?php echo 2;");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nav_history_back_and_forward() {
        let dir = std::env::temp_dir().join(format!("phpide-nav-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.php");
        let b = dir.join("b.php");
        fs::write(&a, "<?php").unwrap();
        fs::write(&b, "<?php").unwrap();
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.open_file_ex(&a, false);
        ws.open_file_at(&b, 0, 0);
        assert_eq!(
            ws.active_buffer().and_then(|b| b.path.clone()),
            Some(b.clone())
        );
        assert!(ws.navigate_back());
        assert_eq!(
            ws.active_buffer().and_then(|b| b.path.clone()),
            Some(a.clone())
        );
        assert!(ws.navigate_forward());
        assert_eq!(
            ws.active_buffer().and_then(|b| b.path.clone()),
            Some(b.clone())
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recent_files_mru_order() {
        let dir = std::env::temp_dir().join(format!("phpide-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.php");
        let b = dir.join("b.php");
        fs::write(&a, "<?php").unwrap();
        fs::write(&b, "<?php").unwrap();
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.open_file_ex(&a, false);
        ws.open_file_ex(&b, false);
        assert_eq!(ws.recent_files.first(), Some(&b));
        ws.open_file_ex(&a, false); // reabrir a move para o topo
        assert_eq!(ws.recent_files.first(), Some(&a));
        assert_eq!(ws.recent_files.len(), 2, "sem duplicatas");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn diagnostics_are_invalidated_by_document_revision() {
        let dir = std::env::temp_dir().join(format!("phpide-diagrev-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.php");
        fs::write(&a, "<?php echo 1;").unwrap();
        let mut ws = Workspace::open(dir.clone());
        ws.buffers.clear();
        ws.open_file_ex(&a, false);
        let mk = |msg: &str| Diagnostic {
            path: a.clone(),
            line: 1,
            col: 1,
            message: msg.to_string(),
            severity: Severity::Error,
            source: "test".to_string(),
        };
        let rev = ws.buffers[0].document.revision();
        ws.set_diagnostics(&a, rev, vec![mk("erro-v1")]);
        assert_eq!(ws.diagnostics_for(&a).count(), 1);
        assert!(ws.diagnostics_current(&a));
        // Editar o documento invalida o conjunto antigo (NFR-002).
        let base = ws.buffers[0].document.len_chars();
        ws.buffers[0].insert(base, " ");
        assert!(!ws.diagnostics_current(&a), "edição invalida diagnósticos");
        assert_eq!(ws.diagnostics_for(&a).count(), 0, "não exibir dados velhos");
        assert_eq!(ws.current_diagnostics().count(), 0);
        // Reanexo na nova revisão volta a exibir.
        let rev2 = ws.buffers[0].document.revision();
        ws.set_diagnostics(&a, rev2, vec![mk("erro-v2")]);
        assert_eq!(ws.current_diagnostics().count(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_failure_keeps_buffer_dirty_and_original_intact() {
        let dir = std::env::temp_dir().join(format!("phpide-savefail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.php");
        let mut buf = Buffer::untitled();
        buf.path = Some(file.clone());
        buf.save().unwrap();
        assert!(!buf.dirty);
        // Destino vira diretório: a escrita atômica falha sem partial write.
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        buf.insert(0, "X");
        assert!(buf.dirty);
        let err = buf.try_save().unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::Io);
        assert!(buf.dirty, "buffer preservado sujo para nova tentativa");
        assert!(file.is_dir(), "destino original intacto");
        let _ = err;
        fs::remove_dir_all(&dir).ok();
    }
}

// ---------------------------------------------------------------------------
// Estágio 2: diagnósticos, navegação com salto e recarga externa
// ---------------------------------------------------------------------------

/// Gravidade de um diagnóstico (mesma escala do LSP: 1=Erro .. 4=Dica).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "erro",
            Severity::Warning => "aviso",
            Severity::Information => "info",
            Severity::Hint => "dica",
        }
    }

    /// Marca curta para o painel (sem depender de ícones).
    pub fn marker(self) -> &'static str {
        match self {
            Severity::Error => "E",
            Severity::Warning => "A",
            Severity::Information => "I",
            Severity::Hint => "D",
        }
    }
}

/// Um problema detectado em um arquivo (LSP no Estágio 3; sintaxe no Estágio 4).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub path: PathBuf,
    /// 1-based.
    pub line: usize,
    /// 1-based.
    pub col: usize,
    pub message: String,
    pub severity: Severity,
    pub source: String,
}

/// Item de outline (estrutura do arquivo). Preenchido via LSP documentSymbol.
#[derive(Debug, Clone)]
pub struct OutlineEntry {
    pub name: String,
    /// Rótulo do tipo: "classe", "função", ...
    pub kind: String,
    /// Linha 0-based para salto.
    pub line0: usize,
    pub depth: usize,
}

impl Buffer {
    /// Salta para uma posição assim que o buffer estiver visível no editor.
    pub fn request_cursor(&mut self, line0: usize, col0: usize) {
        self.pending_cursor = Some((line0, col0));
    }

    pub fn take_pending_cursor(&mut self) -> Option<(usize, usize)> {
        self.pending_cursor.take()
    }

    /// Recarrega do disco se não houver edições locais. Retorna true se recarregou.
    pub fn reload_from_disk(&mut self) -> io::Result<bool> {
        if self.dirty {
            return Ok(false);
        }
        let Some(path) = self.path.clone() else {
            return Ok(false);
        };
        let on_disk = fs::read_to_string(&path)?;
        // Same content: keep the document (and its revision) untouched, so
        // diagnostics, markers and the cursor stay valid.
        if self.document.rope == on_disk.as_str() {
            return Ok(false);
        }
        // Applied as a change, so the revision keeps growing and editor views
        // map their carets; the undo history no longer matches the file.
        let changes = self.document.diff_to(&on_disk);
        self.document.apply(&changes);
        self.history.clear();
        self.saved_state = self.history.state_id();
        self.dirty = false;
        Ok(true)
    }
}

impl Workspace {
    /// Abre (ou foca) o arquivo e posiciona o cursor em (line0, col0) 0-based.
    /// Registra a posição anterior no histórico de navegação (Alt+←/→).
    pub fn open_file_at(&mut self, path: &Path, line0: usize, col0: usize) {
        // Não empilha se já estamos navegando para onde já estamos.
        let same_active = self
            .active_buffer()
            .and_then(|b| b.path.clone())
            .map(|p| p == path)
            .unwrap_or(false);
        if !same_active {
            if let Some(cur) = self.active_buffer() {
                if let Some(cur_path) = cur.path.clone() {
                    self.nav_back.push((cur_path, 0, 0));
                    if self.nav_back.len() > 128 {
                        self.nav_back.remove(0);
                    }
                }
            }
            self.nav_forward.clear();
        }
        self.open_file(path);
        if let Some(buf) = self.active_buffer_mut() {
            buf.request_cursor(line0, col0);
        }
    }

    /// Alt+←: volta para a posição anterior da pilha.
    pub fn navigate_back(&mut self) -> bool {
        let Some((path, line0, col0)) = self.nav_back.pop() else {
            return false;
        };
        if let Some(cur) = self.active_buffer() {
            if let Some(cur_path) = cur.path.clone() {
                self.nav_forward.push((cur_path, 0, 0));
            }
        }
        self.open_file(&path);
        if let Some(buf) = self.active_buffer_mut() {
            buf.request_cursor(line0, col0);
        }
        true
    }

    /// Alt+→: refaz a navegação desfeita por Alt+←.
    pub fn navigate_forward(&mut self) -> bool {
        let Some((path, line0, col0)) = self.nav_forward.pop() else {
            return false;
        };
        if let Some(cur) = self.active_buffer() {
            if let Some(cur_path) = cur.path.clone() {
                self.nav_back.push((cur_path, 0, 0));
            }
        }
        self.open_file(&path);
        if let Some(buf) = self.active_buffer_mut() {
            buf.request_cursor(line0, col0);
        }
        true
    }

    /// Substitui os diagnósticos de um arquivo, anexando-os à revisão do
    /// documento em que foram calculados (NFR-002). Diagnósticos cuja
    /// revisão divergiu do buffer aberto são desconsiderados: um resultado
    /// velho nunca descreve o texto atual.
    pub fn set_diagnostics(&mut self, path: &Path, revision: Revision, mut diags: Vec<Diagnostic>) {
        self.diagnostics.retain(|d| d.path != path);
        diags.sort_by_key(|d| (d.line, d.col));
        self.diagnostics.extend(diags);
        self.diagnostics
            .sort_by_key(|d| (d.path.clone(), d.line, d.col));
        self.diag_revisions.insert(path.to_path_buf(), revision);
    }

    pub fn clear_diagnostics(&mut self) {
        self.diagnostics.clear();
        self.diag_revisions.clear();
    }

    /// `true` se os diagnósticos de `path` ainda descrevem o buffer aberto.
    pub fn diagnostics_current(&self, path: &Path) -> bool {
        let Some(stored) = self.diag_revisions.get(path) else {
            return false;
        };
        self.buffers
            .iter()
            .find(|b| b.path.as_deref() == Some(path))
            .map(|b| b.document.revision() == *stored)
            .unwrap_or(false)
    }

    /// Problemas de `path` que ainda refletem a revisão do documento.
    pub fn diagnostics_for<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        let current = self.diagnostics_current(path);
        self.diagnostics
            .iter()
            .filter(move |d| d.path == path && current)
    }

    /// Problemas vigentes de todos os arquivos (painel Problems).
    pub fn current_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| self.diagnostics_current(&d.path))
    }

    /// Recarrega buffers abertos afetados por mudança externa (git, build, ...).
    /// Retorna os caminhos recarregados.
    pub fn reload_clean_buffers(&mut self, changed: &[PathBuf]) -> Vec<PathBuf> {
        let mut reloaded = Vec::new();
        for buf in &mut self.buffers {
            let Some(path) = buf.path.clone() else {
                continue;
            };
            if changed.iter().any(|c| c == &path) {
                match buf.reload_from_disk() {
                    Ok(true) => reloaded.push(path),
                    Ok(false) => {} // sujo: mantém edição local
                    Err(e) => self.message = format!("could not reload {}: {e}", path.display()),
                }
            }
        }
        reloaded
    }
}
