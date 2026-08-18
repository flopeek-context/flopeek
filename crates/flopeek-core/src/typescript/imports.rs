//! Import and export syntax extraction.

#[allow(unused_imports)]
use super::*;

pub(super) fn parse_import_statement(
    node: Node<'_>,
    source: &[u8],
    output: &mut Vec<TypeScriptImport>,
) {
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

pub(super) fn parse_export_statement(
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
