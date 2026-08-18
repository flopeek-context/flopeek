//! TypeScript and TSX structural facts.
//!
//! The parser reports bounded syntax facts and binding information. Module
//! resolution is assembled by the graph layer; dynamic dispatch, runtime
//! causality, and source-text persistence remain unsupported.

use crate::discovery::language_for_path;
use crate::model::{
    SourcePosition, TYPESCRIPT_FACTS_SCHEMA, TypeScriptCall, TypeScriptDeclaration,
    TypeScriptExport, TypeScriptFacts, TypeScriptImport,
};
use std::collections::BTreeSet;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

pub const PARSER_IDENTITY: &str = "tree-sitter-typescript/0.23.2";

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
    let mut exports = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "import_statement" => parse_import_statement(child, source, &mut imports),
            "export_statement" => {
                parse_export_statement(
                    child,
                    source,
                    &mut imports,
                    &mut declarations,
                    &mut exports,
                );
            }
            _ => collect_declarations(child, source, false, None, &mut declarations),
        }
    }

    let mut calls = Vec::new();
    collect_calls(root, source, &mut calls);
    let mut unsupported = Vec::new();
    if root.has_error() {
        unsupported.push("tree-sitter-recovered-syntax".to_string());
    }
    for declaration in declarations
        .iter()
        .filter(|declaration| declaration.exported)
    {
        if !exports.iter().any(|export| {
            export.source.is_none()
                && export.local_name.as_deref() == Some(declaration.name.as_str())
                && export.exported_name == declaration.name
        }) {
            exports.push(TypeScriptExport {
                exported_name: declaration.name.clone(),
                local_name: Some(declaration.name.clone()),
                kind: declaration.kind.clone(),
                source: None,
                type_only: is_type_declaration(&declaration.kind),
            });
        }
    }

    imports.sort_by(|left, right| {
        left.specifier
            .cmp(&right.specifier)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.imported_name.cmp(&right.imported_name))
            .then_with(|| left.local_name.cmp(&right.local_name))
            .then_with(|| left.position.start_line.cmp(&right.position.start_line))
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
    });
    declarations.sort_by(|left, right| {
        left.qualified_name
            .cmp(&right.qualified_name)
            .then_with(|| left.position.start_line.cmp(&right.position.start_line))
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
    });
    exports.sort_by(|left, right| {
        left.exported_name
            .cmp(&right.exported_name)
            .then_with(|| left.local_name.cmp(&right.local_name))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    exports.dedup();
    calls.sort_by(|left, right| {
        left.position
            .start_line
            .cmp(&right.position.start_line)
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
            .then_with(|| left.callee.cmp(&right.callee))
            .then_with(|| left.callee_form.cmp(&right.callee_form))
    });
    Ok(TypeScriptFacts {
        schema_version: TYPESCRIPT_FACTS_SCHEMA.to_string(),
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
        exports,
        calls,
        unsupported,
        resolution_records: Vec::new(),
        canonical_fingerprint: blake3::hash(canonical_ast(root, source).as_bytes())
            .to_hex()
            .to_string(),
    })
}

fn parse_import_statement(node: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptImport>) {
    let Some(specifier) = string_field(node, "source", source) else {
        return;
    };
    let statement_type_only =
        node_text(node, source).is_some_and(|text| text.trim_start().starts_with("import type"));
    let Some(clause) = node
        .child_by_field_name("import_clause")
        .or_else(|| named_child_kind(node, "import_clause"))
    else {
        output.push(TypeScriptImport {
            specifier,
            kind: "side-effect-import".to_string(),
            position: position(node),
            local_name: None,
            imported_name: None,
            type_only: false,
        });
        return;
    };
    let mut cursor = clause.walk();
    let children = clause.named_children(&mut cursor).collect::<Vec<_>>();
    for child in children {
        match child.kind() {
            "identifier" => {
                let Some(local_name) = node_text(child, source) else {
                    continue;
                };
                output.push(TypeScriptImport {
                    specifier: specifier.clone(),
                    kind: "default-import".to_string(),
                    position: position(child),
                    local_name: Some(local_name),
                    imported_name: Some("default".to_string()),
                    type_only: statement_type_only,
                });
            }
            "namespace_import" => {
                let Some(local_name) = child
                    .named_child(0)
                    .and_then(|name| node_text(name, source))
                else {
                    continue;
                };
                output.push(TypeScriptImport {
                    specifier: specifier.clone(),
                    kind: "namespace-import".to_string(),
                    position: position(child),
                    local_name: Some(local_name),
                    imported_name: None,
                    type_only: statement_type_only,
                });
            }
            "named_imports" => {
                let mut named_cursor = child.walk();
                for specifier_node in child.named_children(&mut named_cursor) {
                    if specifier_node.kind() != "import_specifier" {
                        continue;
                    }
                    let Some(imported_name) = specifier_node
                        .child_by_field_name("name")
                        .and_then(|name| node_text(name, source))
                    else {
                        continue;
                    };
                    let local_name = specifier_node
                        .child_by_field_name("alias")
                        .and_then(|alias| node_text(alias, source))
                        .unwrap_or_else(|| imported_name.clone());
                    let specifier_type_only = node_text(specifier_node, source)
                        .is_some_and(|text| text.trim_start().starts_with("type "));
                    output.push(TypeScriptImport {
                        specifier: specifier.clone(),
                        kind: "named-import".to_string(),
                        position: position(specifier_node),
                        local_name: Some(local_name),
                        imported_name: Some(imported_name),
                        type_only: statement_type_only || specifier_type_only,
                    });
                }
            }
            _ => {}
        }
    }
}

fn parse_export_statement(
    node: Node<'_>,
    source: &[u8],
    imports: &mut Vec<TypeScriptImport>,
    declarations: &mut Vec<TypeScriptDeclaration>,
    exports: &mut Vec<TypeScriptExport>,
) {
    let text = node_text(node, source).unwrap_or_default();
    let trimmed = text.trim_start();
    let default_export = trimmed.starts_with("export default");
    let type_only = trimmed.starts_with("export type")
        || trimmed
            .strip_prefix("export default")
            .is_some_and(|rest| rest.trim_start().starts_with("type "));
    let source_specifier = string_field(node, "source", source);

    if let Some(declaration) = node.child_by_field_name("declaration") {
        let before = declarations.len();
        let forced_name = if declaration_name(declaration, source).is_none()
            && declaration_kind(declaration).is_some()
        {
            Some("default")
        } else {
            None
        };
        collect_declarations(
            declaration,
            source,
            !default_export,
            forced_name,
            declarations,
        );
        for declaration in declarations[before..].iter() {
            exports.push(TypeScriptExport {
                exported_name: if default_export {
                    "default".to_string()
                } else {
                    declaration.name.clone()
                },
                local_name: Some(declaration.name.clone()),
                kind: declaration.kind.clone(),
                source: None,
                type_only: type_only || is_type_declaration(&declaration.kind),
            });
        }
        return;
    }

    if let Some(clause) = node.child_by_field_name("value")
        && default_export
    {
        if declaration_kind(clause).is_some() {
            collect_declarations(clause, source, false, Some("default"), declarations);
        }
        let local_name = if clause.kind() == "identifier" {
            node_text(clause, source)
        } else if declaration_kind(clause).is_some() {
            Some("default".to_string())
        } else {
            None
        };
        exports.push(TypeScriptExport {
            exported_name: "default".to_string(),
            local_name,
            kind: "default-expression".to_string(),
            source: None,
            type_only,
        });
    }

    if let Some(clause) = node
        .child_by_field_name("export_clause")
        .or_else(|| named_child_kind(node, "export_clause"))
    {
        let mut cursor = clause.walk();
        for specifier in clause.named_children(&mut cursor) {
            if specifier.kind() != "export_specifier" {
                continue;
            }
            let Some(local_name) = specifier
                .child_by_field_name("name")
                .and_then(|name| node_text(name, source))
            else {
                continue;
            };
            let exported_name = specifier
                .child_by_field_name("alias")
                .and_then(|alias| node_text(alias, source))
                .unwrap_or_else(|| local_name.clone());
            exports.push(TypeScriptExport {
                exported_name: exported_name.clone(),
                local_name: Some(local_name.clone()),
                kind: if source_specifier.is_some() {
                    "re-export".to_string()
                } else {
                    "local-export".to_string()
                },
                source: source_specifier.clone(),
                type_only,
            });
            if let Some(module_specifier) = source_specifier.as_ref() {
                imports.push(TypeScriptImport {
                    specifier: module_specifier.clone(),
                    kind: "re-export".to_string(),
                    position: position(specifier),
                    local_name: Some(exported_name),
                    imported_name: Some(local_name),
                    type_only,
                });
            }
        }
    }

    if let Some(namespace_export) = trimmed
        .strip_prefix("export * as ")
        .and_then(|rest| rest.split_once(" from "))
    {
        let exported_name = namespace_export.0.trim().trim_end_matches(';');
        if !exported_name.is_empty() {
            exports.push(TypeScriptExport {
                exported_name: exported_name.to_string(),
                local_name: None,
                kind: "namespace-re-export".to_string(),
                source: source_specifier.clone(),
                type_only,
            });
        }
        if let Some(specifier) = source_specifier {
            imports.push(TypeScriptImport {
                specifier,
                kind: "re-export".to_string(),
                position: position(node),
                local_name: Some(exported_name.to_string()),
                imported_name: Some("*".to_string()),
                type_only,
            });
        }
    } else if node
        .named_child(0)
        .is_some_and(|child| child.kind() == "namespace_export")
        || trimmed.contains("export *")
    {
        exports.push(TypeScriptExport {
            exported_name: "*".to_string(),
            local_name: None,
            kind: "re-export".to_string(),
            source: source_specifier.clone(),
            type_only,
        });
        if let Some(specifier) = source_specifier {
            imports.push(TypeScriptImport {
                specifier,
                kind: "re-export".to_string(),
                position: position(node),
                local_name: None,
                imported_name: None,
                type_only,
            });
        }
    }
}

fn collect_declarations(
    node: Node<'_>,
    source: &[u8],
    exported: bool,
    forced_name: Option<&str>,
    output: &mut Vec<TypeScriptDeclaration>,
) {
    if let Some(kind) = declaration_kind(node)
        && let Some(name) = forced_name
            .map(ToOwned::to_owned)
            .or_else(|| declaration_name(node, source))
    {
        let kind = kind.to_string();
        let qualified_name = format!("{}:{name}", normalize_symbol_kind(&kind));
        output.push(TypeScriptDeclaration {
            name,
            kind: kind.clone(),
            exported,
            position: position(node),
            qualified_name,
            ast_fingerprint: blake3::hash(canonical_ast(node, source).as_bytes())
                .to_hex()
                .to_string(),
        });
    }
    if matches!(node.kind(), "lexical_declaration" | "ambient_declaration") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_declarations(child, source, exported, None, output);
        }
    }
}

