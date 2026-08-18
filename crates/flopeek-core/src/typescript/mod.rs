//! TypeScript and TSX structural facts.
//!
//! The parser intentionally reports only bounded, deterministic syntax facts.  It
//! does not execute modules, resolve dynamic dispatch, infer runtime causality, or
//! persist source text.

use crate::discovery::language_for_path;
use crate::model::{
    SourcePosition, TypeScriptCall, TypeScriptDeclaration, TypeScriptFacts, TypeScriptImport,
};
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

pub const PARSER_IDENTITY: &str = "tree-sitter-typescript/0.23";

pub fn parse(path: &str, source: &[u8], source_hash: &str) -> Result<TypeScriptFacts, String> {
    let language = language_for_path(Path::new(path))
        .ok_or_else(|| format!("Only TypeScript and TSX are supported: {path}"))?;
    let mut parser = Parser::new();
    let grammar = if language == "tsx" {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    parser
        .set_language(&grammar)
        .map_err(|error| format!("Unable to initialize TypeScript parser: {error}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| format!("Parser returned no tree for {path}"))?;
    parse_tree(path, source, source_hash, &tree)
}

fn parse_tree(
    path: &str,
    source: &[u8],
    source_hash: &str,
    tree: &Tree,
) -> Result<TypeScriptFacts, String> {
    let root = tree.root_node();
    let mut imports = Vec::new();
    let mut declarations = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        let (node, exported) = if child.kind() == "export_statement" {
            (exported_child(child).unwrap_or(child), true)
        } else {
            (child, false)
        };
        if node.kind() == "import_statement"
            && let Some(specifier) = string_field(node, "source", source)
        {
            imports.push(TypeScriptImport {
                specifier,
                kind: "static-import".to_string(),
                position: position(node),
            });
        }
        if node.kind() == "export_statement"
            && let Some(specifier) = string_field(node, "source", source)
        {
            imports.push(TypeScriptImport {
                specifier,
                kind: "re-export".to_string(),
                position: position(node),
            });
        }
        collect_declarations(node, source, exported, &mut declarations);
    }

    let mut calls = Vec::new();
    collect_calls(root, source, &mut calls);
    let mut unsupported = Vec::new();
    if root.has_error() {
        unsupported.push("tree-sitter-recovered-syntax".to_string());
    }
    imports.sort_by(|left, right| {
        left.specifier
            .cmp(&right.specifier)
            .then_with(|| left.position.start_line.cmp(&right.position.start_line))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    declarations.sort_by(|left, right| {
        left.position
            .start_line
            .cmp(&right.position.start_line)
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
            .then_with(|| left.name.cmp(&right.name))
    });
    calls.sort_by(|left, right| {
        left.position
            .start_line
            .cmp(&right.position.start_line)
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
            .then_with(|| left.callee.cmp(&right.callee))
    });
    Ok(TypeScriptFacts {
        path: path.to_string(),
        language: language_for_path(Path::new(path))
            .ok_or_else(|| format!("Only TypeScript and TSX are supported: {path}"))?
            .to_string(),
        source_hash: source_hash.to_string(),
        parser: PARSER_IDENTITY.to_string(),
        parse_status: if root.has_error() {
            "recovered"
        } else {
            "parsed"
        }
        .to_string(),
        imports,
        declarations,
        calls,
        unsupported,
        canonical_fingerprint: blake3::hash(canonical_ast(root, source).as_bytes())
            .to_hex()
            .to_string(),
    })
}

fn exported_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "export_clause")
}

fn collect_declarations(
    node: Node<'_>,
    source: &[u8],
    exported: bool,
    output: &mut Vec<TypeScriptDeclaration>,
) {
    let kind = match node.kind() {
        "function_declaration" => Some("function"),
        "class_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        "enum_declaration" => Some("enum"),
        "abstract_class_declaration" => Some("class"),
        "function_signature" => Some("function-signature"),
        "class_signature" => Some("class-signature"),
        "variable_declaration" | "variable_declarator" => Some("variable"),
        _ => None,
    };
    if let Some(kind) = kind
        && let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| node_text(name, source))
    {
        output.push(TypeScriptDeclaration {
            name,
            kind: kind.to_string(),
            exported,
            position: position(node),
            ast_fingerprint: blake3::hash(canonical_ast(node, source).as_bytes())
                .to_hex()
                .to_string(),
        });
    }
    if matches!(node.kind(), "lexical_declaration" | "export_statement") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "type_annotation" && child.kind() != "export_clause" {
                collect_declarations(child, source, exported, output);
            }
        }
    }
}

