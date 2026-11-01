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

/// Um documento de texto editável. Equivalente ao `Document` da IntelliJ Platform.
#[derive(Debug)]
pub struct Document {
    rope: Rope,
    /// Incrementa a cada mutação; útil para invalidar caches futuros (highlight, LSP).
    pub version: u64,
}

impl Document {
    pub fn new(text: &str) -> Self {
        Document {
            rope: Rope::from_str(text),
            version: 0,
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self::new(&text))
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

    /// Insere `text` no deslocamento em caracteres `at`.
    pub fn insert(&mut self, at: usize, text: &str) {
        self.rope.insert(at, text);
        self.version += 1;
    }

    /// Remove o intervalo em caracteres `[start, end)`.
    pub fn delete(&mut self, start: usize, end: usize) {
        self.rope.remove(start..end);
        self.version += 1;
    }

    /// Texto da linha `line0` (0-based), incluindo o `\n` final se houver.
    pub fn line(&self, line0: usize) -> String {
        self.rope.line(line0).to_string()
    }

    pub fn char(&self, offset: usize) -> char {
        self.rope.char(offset)
    }
}

const UNDO_CAP: usize = 128;

/// Um arquivo aberto na IDE: documento + metadados + histórico.
pub struct Buffer {
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
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// Salto de cursor pendente (line0, col0), aplicado quando o editor desenhar.
    pending_cursor: Option<(usize, usize)>,
}

impl Buffer {
    pub fn untitled() -> Self {
        Buffer {
            document: Document::new("<?php\n\n"),
            path: None,
            dirty: true,
            preview: false,
            pinned: false,
            readonly: false,
            undo: Vec::new(),
            redo: Vec::new(),
            pending_cursor: None,
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Buffer {
            document: Document::open(path)?,
            path: Some(path.to_path_buf()),
            dirty: false,
            preview: false,
            pinned: false,
            readonly: false,
            undo: Vec::new(),
            redo: Vec::new(),
            pending_cursor: None,
        })
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sem-titulo.php".to_string())
    }