fn declaration_kind(node: Node<'_>) -> Option<&'static str> {
    match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "arrow_function" => Some("function"),
        "class_declaration" | "abstract_class_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        "enum_declaration" => Some("enum"),
        "function_signature" => Some("function-signature"),
        "class_signature" => Some("class-signature"),
        "variable_declarator" => Some("variable"),
        _ => None,
    }
}

fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| node_text(name, source))
        .filter(|name| is_simple_identifier(name))
}

fn is_type_declaration(kind: &str) -> bool {
    matches!(kind, "interface" | "type" | "class-signature")
}

fn normalize_symbol_kind(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn collect_calls(root: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptCall>) {
    fn visit(node: Node<'_>, root: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptCall>) {
        if node.kind() != "call_expression" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                visit(child, root, source, output);
            }
            return;
        }
        let function = node.child_by_field_name("function");
        let syntax = function.and_then(|function| callee_syntax(function, source));
        let caller = enclosing_top_level_owner(node, root, source);
        let shadowed = syntax.as_ref().is_some_and(|syntax| {
            syntax
                .receiver
                .as_deref()
                .or(syntax.callee.as_deref())
                .is_some_and(|name| is_shadowed_in_owner(node, root, source, name))
        });
        output.push(TypeScriptCall {
            callee: syntax.as_ref().and_then(|syntax| syntax.callee.clone()),
            dynamic: syntax.is_none(),
            position: position(node),
            caller,
            callee_form: syntax
                .as_ref()
                .map_or_else(|| "dynamic".to_string(), |syntax| syntax.form.clone()),
            receiver: syntax.and_then(|syntax| syntax.receiver),
            shadowed,
        });
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, root, source, output);
        }
    }
    visit(root, root, source, output);
}

