//! TypeScript and TSX structural facts.
//!
//! The parser reports bounded syntax facts and binding information. Module
//! resolution is assembled by the graph layer; dynamic dispatch, runtime
//! causality, and source-text persistence remain unsupported.

use crate::discovery::language_for_path;
use crate::model::{
    SourcePosition, TYPESCRIPT_FACTS_SCHEMA, TypeScriptCall, TypeScriptDeclaration,
    TypeScriptExport, TypeScriptFacts, TypeScriptHeritage, TypeScriptImport,
};
use std::collections::BTreeSet;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

pub const PARSER_IDENTITY: &str = "tree-sitter-typescript/0.23.2";

mod calls;
mod declarations;
mod imports;
#[cfg(test)]
mod tests;

use calls::*;
use declarations::*;
use imports::*;

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
    let mut heritage = Vec::new();
    let mut unsupported = Vec::new();
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

    collect_class_metadata(
        root,
        source,
        &mut declarations,
        &mut heritage,
        &mut unsupported,
    );

    let mut calls = Vec::new();
    collect_calls(root, source, &mut calls);
    if root.has_error() {
        unsupported.push("tree-sitter-recovered-syntax".to_string());
    }
    for declaration in declarations
        .iter()
        .filter(|declaration| declaration.exported && declaration.owner.is_none())
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
    heritage.sort_by(|left, right| {
        left.owner
            .cmp(&right.owner)
            .then_with(|| left.relation.cmp(&right.relation))
            .then_with(|| left.reference.cmp(&right.reference))
            .then_with(|| left.form.cmp(&right.form))
            .then_with(|| left.position.start_line.cmp(&right.position.start_line))
            .then_with(|| left.position.start_column.cmp(&right.position.start_column))
    });
    unsupported.sort();
    unsupported.dedup();
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
        heritage,
    })
}
