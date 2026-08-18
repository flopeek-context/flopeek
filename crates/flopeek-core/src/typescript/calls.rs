//! Call-site and canonical syntax extraction.

#[allow(unused_imports)]
use super::*;

pub(super) fn collect_calls(root: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptCall>) {
    fn visit(node: Node<'_>, root: Node<'_>, source: &[u8], output: &mut Vec<TypeScriptCall>) {
        if node.kind() == "new_expression" {
            let constructor = node
                .child_by_field_name("constructor")
                .and_then(|constructor| constructor_syntax(constructor, source));
            let caller = enclosing_owner(node, root, source);
            let shadowed = constructor.as_ref().is_some_and(|syntax| {
                syntax
                    .callee
                    .as_deref()
                    .is_some_and(|name| is_shadowed_in_owner(node, root, source, name))
            });
            output.push(TypeScriptCall {
                callee: constructor
                    .as_ref()
                    .and_then(|syntax| syntax.callee.clone()),
                dynamic: constructor.is_none(),
                position: position(node),
                caller,
                callee_form: constructor.as_ref().map_or_else(
                    || "dynamic-constructor".to_string(),
                    |syntax| syntax.form.clone(),
                ),
                receiver: None,
                shadowed,
                enclosing_type: enclosing_type(node, root, source),
            });
        }
        if node.kind() != "call_expression" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                visit(child, root, source, output);
            }
            return;
        }
        let function = node.child_by_field_name("function");
        let syntax = function.and_then(|function| callee_syntax(function, source));
        let caller = enclosing_owner(node, root, source);
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
            enclosing_type: enclosing_type(node, root, source),
        });
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, root, source, output);
        }
    }
    visit(root, root, source, output);
}

#[derive(Debug, Clone)]
pub(super) struct CalleeSyntax {
    callee: Option<String>,
    form: String,
    receiver: Option<String>,
}

pub(super) fn callee_syntax(node: Node<'_>, source: &[u8]) -> Option<CalleeSyntax> {
    match node.kind() {
        "identifier" | "type_identifier" => node_text(node, source).map(|callee| CalleeSyntax {
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
                    if (is_simple_identifier(&receiver) || receiver == "this")
                        && (is_simple_identifier(&property)
                            || (property.starts_with('#')
                                && is_simple_identifier(property.trim_start_matches('#'))))
                        && node_text(node, source)
                            .is_some_and(|text| text == format!("{receiver}.{property}")) =>
                {
                    Some(CalleeSyntax {
                        callee: Some(format!("{receiver}.{property}")),
                        form: if receiver == "this" {
                            "this-member".to_string()
                        } else {
                            "member".to_string()
                        },
                        receiver: Some(receiver),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn constructor_syntax(node: Node<'_>, source: &[u8]) -> Option<CalleeSyntax> {
    let callee = node_text(node, source)?;
    if !is_simple_identifier(&callee) {
        return None;
    }
    Some(CalleeSyntax {
        callee: Some(callee),
        form: "constructor".to_string(),
        receiver: None,
    })
}

pub(super) fn enclosing_owner(node: Node<'_>, root: Node<'_>, source: &[u8]) -> Option<String> {
    let mut current = node;
    let mut method = None;
    loop {
        if matches!(
            current.kind(),
            "method_definition" | "method_signature" | "abstract_method_signature"
        ) {
            method = Some(current);
        }
        let parent = current.parent()?;
        if let Some(owner_kind) = type_declaration_kind(parent)
            && let Some(owner_name) = type_owner_name(parent, root, source)
        {
            if let Some(method) = method {
                let Some(name_node) = method.child_by_field_name("name") else {
                    return Some(format!("{owner_kind}:{owner_name}"));
                };
                let Some(name) = member_name(name_node, source) else {
                    return Some(format!("{owner_kind}:{owner_name}"));
                };
                let text = node_text(method, source).unwrap_or_default();
                let words = modifier_words(
                    &text,
                    name_node.start_byte().saturating_sub(method.start_byte()),
                );
                let kind = if owner_kind == "class" && name == "constructor" {
                    "constructor"
                } else if owner_kind == "interface" {
                    "method-signature"
                } else if words.iter().any(|word| word == "static") {
                    "static-method"
                } else {
                    "method"
                };
                return Some(format!(
                    "{}:{owner_name}.{name}",
                    normalize_symbol_kind(kind)
                ));
            }
            return Some(format!("{owner_kind}:{owner_name}"));
        }
        if parent.id() == root.id() {
            break;
        }
        current = parent;
    }
    enclosing_top_level_owner(node, root, source)
}

pub(super) fn enclosing_top_level_owner(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
) -> Option<String> {
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

pub(super) fn enclosing_type(node: Node<'_>, root: Node<'_>, source: &[u8]) -> Option<String> {
    let mut current = node;
    loop {
        let parent = current.parent()?;
        if type_declaration_kind(parent).is_some() {
            return type_owner_name(parent, root, source);
        }
        if parent.id() == root.id() {
            return None;
        }
        current = parent;
    }
}

pub(super) fn type_owner_name(node: Node<'_>, root: Node<'_>, source: &[u8]) -> Option<String> {
    declaration_name(node, source).or_else(|| {
        let top = top_level_child(node, root)?;
        (top.kind() == "export_statement"
            && node_text(top, source).is_some_and(|text| text.starts_with("export default")))
        .then(|| "default".to_string())
    })
}

pub(super) fn top_level_child<'a>(node: Node<'a>, root: Node<'a>) -> Option<Node<'a>> {
    let mut current = node;
    loop {
        let parent = current.parent()?;
        if parent.id() == root.id() {
            return Some(current);
        }
        current = parent;
    }
}

pub(super) fn is_shadowed_in_owner(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    name: &str,
) -> bool {
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

pub(super) fn binding_names(node: Node<'_>, source: &[u8]) -> BTreeSet<String> {
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

pub(super) fn contains_range(outer: tree_sitter::Range, inner: tree_sitter::Range) -> bool {
    outer.start_byte <= inner.start_byte && outer.end_byte >= inner.end_byte
}

pub(super) fn exported_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "export_clause" && child.kind() != "namespace_export")
}

pub(super) fn named_child_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub(super) fn canonical_ast(node: Node<'_>, source: &[u8]) -> String {
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

pub(super) fn canonical_type_header(node: Node<'_>, source: &[u8]) -> String {
    let body_id = node.child_by_field_name("body").map(|body| body.id());
    let mut output = String::new();
    output.push('(');
    output.push_str(node.kind());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if Some(child.id()) == body_id || child.kind() == "comment" {
            continue;
        }
        output.push_str(&canonical_ast(child, source));
    }
    output.push(')');
    output
}

pub(super) fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().enumerate().all(|(index, character)| {
            character == '_'
                || character == '$'
                || character.is_ascii_alphanumeric() && (index > 0 || !character.is_ascii_digit())
        })
}

pub(super) fn string_field(node: Node<'_>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field).and_then(|child| {
        node_text(child, source).and_then(|value| {
            value
                .strip_prefix(['\'', '"', '`'])
                .and_then(|value| value.strip_suffix(['\'', '"', '`']))
                .map(ToOwned::to_owned)
        })
    })
}

pub(super) fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(ToOwned::to_owned)
}

pub(super) fn position(node: Node<'_>) -> SourcePosition {
    let start = node.start_position();
    let end = node.end_position();
    SourcePosition {
        start_line: start.row + 1,
        start_column: start.column,
        end_line: end.row + 1,
        end_column: end.column,
    }
}