fn canonical_ast(node: Node<'_>, source: &[u8]) -> String {
    if node.kind() == "comment" {
        return String::new();
    }
    let mut output = String::new();
    output.push('(');
    output.push_str(node.kind());
    if node.child_count() == 0 {
        if let Ok(text) = node.utf8_text(source) {
            output.push(':');
            output.push_str(text);
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "comment" {
                continue;
            }
            output.push_str(&canonical_ast(child, source));
        }
    }
    output.push(')');
    output
}

fn collect_calls(node: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptCall>) {
    if node.kind() == "call_expression" {
        let function = node.child_by_field_name("function");
        let callee = function.and_then(|function| direct_callee(function, source));
        output.push(TypeScriptCall {
            dynamic: callee.is_none(),
            callee,
            position: position(node),
        });
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_calls(child, source, output);
    }
}

fn direct_callee(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => node_text(node, source),
        "member_expression" => {
            let object = node
                .child_by_field_name("object")
                .and_then(|value| node_text(value, source));
            let property = node
                .child_by_field_name("property")
                .and_then(|value| node_text(value, source));
            match (object, property) {
                (Some(object), Some(property))
                    if is_simple_identifier(&object) && is_simple_identifier(&property) =>
                {
                    Some(format!("{object}.{property}"))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().enumerate().all(|(index, character)| {
            character == '_'
                || character == '$'
                || character.is_ascii_alphanumeric() && (index > 0 || !character.is_ascii_digit())
        })
}

fn string_field(node: Node<'_>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field).and_then(|child| {
        node_text(child, source).and_then(|value| {
            value
                .strip_prefix(['\'', '"', '`'])
                .and_then(|value| value.strip_suffix(['\'', '"', '`']))
                .map(ToOwned::to_owned)
        })
    })
}

fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(ToOwned::to_owned)
}

fn position(node: Node<'_>) -> SourcePosition {
    let start = node.start_position();
    let end = node.end_position();
    SourcePosition {
        start_line: start.row + 1,
        start_column: start.column,
        end_line: end.row + 1,
        end_column: end.column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_typescript_and_tsx_structure_without_source_body() {
        let ts = parse(
            "src/checkout.ts",
            b"import { charge } from './payments';\nexport function checkout() { return charge(); }\n",
            "hash-ts",
        )
        .expect("parse TypeScript");
        assert_eq!(ts.language, "typescript");
        assert_eq!(ts.imports[0].specifier, "./payments");
        assert_eq!(ts.declarations[0].name, "checkout");
        assert_eq!(ts.calls[0].callee.as_deref(), Some("charge"));
        let encoded = serde_json::to_string(&ts).expect("serialize facts");
        assert!(!encoded.contains("return charge"));

        let tsx = parse(
            "src/Checkout.tsx",
            b"export function Checkout() { return <button />; }\n",
            "hash-tsx",
        )
        .expect("parse TSX");
        assert_eq!(tsx.language, "tsx");
        assert_eq!(tsx.declarations[0].name, "Checkout");
    }

    #[test]
    fn rejects_javascript_and_non_typescript_inputs() {
        assert!(parse("src/legacy.js", b"export const x = 1;", "hash").is_err());
        assert!(parse("src/service.py", b"def service(): pass", "hash").is_err());
    }
}
