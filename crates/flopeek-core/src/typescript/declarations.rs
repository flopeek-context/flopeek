//! Declaration and heritage extraction.

#[allow(unused_imports)]
use super::*;

pub(super) fn collect_declarations(
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
            ast_fingerprint: blake3::hash(
                (if type_declaration_kind(node).is_some() {
                    canonical_type_header(node, source)
                } else {
                    canonical_ast(node, source)
                })
                .as_bytes(),
            )
            .to_hex()
            .to_string(),
            owner: None,
            static_member: false,
            visibility: String::new(),
            abstract_member: false,
            declaration_only: false,
        });
    }
    if matches!(node.kind(), "lexical_declaration" | "ambient_declaration") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_declarations(child, source, exported, None, output);
        }
    }
}

pub(super) fn collect_class_metadata(
    root: Node<'_>,
    source: &[u8],
    declarations: &mut Vec<TypeScriptDeclaration>,
    heritage: &mut Vec<TypeScriptHeritage>,
    unsupported: &mut Vec<String>,
) {
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        let declaration = if child.kind() == "export_statement" {
            exported_child(child).unwrap_or(child)
        } else {
            child
        };
        let Some(kind) = type_declaration_kind(declaration) else {
            continue;
        };
        let owner_name = declaration_name(declaration, source).or_else(|| {
            (child.kind() == "export_statement"
                && node_text(child, source).is_some_and(|text| text.starts_with("export default")))
            .then(|| "default".to_string())
        });
        let Some(owner_name) = owner_name else {
            unsupported.push("anonymous-class-expression-unsupported".to_string());
            continue;
        };
        let owner = format!("{kind}:{owner_name}");
        collect_members(
            declaration,
            source,
            kind,
            &owner_name,
            declarations,
            unsupported,
        );
        collect_heritage(declaration, source, &owner, kind, heritage, unsupported);
    }
}

pub(super) fn type_declaration_kind(node: Node<'_>) -> Option<&'static str> {
    match node.kind() {
        "class_declaration" | "abstract_class_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        _ => None,
    }
}

pub(super) fn collect_members(
    declaration: Node<'_>,
    source: &[u8],
    owner_kind: &str,
    owner_name: &str,
    output: &mut Vec<TypeScriptDeclaration>,
    unsupported: &mut Vec<String>,
) {
    let Some(body) = declaration.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if !matches!(
            member.kind(),
            "method_definition" | "method_signature" | "abstract_method_signature"
        ) {
            if matches!(
                member.kind(),
                "public_field_definition"
                    | "class_static_block"
                    | "index_signature"
                    | "property_signature"
                    | "call_signature"
                    | "construct_signature"
            ) {
                unsupported.push(format!(
                    "{}-{}-unsupported",
                    owner_kind,
                    member.kind().replace('_', "-")
                ));
            }
            continue;
        }
        let Some(name_node) = member.child_by_field_name("name") else {
            unsupported.push(format!("{owner_kind}-computed-member-name"));
            continue;
        };
        let Some(name) = member_name(name_node, source) else {
            unsupported.push(format!("{owner_kind}-computed-member-name"));
            continue;
        };
        let text = node_text(member, source).unwrap_or_default();
        let words = modifier_words(
            &text,
            name_node.start_byte().saturating_sub(member.start_byte()),
        );
        let accessor = words.iter().any(|word| word == "get" || word == "set");
        if accessor {
            unsupported.push(format!("{owner_kind}-accessor-unsupported"));
            continue;
        }
        let static_member = owner_kind == "class" && words.iter().any(|word| word == "static");
        let private_member = name.starts_with('#') || words.iter().any(|word| word == "private");
        let visibility = if private_member {
            "private"
        } else if words.iter().any(|word| word == "protected") {
            "protected"
        } else if words.iter().any(|word| word == "public") {
            "public"
        } else {
            ""
        };
        let abstract_member = member.kind() == "abstract_method_signature"
            || words.iter().any(|word| word == "abstract");
        let declaration_only = member.kind() != "method_definition";
        let is_constructor = owner_kind == "class" && name == "constructor";
        let kind = if is_constructor {
            "constructor"
        } else if owner_kind == "interface" {
            "method-signature"
        } else if static_member {
            "static-method"
        } else {
            "method"
        };
        let qualified_name = format!("{}:{}.{name}", normalize_symbol_kind(kind), owner_name);
        output.push(TypeScriptDeclaration {
            name,
            kind: kind.to_string(),
            exported: false,
            position: position(member),
            qualified_name,
            ast_fingerprint: blake3::hash(canonical_ast(member, source).as_bytes())
                .to_hex()
                .to_string(),
            owner: Some(format!("{owner_kind}:{owner_name}")),
            static_member,
            visibility: visibility.to_string(),
            abstract_member,
            declaration_only,
        });
    }
}

