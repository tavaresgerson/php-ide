//! Incremental syntax highlighting and code structure.
//!
//! PHP files keep a tree-sitter tree that is edited with the document's
//! change log and reparsed incrementally; highlighting runs the
//! tree-sitter highlight query over the visible lines only. Other languages
//! use syntect line by line with stored states (`phpide_php::LineHighlighter`).
//! Results are cached per line until the document revision changes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use phpide_core::{Document, Revision};
use phpide_php::filetype::{self, Detected, FileType, Highlighting, SimpleLanguage};
use phpide_php::{LineHighlighter, SyntaxRole};
use phpide_syntax::folding::FoldRange;
use phpide_syntax::SyntaxReport;
use ropey::Rope;
use tree_sitter::{
    InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, TextProvider, Tree,
};

/// A highlighted run of one line, in char columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub role: SyntaxRole,
}

/// Folds, blocks and syntax errors of the current revision.
#[derive(Debug, Clone, Default)]
pub struct Structure {
    pub folds: Vec<FoldRange>,
    pub report: SyntaxReport,
}

/// Extra patterns on top of the grammar's highlight query (later patterns
/// win, so these refine the generic ones).
const EXTRA_QUERY: &str = r#"
(class_declaration name: (name) @type)
(interface_declaration name: (name) @type)
(trait_declaration name: (name) @type)
(enum_declaration name: (name) @type)
(base_clause [(name) (qualified_name)] @type)
(class_interface_clause [(name) (qualified_name)] @type)
(scoped_call_expression scope: [(name) (qualified_name)] @type)
(class_constant_access_expression . [(name) (qualified_name)] @type)
(simple_parameter name: (variable_name) @variable.parameter)
(variadic_parameter name: (variable_name) @variable.parameter)
(property_promotion_parameter name: (variable_name) @variable.parameter)
(attribute [(name) (qualified_name)] @attribute)
((comment) @comment.documentation (#match? @comment.documentation "^/\\*\\*"))
"#;

struct Highlights {
    query: Query,
    roles: Vec<Option<SyntaxRole>>,
}

fn role_for_capture(name: &str) -> Option<SyntaxRole> {
    Some(match name {
        "comment" => SyntaxRole::Comment,
        "comment.documentation" => SyntaxRole::Documentation,
        "constant" => SyntaxRole::Constant,
        "constant.builtin" | "function.builtin" | "type.builtin" | "variable.builtin"
        | "keyword" | "tag" => SyntaxRole::Keyword,
        "constructor" | "type" => SyntaxRole::Type,
        "function" => SyntaxRole::Function,
        "function.method" => SyntaxRole::Method,
        "module" => SyntaxRole::Namespace,
        "number" => SyntaxRole::Number,
        "operator" => SyntaxRole::Operator,
        "property" => SyntaxRole::Property,
        "string" => SyntaxRole::String,
        "variable" => SyntaxRole::Variable,
        "variable.parameter" => SyntaxRole::Parameter,
        "attribute" => SyntaxRole::Attribute,
        _ => return None,
    })
}

fn php_language() -> tree_sitter::Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}

fn highlights() -> &'static Highlights {
    static HIGHLIGHTS: OnceLock<Highlights> = OnceLock::new();
    HIGHLIGHTS.get_or_init(|| {
        let language = php_language();
        let source = format!("{}\n{}", tree_sitter_php::HIGHLIGHTS_QUERY, EXTRA_QUERY);
        let (mut query, source) = match Query::new(&language, &source) {
            Ok(query) => (query, source),
            Err(_) => (
                Query::new(&language, tree_sitter_php::HIGHLIGHTS_QUERY)
                    .expect("the grammar's highlight query compiles"),
                tree_sitter_php::HIGHLIGHTS_QUERY.to_string(),
            ),
        };
        // `$` keeps the color of its variable (parameter, property…)
        // instead of the operator color the grammar gives it.
        for pattern in 0..query.pattern_count() {
            if source[query.start_byte_for_pattern(pattern)..].starts_with("\"$\" @operator") {
                query.disable_pattern(pattern);
            }
        }
        let roles = query
            .capture_names()
            .iter()
            .map(|name| role_for_capture(name))
            .collect();
        Highlights { query, roles }
    })
}

