//! Ranges de folding estrutural — `T-UI-ED-002-01/04` (REQUISITOS UI-ED-002).
//!
//! Deriva regiões dobráveis da árvore tree-sitter: classes, métodos/funções,
//! blocos de controle, arrays multilinha e comentários de bloco. Garantias
//! do aceite UI-ED-002:
//! - **syntax error não trava nem oculta o arquivo inteiro**: nós ERROR são
//!   ignorados e, quando há erro no arquivo, ranges que cobrem quase tudo
//!   (> 90%) são descartados;
//! - **ranges inválidos** (end < start, fora do arquivo) são descartados;
//! - aninhamento é preservado (ordenação por início, com fim decrescente
//!   para o mais externo vir antes).

/// Uma região dobrável. Linhas 0-based, `end_line0` inclusivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line0: usize,
    pub end_line0: usize,
    /// namespace/class/interface/trait/enum/method/function/block/array/comment.
    pub kind: &'static str,
}

/// Ranges de folding de `code`. `total_lines` é `code.lines().count()`
/// (passado para clampar sem re-contar).
pub fn folding_ranges(code: &str, total_lines: usize) -> Vec<FoldRange> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&crate::language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(code, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let has_error = tree.root_node().has_error();
    collect(tree.root_node(), &mut out);
    let mut out: Vec<FoldRange> = out
        .into_iter()
        .filter(|r| {
            if r.end_line0 <= r.start_line0 || r.end_line0 >= total_lines.max(1) {
                return false;
            }
            // Com erro no arquivo, não deixar um range engolir quase tudo.
            if has_error && total_lines > 10 {
                let cover = r.end_line0 - r.start_line0;
                if cover * 10 > total_lines * 9 {
                    return false;
                }
            }
            true
        })
        .collect();
    // Ordenação estável: início crescente; no mesmo início, o mais externo
    // (maior fim) primeiro — é o pai no aninhamento.
    out.sort_by(|a, b| {
        a.start_line0
            .cmp(&b.start_line0)
            .then(b.end_line0.cmp(&a.end_line0))
    });
    out
}

fn collect(node: tree_sitter::Node, out: &mut Vec<FoldRange>) {
    if node.is_error() || node.is_missing() {
        return; // não desce em ERROR: filhos geram ranges de ruído
    }
    let kind = node.kind();
    let kind_label: &'static str = match kind {
        "namespace_definition" => "namespace",
        "class_declaration"
        | "interface_declaration"
        | "trait_declaration"
        | "enum_declaration" => "class",
        "method_declaration" => "method",
        "function_declaration" => "function",
        "compound_statement" | "block" => "block",
        "array_creation_expression" => "array",
        "comment" => "comment",
        _ => "",
    };
    if !kind_label.is_empty() {
        // Só comentários de bloco multilinha (// não é 'comment' no PHP, mas
        // guarda custa nada).
        out.push(FoldRange {
            start_line0: node.start_position().row,
            end_line0: node.end_position().row,
            kind: kind_label,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_nested_class_method_and_block() {
        let code = "<?php\nclass A {\n    public function f(): void {\n        if (true) {\n            echo 1;\n        }\n    }\n}\n";
        let ranges = folding_ranges(code, code.lines().count());
        let kinds: Vec<_> = ranges.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&"class"), "{kinds:?}");
        assert!(kinds.contains(&"method"), "{kinds:?}");
        assert!(kinds.contains(&"block"), "{kinds:?}");
        // Aninhamento: class antes de method antes de block no mesmo prefixo.
        let class = ranges.iter().find(|r| r.kind == "class").unwrap();
        let method = ranges.iter().find(|r| r.kind == "method").unwrap();
        assert!(class.start_line0 < method.start_line0);
        assert!(class.end_line0 > method.end_line0);
        // Linha única não dobra.
        assert!(ranges.iter().all(|r| r.end_line0 > r.start_line0));
    }

    #[test]
    fn folds_multiline_array_and_block_comment() {
        let code = "<?php\n$a = [\n    1,\n    2,\n];\n/* comentário\n   de bloco */\necho 1;\n";
        let ranges = folding_ranges(code, code.lines().count());
        assert!(ranges.iter().any(|r| r.kind == "array"), "{ranges:?}");
        assert!(ranges.iter().any(|r| r.kind == "comment"), "{ranges:?}");
    }

    #[test]
    fn syntax_error_does_not_fold_whole_file() {
        // Erro cedo: o parser engoliria quase tudo em um único nó; o guarda
        // (>90% com erro) protege. Arquivo pequeno (<10 linhas) nem chega a
        // aplicar o guarda — usa arquivo maior.
        let mut code = String::from("<?php\nclass A {\n    public function f( {\n");
        for i in 0..30 {
            code.push_str(&format!("echo {i};\n"));
        }
        let total = code.lines().count();
        let ranges = folding_ranges(&code, total);
        assert!(
            ranges
                .iter()
                .all(|r| (r.end_line0 - r.start_line0) * 10 <= total * 9),
            "nenhum range cobre >90% do arquivo com erro: {ranges:?}"
        );
    }

    #[test]
    fn invalid_ranges_are_dropped() {
        let code = "<?php\n$a = 1;\n";
        let total = code.lines().count();
        let ranges = folding_ranges(code, total);
        assert!(
            ranges
                .iter()
                .all(|r| r.end_line0 < total.max(1) && r.end_line0 > r.start_line0),
            "{ranges:?}"
        );
    }
}