pub(super) fn collect_heritage(
    declaration: Node<'_>,
    source: &[u8],
    owner: &str,
    owner_kind: &str,
    output: &mut Vec<TypeScriptHeritage>,
    unsupported: &mut Vec<String>,
) {
    if owner_kind == "class"
        && let Some(heritage) = named_child_kind(declaration, "class_heritage")
    {
        let mut cursor = heritage.walk();
        for clause in heritage.named_children(&mut cursor) {
            let relation = match clause.kind() {
                "extends_clause" => "extends",
                "implements_clause" => "implements",
                _ => continue,
            };
            let mut clause_cursor = clause.walk();
            for reference in clause.named_children(&mut clause_cursor) {
                if relation == "extends" && reference.kind() == "type_arguments" {
                    continue;
                }
                let (reference_text, form) = heritage_reference(reference, source);
                if form == "dynamic" {
                    unsupported.push(format!("dynamic-{relation}-unsupported"));
                }
                output.push(TypeScriptHeritage {
                    owner: owner.to_string(),
                    relation: relation.to_string(),
                    reference: reference_text,
                    form,
                    position: position(reference),
                    type_only: false,
                });
            }
        }
    }
    if owner_kind == "interface"
        && let Some(heritage) = named_child_kind(declaration, "extends_type_clause")
    {
        let mut cursor = heritage.walk();
        for reference in heritage.named_children(&mut cursor) {
            let (reference_text, form) = heritage_reference(reference, source);
            if form == "dynamic" {
                unsupported.push("dynamic-interface-extends-unsupported".to_string());
            }
            output.push(TypeScriptHeritage {
                owner: owner.to_string(),
                relation: "extends".to_string(),
                reference: reference_text,
                form,
                position: position(reference),
                type_only: false,
            });
        }
    }
}

pub(super) fn heritage_reference(node: Node<'_>, source: &[u8]) -> (String, String) {
    let text = node_text(node, source).unwrap_or_default();
    let reference = text
        .split('<')
        .next()
        .unwrap_or(text.as_str())
        .trim()
        .to_string();
    let form = if is_simple_identifier(&reference) {
        "identifier"
    } else if reference.split('.').all(is_simple_identifier) {
        "member"
    } else {
        "dynamic"
    };
    (reference, form.to_string())
}

pub(super) fn member_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "property_identifier" | "private_property_identifier" | "identifier" => {
            node_text(node, source).filter(|text| {
                is_simple_identifier(text)
                    || (text.starts_with('#') && is_simple_identifier(text.trim_start_matches('#')))
            })
        }
        _ => None,
    }
}

pub(super) fn modifier_words(text: &str, name_offset: usize) -> Vec<String> {
    text.get(..name_offset.min(text.len()))
        .unwrap_or_default()
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| !character.is_ascii_alphabetic()))
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

pub(super) fn declaration_kind(node: Node<'_>) -> Option<&'static str> {
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

pub(super) fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| node_text(name, source))
        .filter(|name| is_simple_identifier(name))
}

pub(super) fn is_type_declaration(kind: &str) -> bool {
    matches!(kind, "interface" | "type" | "class-signature")
}

pub(super) fn normalize_symbol_kind(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}
