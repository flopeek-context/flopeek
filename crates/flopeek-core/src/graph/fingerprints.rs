//! Canonical graph fingerprints and stable identities.

use super::*;

pub fn assign_node_fingerprints(
    nodes: &mut [GraphNode],
    edges: &[GraphEdge],
    facts: &[TypeScriptFacts],
) {
    let mut intrinsic = BTreeMap::<String, String>::new();
    for node in nodes.iter() {
        let value = if node.kind == "file" {
            node.path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .map(canonical_fact)
                .unwrap_or_default()
        } else {
            let declaration_fingerprints = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .map(|fact| {
                    fact.declarations
                        .iter()
                        .filter(|declaration| {
                            node.path.as_deref().is_some_and(|path| {
                                node.id == node_id("symbol", path, &declaration.qualified_name)
                            })
                        })
                        .map(|declaration| {
                            format!(
                                "{}:{}:{}:{}:{}",
                                declaration.ast_fingerprint,
                                declaration.exported,
                                declaration.visibility,
                                declaration.static_member,
                                declaration.abstract_member
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let exported = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .is_some_and(|fact| {
                    fact.declarations.iter().any(|declaration| {
                        node.path.as_deref().is_some_and(|path| {
                            node.id == node_id("symbol", path, &declaration.qualified_name)
                        }) && declaration.exported
                    })
                });
            let mut declaration_fingerprints = declaration_fingerprints;
            declaration_fingerprints.sort();
            format!(
                "symbol\0{}\0exported={exported}\0ast={}",
                node.kind,
                declaration_fingerprints.join("|")
            )
        };
        intrinsic.insert(node.id.clone(), value);
    }
    for node in nodes {
        let mut canonical = intrinsic.remove(&node.id).unwrap_or_default();
        if let Some(path) = node.path.as_deref()
            && let Some(fact) = facts.iter().find(|fact| fact.path == path)
        {
            for record in &fact.resolution_records {
                if node.kind == "file" || record.caller_node_id == node.id {
                    canonical.push('\0');
                    canonical.push_str(&serde_json::to_string(record).unwrap_or_default());
                }
            }
        }
        let mut signatures = edges
            .iter()
            .filter_map(|edge| {
                if edge.kind == "declares" {
                    return None;
                }
                if edge.from == node.id {
                    Some(format!(
                        "out\0{}\0{}\0{}",
                        edge.kind, edge.evidence, edge.to
                    ))
                } else if edge.to == node.id {
                    Some(format!(
                        "in\0{}\0{}\0{}",
                        edge.kind, edge.evidence, edge.from
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        signatures.sort();
        for signature in signatures {
            canonical.push('\0');
            canonical.push_str(&signature);
        }
        node.evidence_fingerprint = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    }
}

fn canonical_fact(fact: &TypeScriptFacts) -> String {
    let resolutions = fact
        .resolution_records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap_or_default())
        .collect::<Vec<_>>();
    format!(
        "{}\0{}\0{}\0{}\0{}",
        fact.language,
        fact.parser,
        fact.parse_status,
        fact.canonical_fingerprint,
        resolutions.join("|")
    )
}

pub fn node_id(kind: &str, path: &str, name: &str) -> String {
    let input = format!("flopeek-node-v1\0{kind}\0{path}\0{name}");
    format!("node_{}", blake3::hash(input.as_bytes()).to_hex())
}

pub fn project_id(root: &Path) -> String {
    crate::identity::resolve(root)
        .map(|identity| identity.project_id)
        .unwrap_or_else(|_| crate::identity::checkout_id(root))
}

pub fn source_revision(root: &Path) -> String {
    let head = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .output();
    let Ok(head) = head else {
        return "unavailable".to_string();
    };
    if !head.status.success() {
        return "unavailable".to_string();
    }
    let revision = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if revision.is_empty() {
        return "unavailable".to_string();
    }
    let dirty = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    let path = line.get(3..).unwrap_or_default().replace('\\', "/");
                    !(path == ".flopeek" || path.starts_with(".flopeek/"))
                })
        });
    if dirty {
        format!("{revision}+dirty")
    } else {
        revision
    }
}

pub fn resolve_relative(
    current: &str,
    specifier: &str,
    known_paths: &BTreeSet<String>,
) -> Option<String> {
    resolve_module_relative(current, specifier, known_paths)
}

pub fn is_non_relative_alias(specifier: &str, known_paths: &BTreeSet<String>) -> bool {
    if specifier.starts_with("@/")
        || specifier.starts_with("~/")
        || specifier.starts_with('#')
        || specifier.starts_with('/')
        || specifier.contains('\\')
    {
        return true;
    }
    let candidates = [
        specifier.to_string(),
        format!("{specifier}.ts"),
        format!("{specifier}.tsx"),
        format!("{specifier}.d.ts"),
        format!("{specifier}/index.ts"),
        format!("{specifier}/index.tsx"),
        format!("{specifier}/index.d.ts"),
    ];
    candidates
        .iter()
        .any(|candidate| known_paths.contains(candidate))
}

pub fn exact_source_fingerprint(files: &[SourceFile]) -> Result<String, String> {
    let canonical = files
        .iter()
        .map(|file| (&file.path, &file.language, file.bytes, &file.hash))
        .collect::<Vec<_>>();
    serde_json::to_vec(&canonical)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|error| format!("Unable to derive source fingerprint: {error}"))
}

pub fn normalize_symbol_kind(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}