/// Feeds node text to query predicates straight from the rope.
struct RopeText<'a>(&'a Rope);

impl<'a> TextProvider<&'a [u8]> for RopeText<'a> {
    type I = std::iter::Map<ropey::iter::Chunks<'a>, fn(&'a str) -> &'a [u8]>;

    fn text(&mut self, node: Node) -> Self::I {
        self.0
            .byte_slice(node.byte_range())
            .chunks()
            .map(str::as_bytes as fn(&'a str) -> &'a [u8])
    }
}

fn parse(parser: &mut Parser, rope: &Rope, old: Option<&Tree>) -> Option<Tree> {
    let mut read = |byte: usize, _: Point| -> &[u8] {
        if byte >= rope.len_bytes() {
            return &[];
        }
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
        &chunk.as_bytes()[byte - chunk_start..]
    };
    parser.parse_with_options(&mut read, old, None)
}

struct TreeSitterEngine {
    parser: Parser,
    tree: Option<Tree>,
    /// Document revision the tree describes.
    tree_revision: Option<Revision>,
    /// Full parse running on another thread (large files).
    pending: Option<Receiver<(Revision, Option<Tree>)>>,
}

/// Files above this size are parsed in the background the first time, so
/// opening them never waits for the parser.
const BACKGROUND_PARSE_BYTES: usize = 128 * 1024;

fn parse_in_background(rope: Rope, revision: Revision) -> Receiver<(Revision, Option<Tree>)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut parser = Parser::new();
        let tree = parser
            .set_language(&php_language())
            .ok()
            .and_then(|_| parse(&mut parser, &rope, None));
        let _ = sender.send((revision, tree));
    });
    receiver
}

enum Engine {
    TreeSitter(Box<TreeSitterEngine>),
    Lines(Box<LineHighlighter>),
    Simple(&'static SimpleLanguage),
    Plain,
}

/// Highlighting and structure of one document.
pub struct Syntax {
    engine: Engine,
    file_type: Detected,
    /// Revision the line cache describes (with or without a tree).
    revision: Option<Revision>,
    had_tree: bool,
    cache: HashMap<usize, Arc<Vec<Span>>>,
    structure: Option<(Revision, Arc<Structure>)>,
    structure_job: Option<(Instant, Receiver<(Revision, Structure)>)>,
}

fn php_type() -> &'static FileType {
    filetype::by_id("php").expect("PHP file type")
}

impl Syntax {
    /// Compiles the highlight query on another thread, so the first PHP
    /// file opened does not pay for it (~30 ms).
    pub fn warm_up() {
        std::thread::spawn(|| {
            highlights();
        });
    }

    /// Chooses the engine by file name (`None`: an untitled PHP file).
    pub fn for_path(path: Option<&Path>) -> Self {
        match path.and_then(|p| p.file_name()) {
            None => Syntax::for_type(&Detected::Known(php_type())),
            Some(name) => Syntax::for_type(&filetype::detect(&name.to_string_lossy(), "")),
        }
    }

    /// The engine of a detected file type.
    pub fn for_type(detected: &Detected) -> Self {
        let engine = match detected {
            Detected::Known(file_type) => match file_type.highlighting {
                Highlighting::Php => {
                    let mut parser = Parser::new();
                    match parser.set_language(&php_language()) {
                        Ok(()) => Engine::TreeSitter(Box::new(TreeSitterEngine {
                            parser,
                            tree: None,
                            tree_revision: None,
                            pending: None,
                        })),
                        Err(_) => Engine::Plain,
                    }
                }
                Highlighting::Syntect(grammar) => LineHighlighter::for_syntax_name(grammar)
                    .map(|h| Engine::Lines(Box::new(h)))
                    .unwrap_or(Engine::Plain),
                Highlighting::Simple(language) => Engine::Simple(language),
                Highlighting::None => Engine::Plain,
            },
            Detected::Grammar(grammar) => LineHighlighter::for_syntax_name(grammar)
                .map(|h| Engine::Lines(Box::new(h)))
                .unwrap_or(Engine::Plain),
            Detected::Unknown => Engine::Plain,
        };
        Syntax {
            engine,
            file_type: detected.clone(),
            revision: None,
            had_tree: false,
            cache: HashMap::new(),
            structure: None,
            structure_job: None,
        }
    }