    fn record(&mut self) {
        self.undo.push(Document {
            rope: self.document.rope.clone(),
            version: self.document.version,
        });
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn insert(&mut self, at: usize, text: &str) {
        self.record();
        self.document.insert(at, text);
        self.dirty = true;
        // Editar promove a aba de pré-visualização a permanente.
        self.preview = false;
    }

    pub fn delete(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.record();
        self.document.delete(start, end);
        self.dirty = true;
        self.preview = false;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(Document {
                    rope: self.document.rope.clone(),
                    version: self.document.version,
                });
                self.document = prev;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(Document {
                    rope: self.document.rope.clone(),
                    version: self.document.version,
                });
                self.document = next;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "buffer sem caminho"))?;
        fs::write(&path, self.document.text())?;
        self.dirty = false;
        Ok(())
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

fn scan_into(root: &Path, dir: &Path, depth: usize, out: &mut Vec<FileEntry>) {
    if depth > 6 {
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
        let is_dir = path.is_dir();
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
    /// Diagnósticos agregados de todos os arquivos (LSP).
    pub diagnostics: Vec<Diagnostic>,
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
}

/// Máximo de abas antes da Closing Policy fechar a mais antiga não fixada
/// (mesmo default do PhpStorm: Editor Tabs | Closing Policy | 10).
pub const TAB_LIMIT: usize = 10;

impl Workspace {
    pub fn open(root: PathBuf) -> Self {
        let files = scan(&root);
        let mut ws = Workspace {
            root,
            buffers: Vec::new(),
            active: 0,
            files,
            diagnostics: Vec::new(),
            outline: Vec::new(),
            problems_visible: false,
            message: String::new(),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            recent_files: Vec::new(),
        };
        ws.new_file();
        ws
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
        if let Some(pos) = self.buffers.iter().position(|b| b.path.as_deref() == Some(path)) {
            self.active = pos;
            self.touch_recent(path);
            return;
        }
        match Buffer::open(path) {
            Ok(mut buf) => {
                buf.preview = preview;
                // Reutiliza a aba de preview limpa e não fixada.
                if preview {
                    if let Some(pos) = self.buffers.iter().position(|b| {
                        b.preview && !b.pinned && !b.dirty && b.path.is_some()
                    }) {
                        self.buffers[pos] = buf;
                        self.active = pos;
                        self.touch_recent(path);
                        return;
                    }
                }
                self.buffers.push(buf);
                self.active = self.buffers.len() - 1;
                self.enforce_tab_limit();
                self.touch_recent(path);
                self.message.clear();
            }
            Err(e) => self.message = format!("falha ao abrir {}: {e}", path.display()),
        }
    }

    /// Closing Policy: excedido o limite, fecha a aba mais antiga não fixada
    /// e sem edição (suja só fecha se for a única candidata — evita perda).
    fn enforce_tab_limit(&mut self) {
        while self.buffers.len() > TAB_LIMIT {
            let victim = self
                .buffers
                .iter()
                .position(|b| !b.pinned && !b.dirty)
                .or_else(|| self.buffers.iter().position(|b| !b.pinned));
            match victim {
                Some(i) => {
                    let was_active = i == self.active;
                    self.buffers.remove(i);
                    if was_active {
                        self.active = i.saturating_sub(1).min(self.buffers.len().saturating_sub(1));
                    } else if i < self.active {
                        self.active -= 1;
                    }
                }
                None => break,
            }
        }
    }

    fn touch_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(50);
    }

    /// Fecha a aba `i` ajustando o índice ativo. Retorna true se sobraram abas.
    pub fn close_tab(&mut self, i: usize) -> bool {
        if i >= self.buffers.len() {
            return !self.buffers.is_empty();
        }
        let was_active = i == self.active;
        self.buffers.remove(i);
        if was_active {
            self.active = i.saturating_sub(1).min(self.buffers.len().saturating_sub(1));
        } else if i < self.active {
            self.active -= 1;
        }
        if self.buffers.is_empty() {
            self.new_file();
            self.active = 0;
        }
        !self.buffers.is_empty()
    }

    /// Fecha todas exceto a aba `i` (Close Others do menu de contexto);
    /// abas fixadas são preservadas.
    pub fn close_other_tabs(&mut self, keep: usize) {
        if keep >= self.buffers.len() {
            return;
        }
        let keep_path = self.buffers[keep].path.clone();
        let mut i = 0;
        self.buffers.retain(|b| {
            let keep_me = i == keep || b.pinned;
            i += 1;
            keep_me
        });
        self.active = keep_path
            .and_then(|p| self.buffers.iter().position(|b| b.path.as_deref() == Some(p.as_path())))
            .unwrap_or(0);
        if self.buffers.is_empty() {
            self.new_file();
        }
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
        assert!(buf.undo());
        assert_eq!(buf.document.text(), "<?php\n\n");
        assert!(buf.redo());
        assert!(buf.document.text().ends_with("echo 1;"));
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
        assert_eq!(ws.buffers[0].path.as_deref(), Some(dir.join("b.php").as_path()));
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
        assert_eq!(ws.buffers[0].path.as_deref(), Some(dir.join("x.php").as_path()));
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
        assert_eq!(ws.active_buffer().and_then(|b| b.path.clone()), Some(b.clone()));
        assert!(ws.navigate_back());
        assert_eq!(ws.active_buffer().and_then(|b| b.path.clone()), Some(a.clone()));
        assert!(ws.navigate_forward());
        assert_eq!(ws.active_buffer().and_then(|b| b.path.clone()), Some(b.clone()));
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
        self.document = Document::open(&path)?;
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
        let Some((path, line0, col0)) = self.nav_back.pop() else { return false };
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
        let Some((path, line0, col0)) = self.nav_forward.pop() else { return false };
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

    /// Substitui os diagnósticos de um arquivo pelos novos (ordena por posição).
    pub fn set_diagnostics(&mut self, path: &Path, mut diags: Vec<Diagnostic>) {
        self.diagnostics.retain(|d| d.path != path);
        diags.sort_by_key(|d| (d.line, d.col));
        self.diagnostics.extend(diags);
        self.diagnostics
            .sort_by_key(|d| (d.path.clone(), d.line, d.col));
    }

    pub fn clear_diagnostics(&mut self) {
        self.diagnostics.clear();
    }

    /// Problemas do arquivo ativo (para outline/problemas filtrados se necessário).
    pub fn diagnostics_for<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        self.diagnostics.iter().filter(move |d| d.path == path)
    }

    /// Recarrega buffers abertos afetados por mudança externa (git, build, ...).
    /// Retorna os caminhos recarregados.
    pub fn reload_clean_buffers(&mut self, changed: &[PathBuf]) -> Vec<PathBuf> {
        let mut reloaded = Vec::new();
        for buf in &mut self.buffers {
            let Some(path) = buf.path.clone() else { continue };
            if changed.iter().any(|c| c == &path) {
                match buf.reload_from_disk() {
                    Ok(true) => reloaded.push(path),
                    Ok(false) => {} // sujo: mantém edição local
                    Err(e) => self.message = format!("falha ao recarregar {}: {e}", path.display()),
                }
            }
        }
        reloaded
    }
}
