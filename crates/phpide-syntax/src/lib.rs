//! Análise estrutural de PHP com tree-sitter (Estágio 3.1).
//!
//! Roda 100% offline — é o que garante blocos/erros de sintaxe mesmo sem LSP
//! (e dá suporte a saltos de bloco, folding e, no futuro, seleção estrutural).
//! O parser é incremental-friendly: o tree-sitter reaproveita a árvore antiga
//! quando analisamos com `parse(code, old_tree)`.

use tree_sitter::{Node, Parser};

pub mod folding;

/// Uma região de bloco (função, classe, if, foreach...) que pode ser dobrada
/// ou usada para navegação estrutural. Linhas 0-based.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub start_line0: usize,
    pub end_line0: usize,
    pub kind: String,
    /// Nome declarado do símbolo (vazio em blocos sem nome, ex.: if/foreach).
    /// Alimenta breadcrumbs e saltos estruturais offline.
    pub name: String,
}

/// Erro de sintaxe detectado pelo parser (nó ERROR ou símbolo faltante).
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxError {
    pub line0: usize,
    pub col0: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SyntaxReport {
    pub blocks: Vec<Block>,
    pub errors: Vec<SyntaxError>,
}

pub(crate) fn language() -> tree_sitter::Language {
    tree_sitter_php::LANGUAGE_PHP_ONLY.into()
}

/// Kinds que valem como região estrutural (equivalente ao "folding range").
/// `namespace_declaration` entra só para breadcrumbs (não é folding).
const BLOCK_KINDS: [&str; 12] = [
    "namespace_definition",
    "function_definition",
    "method_declaration",
    "class_declaration",
    "interface_declaration",
    "trait_declaration",
    "enum_declaration",
    "if_statement",
    "foreach_statement",
    "while_statement",
    "for_statement",
    "switch_statement",
];

/// Nó filho que declara o nome do bloco, por kind (campo `name` do tree-sitter).
fn name_of(node: Node, code: &str) -> String {
    node.child_by_field_name("name")
        .map(|n| n.utf8_text(code.as_bytes()).unwrap_or("").to_string())
        .unwrap_or_default()
}

/// Analisa o código e devolve blocos + erros. None só se o parser falhar.
pub fn analyze(code: &str) -> SyntaxReport {
    let mut parser = Parser::new();
    if parser.set_language(&language()).is_err() {
        return SyntaxReport::default();
    }
    let Some(tree) = parser.parse(code, None) else {
        return SyntaxReport::default();
    };
    let mut report = SyntaxReport::default();
    walk(tree.root_node(), code, &mut report);
    report.blocks.sort_by_key(|b| (b.start_line0, b.end_line0));
    report
}

fn walk(node: Node, code: &str, report: &mut SyntaxReport) {
    let kind = node.kind();
    if BLOCK_KINDS.contains(&kind) {
        let start = node.start_position().row;
        let end = node.end_position().row;
        // namespace de linha única (end == start) ainda vale para breadcrumbs.
        if end > start + 1 || kind == "namespace_definition" {
            report.blocks.push(Block {
                start_line0: start,
                end_line0: end,
                kind: kind.to_string(),
                name: name_of(node, code),
            });
        }
    }
    if node.is_error() {
        let pos = node.start_position();
        let snippet: String = node
            .utf8_text(code.as_bytes())
            .unwrap_or("")
            .chars()
            .take(40)
            .collect();
        report.errors.push(SyntaxError {
            line0: pos.row,
            col0: pos.column,
            message: format!("erro de sintaxe perto de `{snippet}`"),
        });
        // Não desce em filhos de ERROR: só geram ruído.
        return;
    }
    if node.is_missing() {
        let pos = node.start_position();
        report.errors.push(SyntaxError {
            line0: pos.row,
            col0: pos.column,
            message: format!("símbolo faltante: `{}`", node.kind()),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, code, report);
    }
}

/// Bloco estrutural que contém a linha (o mais interno). Linhas 0-based.
pub fn enclosing_block(code: &str, line0: usize) -> Option<Block> {
    let report = analyze(code);
    report
        .blocks
        .iter()
        .filter(|b| b.start_line0 <= line0 && b.end_line0 >= line0)
        .max_by_key(|b| b.start_line0)
        .cloned()
}

/// Linha efetiva de fim do bloco: um `namespace` de linha única vale até o
/// final do arquivo (semântica PHP), não só até sua própria linha.
fn effective_end(b: &Block) -> usize {
    if b.kind == "namespace_definition" {
        usize::MAX
    } else {
        b.end_line0
    }
}

/// Trilha de breadcrumbs para a linha: blocos nomeados do mais externo ao mais
/// interno (ex.: namespace → classe → método). É o que a status/navigation bar
/// do PhpStorm mostra sob o cursor.
pub fn breadcrumb_trail(code: &str, line0: usize) -> Vec<Block> {
    let report = analyze(code);
    let mut chain: Vec<Block> = report
        .blocks
        .iter()
        .filter(|b| b.start_line0 <= line0 && effective_end(b) >= line0)
        .cloned()
        .collect();
    chain.sort_by_key(|b| b.start_line0);
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<?php\nclass Greeter {\n    public function hello(): string {\n        $x = 1;\n        return 'oi';\n    }\n}\n";

    #[test]
    fn finds_class_and_method_blocks() {
        let report = analyze(SAMPLE);
        let kinds: Vec<_> = report.blocks.iter().map(|b| b.kind.as_str()).collect();
        assert!(kinds.contains(&"class_declaration"));
        assert!(kinds.contains(&"method_declaration"));
        let class = report
            .blocks
            .iter()
            .find(|b| b.kind == "class_declaration")
            .unwrap();
        assert_eq!(class.start_line0, 1);
        assert_eq!(class.end_line0, 6);
    }

    #[test]
    fn reports_syntax_errors() {
        let broken = "<?php\nfunction broken( {\n    return 1;\n";
        let report = analyze(broken);
        assert!(!report.errors.is_empty(), "esperava erro de sintaxe");
        assert_eq!(report.errors[0].line0, 1);
    }

    #[test]
    fn enclosing_block_picks_innermost() {
        let block = enclosing_block(SAMPLE, 4).expect("bloco na linha 4");
        assert_eq!(block.kind, "method_declaration");
        assert_eq!(block.start_line0, 2);
    }

    #[test]
    fn blocks_carry_symbol_names() {
        let code = "<?php\nnamespace Demo\\Loja;\nclass Product {\n    public function price(): float {\n        return 1.0;\n    }\n}\n";
        let report = analyze(code);
        let ns = report
            .blocks
            .iter()
            .find(|b| b.kind == "namespace_definition")
            .unwrap();
        assert_eq!(ns.name, "Demo\\Loja");
        let class = report
            .blocks
            .iter()
            .find(|b| b.kind == "class_declaration")
            .unwrap();
        assert_eq!(class.name, "Product");
        let method = report
            .blocks
            .iter()
            .find(|b| b.kind == "method_declaration")
            .unwrap();
        assert_eq!(method.name, "price");
    }

    #[test]
    fn breadcrumb_trail_outer_to_inner() {
        let code = "<?php\nnamespace Demo\\Loja;\nclass Product {\n    public function price(): float {\n        return 1.0;\n    }\n}\n";
        let trail = breadcrumb_trail(code, 4);
        let kinds: Vec<_> = trail.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "namespace_definition",
                "class_declaration",
                "method_declaration"
            ]
        );
        let names: Vec<_> = trail.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["Demo\\Loja", "Product", "price"]);
    }
}