    /// The file type this syntax was built for.
    pub fn file_type(&self) -> &Detected {
        &self.file_type
    }

    /// Name of the file type (status bar).
    pub fn language_name(&self) -> &str {
        self.file_type.name()
    }

    /// The syntax tree (PHP only), as of the last [`Syntax::sync`].
    pub fn tree(&self) -> Option<&Tree> {
        match &self.engine {
            Engine::TreeSitter(engine) => engine.tree.as_ref(),
            _ => None,
        }
    }

    /// A first parse is still running: highlighting appears when it ends.
    pub fn is_pending(&self) -> bool {
        matches!(&self.engine, Engine::TreeSitter(e) if e.pending.is_some())
            || self.structure_job.is_some()
    }

    /// Brings the tree (or line states) up to the document revision,
    /// replaying the change log when it reaches back far enough.
    pub fn sync(&mut self, doc: &Document) {
        let revision = doc.revision();
        match &mut self.engine {
            Engine::TreeSitter(engine) => {
                if let Some(pending) = &engine.pending {
                    if let Ok((rev, tree)) = pending.try_recv() {
                        engine.tree = tree;
                        engine.tree_revision = Some(rev);
                        engine.pending = None;
                    }
                }
                if engine.pending.is_none() && engine.tree_revision != Some(revision) {
                    let changes = engine
                        .tree_revision
                        .zip(engine.tree.as_ref())
                        .and_then(|(rev, _)| doc.changes_since(rev));
                    match (engine.tree.as_mut(), changes) {
                        (Some(tree), Some(changes)) => {
                            for change in &changes {
                                for e in &change.byte_edits {
                                    tree.edit(&InputEdit {
                                        start_byte: e.start_byte,
                                        old_end_byte: e.old_end_byte,
                                        new_end_byte: e.new_end_byte,
                                        start_position: Point::new(
                                            e.start_point.row,
                                            e.start_point.column,
                                        ),
                                        old_end_position: Point::new(
                                            e.old_end_point.row,
                                            e.old_end_point.column,
                                        ),
                                        new_end_position: Point::new(
                                            e.new_end_point.row,
                                            e.new_end_point.column,
                                        ),
                                    });
                                }
                            }
                            let old = engine.tree.take();
                            engine.tree = parse(&mut engine.parser, doc.rope(), old.as_ref());
                            engine.tree_revision = Some(revision);
                        }
                        _ if doc.rope().len_bytes() > BACKGROUND_PARSE_BYTES => {
                            engine.pending =
                                Some(parse_in_background(doc.rope().clone(), revision));
                        }
                        _ => {
                            engine.tree = parse(&mut engine.parser, doc.rope(), None);
                            engine.tree_revision = Some(revision);
                        }
                    }
                }
                let has_tree = engine.tree.is_some() && engine.tree_revision == Some(revision);
                if self.revision != Some(revision) || has_tree != self.had_tree {
                    self.cache.clear();
                    self.had_tree = has_tree;
                }
            }
            Engine::Lines(highlighter) => {
                if self.revision != Some(revision) {
                    let first_row = self
                        .revision
                        .and_then(|rev| doc.changes_since(rev))
                        .and_then(|changes| {
                            changes
                                .iter()
                                .flat_map(|c| c.byte_edits.iter())
                                .map(|e| e.start_point.row)
                                .min()
                        })
                        .unwrap_or(0);
                    highlighter.invalidate_from(first_row);
                    self.cache.clear();
                }
            }
            Engine::Simple(_) => {
                if self.revision != Some(revision) {
                    self.cache.clear();
                }
            }
            Engine::Plain => {}
        }
        self.revision = Some(revision);
    }