#[derive(Debug, Clone)]
struct CalleeSyntax {
    callee: Option<String>,
    form: String,
    receiver: Option<String>,
}

fn callee_syntax(node: Node<'_>, source: &[u8]) -> Option<CalleeSyntax> {
    match node.kind() {
        "identifier" => node_text(node, source).map(|callee| CalleeSyntax {
            callee: Some(callee),
            form: "identifier".to_string(),
            receiver: None,
        }),
        "member_expression" => {
            let receiver = node
                .child_by_field_name("object")
                .and_then(|value| node_text(value, source));
            let property = node
                .child_by_field_name("property")
                .and_then(|value| node_text(value, source));
            match (receiver, property) {
                (Some(receiver), Some(property))
                    if is_simple_identifier(&receiver)
                        && is_simple_identifier(&property)
                        && node_text(node, source)
                            .is_some_and(|text| text == format!("{receiver}.{property}")) =>
                {
                    Some(CalleeSyntax {
                        callee: Some(format!("{receiver}.{property}")),
                        form: "member".to_string(),
                        receiver: Some(receiver),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn enclosing_top_level_owner(node: Node<'_>, root: Node<'_>, source: &[u8]) -> Option<String> {
    let top = top_level_child(node, root)?;
    let declaration = if top.kind() == "export_statement" {
        exported_child(top).unwrap_or(top)
    } else {
        top
    };
    if declaration.kind() == "lexical_declaration" {
        let mut cursor = declaration.walk();
        return declaration
            .named_children(&mut cursor)
            .find(|child| contains_range(child.range(), node.range()))
            .and_then(|child| declaration_name(child, source))
            .map(|name| format!("variable:{name}"));
    }
    declaration_kind(declaration).and_then(|kind| {
        declaration_name(declaration, source)
            .or_else(|| (kind == "function" || kind == "class").then(|| "default".to_string()))
            .map(|name| format!("{}:{name}", normalize_symbol_kind(kind)))
    })
}

fn top_level_child<'a>(node: Node<'a>, root: Node<'a>) -> Option<Node<'a>> {
    let mut current = node;
    loop {
        let parent = current.parent()?;
        if parent.id() == root.id() {
            return Some(current);
        }
        current = parent;
    }
}

fn is_shadowed_in_owner(node: Node<'_>, root: Node<'_>, source: &[u8], name: &str) -> bool {
    let Some(top) = top_level_child(node, root) else {
        return false;
    };
    let owner = if top.kind() == "export_statement" {
        exported_child(top).unwrap_or(top)
    } else {
        top
    };
    if !contains_range(owner.range(), node.range()) {
        return false;
    }
    binding_names(owner, source).contains(name)
}

fn binding_names(node: Node<'_>, source: &[u8]) -> BTreeSet<String> {
    let mut output = BTreeSet::new();
    fn visit(node: Node<'_>, source: &[u8], output: &mut BTreeSet<String>) {
        match node.kind() {
            "variable_declarator" => {
                if let Some(name) = declaration_name(node, source) {
                    output.insert(name);
                }
            }
            kind if kind.ends_with("parameter") => {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("pattern"))
                    .or_else(|| node.named_child(0))
                    .and_then(|name| node_text(name, source))
                    .filter(|name| is_simple_identifier(name))
                {
                    output.insert(name);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, source, output);
        }
    }
    visit(node, source, &mut output);
    output
}

fn contains_range(outer: tree_sitter::Range, inner: tree_sitter::Range) -> bool {
    outer.start_byte <= inner.start_byte && outer.end_byte >= inner.end_byte
}

fn exported_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "export_clause" && child.kind() != "namespace_export")
}

fn named_child_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn canonical_ast(node: Node<'_>, source: &[u8]) -> String {
    if node.kind() == "comment" {
        return String::new();
    }
    if matches!(node.kind(), "string" | "template_string")
        && let Some(text) = node_text(node, source)
    {
        let value = text
            .strip_prefix(['\'', '"', '`'])
            .and_then(|value| value.strip_suffix(['\'', '"', '`']))
            .unwrap_or(&text);
        return format!("({}:literal:{value})", node.kind());
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
    fn extracts_direct_import_bindings_and_call_owner() {
        let facts = parse(
            "src/checkout.ts",
            b"import { charge as debit, type Card } from './payments';\nimport payment, * as ns from './payment';\nexport function checkout() { return debit(); }\n",
            "hash-ts",
        )
        .expect("parse TypeScript");
        assert_eq!(facts.schema_version, TYPESCRIPT_FACTS_SCHEMA);
        assert_eq!(facts.parser, PARSER_IDENTITY);
        assert!(
            facts
                .imports
                .iter()
                .any(|item| item.local_name.as_deref() == Some("debit")
                    && item.imported_name.as_deref() == Some("charge"))
        );
        assert!(facts.imports.iter().any(
            |item| item.kind == "namespace-import" && item.local_name.as_deref() == Some("ns")
        ));
        assert!(
            facts
                .imports
                .iter()
                .any(|item| item.type_only && item.local_name.as_deref() == Some("Card"))
        );
        assert_eq!(facts.calls[0].callee.as_deref(), Some("debit"));
        assert_eq!(facts.calls[0].caller.as_deref(), Some("function:checkout"));
        assert_eq!(facts.calls[0].callee_form, "identifier");
    }

    #[test]
    fn extracts_all_direct_import_and_export_forms_without_source_body() {
        let facts = parse(
            "src/entry.ts",
            br#"import defaultValue, { charge as debit, type Card } from './payment';
import * as payments from './payments';
import './side-effect';
import type { Receipt } from './types';
export { debit as charge };
 export { charge as reexported } from './payment';
 export * from './payments';
 export * as paymentNamespace from './payment';
 export default function () { return defaultValue(); }
"#,
            "hash-imports",
        )
        .expect("parse import forms");

        assert!(facts.imports.iter().any(|item| {
            item.kind == "default-import"
                && item.local_name.as_deref() == Some("defaultValue")
                && item.imported_name.as_deref() == Some("default")
        }));
        assert!(facts.imports.iter().any(|item| {
            item.kind == "named-import"
                && item.local_name.as_deref() == Some("debit")
                && item.imported_name.as_deref() == Some("charge")
        }));
        assert!(facts.imports.iter().any(|item| {
            item.kind == "namespace-import" && item.local_name.as_deref() == Some("payments")
        }));
        assert!(
            facts
                .imports
                .iter()
                .any(|item| item.kind == "side-effect-import" && item.specifier == "./side-effect")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|item| { item.type_only && item.local_name.as_deref() == Some("Receipt") })
        );
        assert!(facts.exports.iter().any(|item| {
            item.kind == "local-export"
                && item.exported_name == "charge"
                && item.local_name.as_deref() == Some("debit")
        }));
        assert!(facts.exports.iter().any(|item| {
            item.kind == "re-export"
                && item.exported_name == "reexported"
                && item.source.as_deref() == Some("./payment")
        }));
        assert!(facts.exports.iter().any(|item| {
            item.kind == "re-export" && item.exported_name == "*" && item.source.is_some()
        }));
        assert!(facts.exports.iter().any(|item| {
            item.kind == "namespace-re-export"
                && item.exported_name == "paymentNamespace"
                && item.source.as_deref() == Some("./payment")
        }));
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.qualified_name == "function:default")
        );
        let encoded = serde_json::to_string(&facts).expect("serialize facts");
        assert!(!encoded.contains("defaultValue();"));
    }

    #[test]
    fn computed_and_dynamic_calls_are_not_reduced_to_direct_members() {
        let facts = parse(
            "src/entry.ts",
            b"declare const ns: { charge(): void }; ns.charge(); ns['charge'](); ns[method](); call?.();",
            "hash-calls",
        )
        .expect("parse calls");
        assert!(facts.calls.iter().any(
            |call| call.callee.as_deref() == Some("ns.charge") && call.callee_form == "member"
        ));
        assert!(facts.calls.iter().any(|call| call.callee_form == "dynamic"));
        assert!(
            facts
                .calls
                .iter()
                .filter(|call| call.callee_form == "member")
                .all(|call| call.callee.as_deref() == Some("ns.charge"))
        );
    }

    #[test]
    fn extracts_default_and_local_exports_without_source_body() {
        let facts = parse(
            "src/payment.ts",
            b"export function charge() {}\nconst retry = () => charge();\nexport { retry as retryPayment };\nexport default charge;\n",
            "hash-ts",
        )
        .expect("parse TypeScript");
        assert!(
            facts
                .exports
                .iter()
                .any(|item| item.exported_name == "retryPayment"
                    && item.local_name.as_deref() == Some("retry"))
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|item| item.exported_name == "default"
                    && item.local_name.as_deref() == Some("charge"))
        );
        let encoded = serde_json::to_string(&facts).expect("serialize facts");
        assert!(!encoded.contains("charge();"));
    }

    #[test]
    fn extracts_anonymous_default_and_tsx() {
        let default = parse(
            "src/payment.ts",
            b"export default function () { return 1; }\n",
            "hash-default",
        )
        .expect("parse default");
        assert!(
            default
                .declarations
                .iter()
                .any(|item| item.name == "default")
        );
        assert!(
            default
                .exports
                .iter()
                .any(|item| item.exported_name == "default")
        );

        let default_arrow = parse("src/arrow.ts", b"export default () => 1;\n", "hash-arrow")
            .expect("parse default arrow");
        assert!(
            default_arrow
                .declarations
                .iter()
                .any(|item| item.qualified_name == "function:default")
        );

        let tsx = parse(
            "src/Checkout.tsx",
            b"export function Checkout() { return <button />; }\n",
            "hash-tsx",
        )
        .expect("parse TSX");
        assert_eq!(tsx.language, "tsx");
        assert!(tsx.declarations.iter().any(|item| item.name == "Checkout"));
    }

    #[test]
    fn rejects_javascript_and_non_typescript_inputs() {
        assert!(parse("src/legacy.js", b"export const x = 1;", "hash").is_err());
        assert!(parse("src/service.py", b"def service(): pass", "hash").is_err());
    }
}
