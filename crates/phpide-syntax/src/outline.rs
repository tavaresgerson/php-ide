//! File structure from the syntax tree: the declarations the Structure tool
//! window and File Structure (Ctrl+F12) list. It needs no language server, so
//! the structure is there as soon as a PHP file opens; when a server answers
//! `textDocument/documentSymbol`, the UI prefers its answer.

use tree_sitter::Node;

use crate::TextOf;

/// One declaration of the file, in document order. Lines are 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineSymbol {
    pub name: String,
    /// Same labels as the language-server path: "class", "interface",
    /// "trait", "enum", "enum case", "method", "property", "constant" and
    /// "function".
    pub kind: &'static str,
    /// Line of the declared name (where a jump should land).
    pub line0: usize,
    /// 0 for file-level declarations, 1 for members of a class-like type.
    pub depth: usize,
}

/// Declarations of a parsed file.
pub fn outline_of(root: Node, text_of: TextOf) -> Vec<OutlineSymbol> {
    let mut out = Vec::new();
    scope(root, text_of, &mut out);
    out
}

fn push(
    out: &mut Vec<OutlineSymbol>,
    name: Node,
    kind: &'static str,
    depth: usize,
    text_of: TextOf,
) {
    let text = text_of(name);
    // Code being typed can leave a declaration without a name.
    if text.trim().is_empty() {
        return;
    }
    out.push(OutlineSymbol {
        name: text,
        kind,
        line0: name.start_position().row,
        depth,
    });
}

/// File level: walks statements (namespaces, `if (!function_exists(...))`,
/// parse errors) but not into function bodies, closures or anonymous classes.
fn scope(node: Node, text_of: TextOf, out: &mut Vec<OutlineSymbol>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let type_kind = match child.kind() {
            "class_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "trait_declaration" => Some("trait"),
            "enum_declaration" => Some("enum"),
            _ => None,
        };
        if let Some(kind) = type_kind {
            if let Some(name) = child.child_by_field_name("name") {
                push(out, name, kind, 0, text_of);
            }
            if let Some(body) = child.child_by_field_name("body") {
                members(body, text_of, out);
            }
            continue;
        }
        match child.kind() {
            "function_definition" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push(out, name, "function", 0, text_of);
                }
            }
            "const_declaration" => constants(child, 0, text_of, out),
            "method_declaration" | "anonymous_function" | "arrow_function" | "anonymous_class" => {}
            _ => scope(child, text_of, out),
        }
    }
}

/// Members of a class, interface, trait or enum body.
fn members(body: Node, text_of: TextOf, out: &mut Vec<OutlineSymbol>) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "method_declaration" => {
                let Some(name) = member.child_by_field_name("name") else {
                    continue;
                };
                push(out, name, "method", 1, text_of);
                // Constructor property promotion declares properties too.
                if text_of(name).eq_ignore_ascii_case("__construct") {
                    if let Some(parameters) = member.child_by_field_name("parameters") {
                        let mut params = parameters.walk();
                        for parameter in parameters.named_children(&mut params) {
                            if parameter.kind() != "property_promotion_parameter" {
                                continue;
                            }
                            if let Some(variable) = parameter.child_by_field_name("name") {
                                push(out, variable, "property", 1, text_of);
                            }
                        }
                    }
                }
            }
            "property_declaration" => {
                let mut elements = member.walk();
                for element in member.named_children(&mut elements) {
                    if element.kind() != "property_element" {
                        continue;
                    }
                    if let Some(variable) = element.child_by_field_name("name") {
                        push(out, variable, "property", 1, text_of);
                    }
                }
            }
            "const_declaration" => constants(member, 1, text_of, out),
            "enum_case" => {
                if let Some(name) = member.child_by_field_name("name") {
                    push(out, name, "enum case", 1, text_of);
                }
            }
            _ => {}
        }
    }
}

/// `const A = 1, B = 2;` (file level or in a class).
fn constants(declaration: Node, depth: usize, text_of: TextOf, out: &mut Vec<OutlineSymbol>) {
    let mut cursor = declaration.walk();
    for element in declaration.named_children(&mut cursor) {
        if element.kind() != "const_element" {
            continue;
        }
        let mut parts = element.walk();
        let name = element
            .named_children(&mut parts)
            .find(|part| part.kind() == "name");
        if let Some(name) = name {
            push(out, name, "constant", depth, text_of);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::analyze;

    fn listed(code: &str) -> Vec<String> {
        analyze(code)
            .outline
            .iter()
            .map(|s| {
                format!(
                    "{}{} {} :{}",
                    "  ".repeat(s.depth),
                    s.kind,
                    s.name,
                    s.line0 + 1
                )
            })
            .collect()
    }

    #[test]
    fn declarations_and_members_in_order() {
        let code = "<?php
namespace App\\Services;

const VERSION = '1.0';

function helper(int $a): int { return $a; }

abstract class OrderService extends Base implements Contract
{
    public const LIMIT = 10, OTHER = 2;
    private ?Order $order = null;
    protected static $count, $total;

    public function __construct(private readonly Repo $repo) {}

    public function place(Order $order): void
    {
        $fn = function () { return 1; };
        $anon = new class { public function hidden() {} };
    }
}

interface Contract { public function place(Order $order): void; }

trait Loggable { public function log(): void {} }

enum Status: string
{
    case Open = 'open';
    public function label(): string { return 'x'; }
}
";
        assert_eq!(
            listed(code),
            [
                "constant VERSION :4",
                "function helper :6",
                "class OrderService :8",
                "  constant LIMIT :10",
                "  constant OTHER :10",
                "  property $order :11",
                "  property $count :12",
                "  property $total :12",
                "  method __construct :14",
                "  property $repo :14",
                "  method place :16",
                "interface Contract :23",
                "  method place :23",
                "trait Loggable :25",
                "  method log :25",
                "enum Status :27",
                "  enum case Open :29",
                "  method label :30",
            ]
        );
    }

    #[test]
    fn conditional_functions_and_braced_namespaces() {
        let code = "<?php
namespace A {
    if (!function_exists('helper')) {
        function helper() {}
    }
    final class X { public function a() { function inner() {} } }
}
";
        assert_eq!(
            listed(code),
            ["function helper :4", "class X :6", "  method a :6"]
        );
    }

    #[test]
    fn survives_code_being_typed() {
        let code = "<?php\nclass Broken {\n  public function a() {\n    $x = \n  }\n  public function b() {}\n}\n";
        assert_eq!(
            listed(code),
            ["class Broken :2", "  method a :3", "  method b :6"]
        );
    }
}