    fn current_tree(&self, doc: &Document) -> Option<&Tree> {
        match &self.engine {
            Engine::TreeSitter(e) if e.tree_revision == Some(doc.revision()) => e.tree.as_ref(),
            _ => None,
        }
    }

    /// Spans of lines `first..=last` (clamped to the document).
    pub fn line_spans(&mut self, doc: &Document, first: usize, last: usize) -> Vec<Arc<Vec<Span>>> {
        self.sync(doc);
        let rope = doc.rope();
        let last = last.min(rope.len_lines().saturating_sub(1));
        if first > last {
            return Vec::new();
        }
        let missing: Vec<usize> = (first..=last)
            .filter(|l| !self.cache.contains_key(l))
            .collect();
        if let (Some(&lo), Some(&hi)) = (missing.first(), missing.last()) {
            let tree = self.current_tree(doc).cloned();
            let computed = match &mut self.engine {
                Engine::TreeSitter(_) => match &tree {
                    Some(tree) => tree_sitter_spans(tree, rope, lo, hi),
                    None => vec![Vec::new(); hi + 1 - lo],
                },
                Engine::Lines(highlighter) => highlighter
                    .highlight_lines(lo, hi, |line| rope.line(line).to_string())
                    .into_iter()
                    .map(|spans| {
                        spans
                            .into_iter()
                            .map(|(start, end, role)| Span { start, end, role })
                            .collect()
                    })
                    .collect(),
                Engine::Simple(language) => (lo..=hi)
                    .map(|line| {
                        language
                            .highlight_line(&rope.line(line).to_string())
                            .into_iter()
                            .map(|(start, end, role)| Span { start, end, role })
                            .collect()
                    })
                    .collect(),
                Engine::Plain => vec![Vec::new(); hi + 1 - lo],
            };
            for (offset, spans) in computed.into_iter().enumerate() {
                self.cache.insert(lo + offset, Arc::new(spans));
            }
        }
        (first..=last)
            .map(|line| self.cache.get(&line).cloned().unwrap_or_default())
            .collect()
    }

    /// Folds, blocks and syntax errors (PHP only). Computed on another
    /// thread: the latest result is returned, possibly from an older
    /// revision while a new one is being computed (`None` before the first).
    pub fn structure(&mut self, doc: &Document) -> Option<Arc<Structure>> {
        self.sync(doc);
        let revision = doc.revision();
        if let Some((_, job)) = &self.structure_job {
            if let Ok((rev, structure)) = job.try_recv() {
                self.structure = Some((rev, Arc::new(structure)));
                self.structure_job = None;
            }
        }
        let stale = self.structure.as_ref().map(|(rev, _)| *rev) != Some(revision);
        let idle = self.structure_job.is_none();
        let rested = self
            .structure_job
            .as_ref()
            .map(|(started, _)| started.elapsed() > Duration::from_millis(300))
            .unwrap_or(true);
        if stale && idle && rested {
            if let Some(tree) = self.current_tree(doc).cloned() {
                let rope = doc.rope().clone();
                let (sender, receiver) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let structure = Structure {
                        folds: phpide_syntax::folding::folding_ranges_from_tree(
                            &tree,
                            rope.len_lines(),
                        ),
                        report: phpide_syntax::analyze_tree(&tree, &|node| {
                            rope.byte_slice(node.byte_range()).to_string()
                        }),
                    };
                    let _ = sender.send((revision, structure));
                });
                self.structure_job = Some((Instant::now(), receiver));
            }
        }
        self.structure.as_ref().map(|(_, s)| s.clone())
    }

    /// What another thread needs to highlight lines of this revision (the
    /// minimap): the tree when it is current, the rules of a simple
    /// language, or nothing (plain text; syntect languages too, whose
    /// line states live here).
    pub fn highlight_source(&self, doc: &Document) -> HighlightSource {
        match &self.engine {
            Engine::TreeSitter(_) => match self.current_tree(doc) {
                Some(tree) => HighlightSource::Tree(tree.clone()),
                None => HighlightSource::Plain,
            },
            Engine::Simple(language) => HighlightSource::Simple(language),
            Engine::Lines(_) | Engine::Plain => HighlightSource::Plain,
        }
    }

    /// Revision the last [`Syntax::structure`] result describes.
    pub fn structure_revision(&self) -> Option<Revision> {
        self.structure.as_ref().map(|(rev, _)| *rev)
    }

    /// Waits for running background work (tests and benchmarks).
    pub fn wait_idle(&mut self, doc: &Document) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.sync(doc);
            let _ = self.structure(doc);
            let fresh = self.structure.as_ref().map(|(rev, _)| *rev) == Some(doc.revision())
                || self.current_tree(doc).is_none() && !self.is_pending();
            if (!self.is_pending() && fresh) || Instant::now() > deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// A snapshot of a document's highlighting, usable on another thread.
#[derive(Clone)]
pub enum HighlightSource {
    Tree(Tree),
    Simple(&'static SimpleLanguage),
    Plain,
}

impl HighlightSource {
    /// Whether the spans carry colors (not plain text).
    pub fn is_colored(&self) -> bool {
        !matches!(self, HighlightSource::Plain)
    }

    /// Spans of lines `first..=last` of `rope` (which must be the text the
    /// source was taken from).
    pub fn spans(&self, rope: &Rope, first: usize, last: usize) -> Vec<Vec<Span>> {
        let last = last.min(rope.len_lines().saturating_sub(1));
        if first > last {
            return Vec::new();
        }
        match self {
            HighlightSource::Tree(tree) => tree_sitter_spans(tree, rope, first, last),
            HighlightSource::Simple(language) => (first..=last)
                .map(|line| {
                    language
                        .highlight_line(&rope.line(line).to_string())
                        .into_iter()
                        .map(|(start, end, role)| Span { start, end, role })
                        .collect()
                })
                .collect(),
            HighlightSource::Plain => vec![Vec::new(); last + 1 - first],
        }
    }
}

/// Small subtrees covering `range`, and the big nodes passed on the way.
fn query_roots<'t>(
    node: Node<'t>,
    range: &std::ops::Range<usize>,
    budget: usize,
    roots: &mut Vec<Node<'t>>,
    ancestors: &mut Vec<Node<'t>>,
) {
    let span = node.byte_range();
    if span.end <= range.start || span.start >= range.end {
        return;
    }
    if span.len() <= budget || node.child_count() == 0 {
        roots.push(node);
        return;
    }
    ancestors.push(node);
    let mut cursor = node.walk();
    if cursor.goto_first_child_for_byte(range.start).is_none() {
        return;
    }
    loop {
        let child = cursor.node();
        if child.start_byte() >= range.end {
            break;
        }
        query_roots(child, range, budget, roots, ancestors);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn tree_sitter_spans(tree: &Tree, rope: &Rope, first: usize, last: usize) -> Vec<Vec<Span>> {
    let highlights = highlights();
    let start_byte = rope.line_to_byte(first);
    let end_byte = if last + 1 < rope.len_lines() {
        rope.line_to_byte(last + 1)
    } else {
        rope.len_bytes()
    };
    let text = rope.byte_slice(start_byte..end_byte).to_string();
    let mut captures: Vec<(usize, usize, usize, SyntaxRole)> = Vec::new();
    let range = start_byte..end_byte;
    // Querying from the root walks every sibling of huge nodes (a class with
    // thousands of methods); querying small subtrees that intersect the
    // range keeps the cost proportional to what is visible. Big nodes on
    // the way down are still matched as pattern roots (method names of
    // long methods, for instance).
    let budget = (range.len() * 4).max(4096);
    let mut roots = Vec::new();
    let mut ancestors = Vec::new();
    query_roots(tree.root_node(), &range, budget, &mut roots, &mut ancestors);
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let mut collect = |cursor: &mut QueryCursor, node: Node| {
        let mut matches = cursor.captures(&highlights.query, node, RopeText(rope));
        while let Some((m, index)) = matches.next() {
            let capture = m.captures[*index];
            if let Some(role) = highlights.roles[capture.index as usize] {
                let r = capture.node.byte_range();
                captures.push((r.start, r.end, m.pattern_index, role));
            }
        }
    };
    cursor.set_max_start_depth(Some(0));
    for node in &ancestors {
        collect(&mut cursor, *node);
    }
    cursor.set_max_start_depth(None);
    for node in &roots {
        collect(&mut cursor, *node);
    }
    // Outer nodes first, inner ones paint over them; for the same node the
    // later pattern wins.
    captures.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.2.cmp(&b.2)));
    let mut roles: Vec<Option<SyntaxRole>> = vec![None; text.len()];
    for (start, end, _, role) in captures {
        let from = start.max(start_byte) - start_byte;
        let to = end.min(end_byte).saturating_sub(start_byte);
        for slot in roles.iter_mut().take(to).skip(from) {
            *slot = Some(role);
        }
    }
    let mut out = Vec::with_capacity(last + 1 - first);
    let mut spans: Vec<Span> = Vec::new();
    let mut column = 0;
    for (byte, ch) in text.char_indices() {
        if ch == '\n' {
            out.push(std::mem::take(&mut spans));
            column = 0;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        if let Some(role) = roles[byte] {
            match spans.last_mut() {
                Some(span) if span.role == role && span.end == column => span.end += 1,
                _ => spans.push(Span {
                    start: column,
                    end: column + 1,
                    role,
                }),
            }
        }
        column += 1;
    }
    out.push(spans);
    out.truncate(last + 1 - first);
    while out.len() < last + 1 - first {
        out.push(Vec::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::{Buffer, ChangeSet, EditKind};

    fn role_of(
        syntax: &mut Syntax,
        doc: &Document,
        line: usize,
        needle: &str,
    ) -> Option<SyntaxRole> {
        let text = crate::textutil::line_text(doc.rope(), line);
        let column = text.find(needle).map(|b| text[..b].chars().count())?;
        let spans = syntax.line_spans(doc, line, line);
        spans[0]
            .iter()
            .find(|s| s.start <= column && column < s.end)
            .map(|s| s.role)
    }

    const SAMPLE: &str = "<?php\nnamespace App;\n\n/** Doc. */\nfinal class User extends Model\n{\n    public function name(int $id): string\n    {\n        // note\n        return \"x\" . 42 . $this->id . self::LIMIT;\n    }\n}\n";

    #[test]
    fn extra_query_compiles() {
        let language = php_language();
        let source = format!("{}\n{}", tree_sitter_php::HIGHLIGHTS_QUERY, EXTRA_QUERY);
        Query::new(&language, &source).expect("extra patterns are valid");
    }

    #[test]
    fn php_roles_from_the_tree() {
        let doc = Document::new(SAMPLE);
        let mut syntax = Syntax::for_path(Some(Path::new("User.php")));
        assert_eq!(
            role_of(&mut syntax, &doc, 0, "<?php"),
            Some(SyntaxRole::Keyword)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 1, "App"),
            Some(SyntaxRole::Namespace)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 3, "Doc"),
            Some(SyntaxRole::Documentation)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 4, "class"),
            Some(SyntaxRole::Keyword)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 4, "User"),
            Some(SyntaxRole::Type)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 4, "Model"),
            Some(SyntaxRole::Type)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 6, "name"),
            Some(SyntaxRole::Method)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 6, "int"),
            Some(SyntaxRole::Keyword)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 6, "$id"),
            Some(SyntaxRole::Parameter)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 8, "note"),
            Some(SyntaxRole::Comment)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 9, "\"x\""),
            Some(SyntaxRole::String)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 9, "42"),
            Some(SyntaxRole::Number)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 9, "this"),
            Some(SyntaxRole::Keyword)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 9, "id ."),
            Some(SyntaxRole::Property)
        );
        assert_eq!(
            role_of(&mut syntax, &doc, 9, "LIMIT"),
            Some(SyntaxRole::Constant)
        );
    }

    #[test]
    fn incremental_reparse_matches_a_full_parse() {
        let mut buffer = Buffer::untitled();
        buffer.replace_all(SAMPLE);
        let mut syntax = Syntax::for_path(None);
        syntax.line_spans(&buffer.document, 0, 11);
        // Comment out a line, then type a string, as edits.
        let at = buffer.document.line_start(9);
        buffer.edit(
            ChangeSet::single(at, at, "//"),
            EditKind::Other,
            vec![],
            vec![],
        );
        let at = buffer.document.line_start(2);
        buffer.edit(
            ChangeSet::single(at, at, "$s = 'new';"),
            EditKind::Other,
            vec![],
            vec![],
        );
        let incremental = syntax.line_spans(&buffer.document, 0, 12);
        let mut fresh = Syntax::for_path(None);
        let full = fresh.line_spans(&Document::new(&buffer.document.text()), 0, 12);
        assert_eq!(incremental, full);
        assert_eq!(
            role_of(&mut syntax, &buffer.document, 9, "return"),
            Some(SyntaxRole::Comment)
        );
        assert_eq!(
            role_of(&mut syntax, &buffer.document, 2, "'new'"),
            Some(SyntaxRole::String)
        );
    }

    #[test]
    fn structure_comes_from_the_same_tree() {
        let doc = Document::new(SAMPLE);
        let mut syntax = Syntax::for_path(Some(Path::new("User.php")));
        syntax.wait_idle(&doc);
        let structure = syntax.structure(&doc).expect("structure computed");
        assert!(structure.folds.iter().any(|f| f.kind == "class"));
        assert!(structure
            .report
            .blocks
            .iter()
            .any(|b| b.kind == "method_declaration" && b.name == "name"));
        assert!(structure.report.errors.is_empty());
    }

    #[test]
    fn other_languages_use_line_states() {
        let doc = Document::new("const a = 1; // hi\n");
        let mut syntax = Syntax::for_path(Some(Path::new("app.js")));
        assert_ne!(syntax.language_name(), "Plain text");
        let spans = syntax.line_spans(&doc, 0, 0);
        assert!(spans[0].iter().any(|s| s.role == SyntaxRole::Comment));
        let mut plain = Syntax::for_path(Some(Path::new("notes.unknownext")));
        assert!(plain.line_spans(&doc, 0, 0)[0].is_empty());
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use phpide_core::{Buffer, ChangeSet, EditKind};
    use std::time::Instant;

    #[test]
    #[ignore = "timing probe: cargo test --release -p phpide-editor timing -- --ignored --nocapture"]
    fn large_file_timings() {
        let text = phpide_core::fixtures::large_php_document(50_000);
        let t = Instant::now();
        let mut buffer = Buffer::untitled();
        buffer.replace_all(&text);
        println!("load 50k lines: {:?}", t.elapsed());
        let mut syntax = Syntax::for_path(None);
        let t = Instant::now();
        syntax.sync(&buffer.document);
        println!("open (parse starts in background): {:?}", t.elapsed());
        syntax.wait_idle(&buffer.document);
        println!("background parse + structure: {:?}", t.elapsed());
        let t = Instant::now();
        syntax.line_spans(&buffer.document, 25_000, 25_060);
        println!("highlight 60 lines: {:?}", t.elapsed());
        let s = syntax.structure(&buffer.document).unwrap();
        println!("{} folds", s.folds.len());
        let at = buffer.document.line_start(25_010) + 4;
        let t = Instant::now();
        buffer.edit(
            ChangeSet::single(at, at, "x"),
            EditKind::Typing,
            vec![],
            vec![],
        );
        syntax.line_spans(&buffer.document, 25_000, 25_060);
        println!("keystroke reparse + highlight: {:?}", t.elapsed());
    }
}
