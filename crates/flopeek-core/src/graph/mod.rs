//! Deterministic graph assembly for TypeScript/TSX evidence.

use crate::discovery::{discover, read_source};
use crate::model::{
    GRAPH_SCHEMA, GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, SourceFile,
    TypeScriptFacts,
};
use crate::typescript;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_GRAPH_NODES: usize = 50_000;
pub const MAX_GRAPH_EDGES: usize = 100_000;

pub fn build(root: &Path) -> Result<(GraphSnapshot, Vec<TypeScriptFacts>), String> {
    let mut files = discover(root)?;
    let mut truncated = false;
    let mut omissions = Vec::new();
    if files.len() > MAX_SOURCE_FILES {
        files.truncate(MAX_SOURCE_FILES);
        truncated = true;
        omissions.push(format!("source files capped at {MAX_SOURCE_FILES}"));
    }
    let mut facts = Vec::with_capacity(files.len());
    for file in &files {
        let source = read_source(root, &file.path)?;
        facts.push(typescript::parse(&file.path, &source, &file.hash)?);
    }
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut file_ids = BTreeMap::new();
    let mut declaration_ids = BTreeMap::<String, Vec<String>>::new();
    let known_paths = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();

    for file in &files {
        let id = node_id("file", &file.path, "");
        file_ids.insert(file.path.clone(), id.clone());
        nodes.push(GraphNode {
            id,
            kind: "file".to_string(),
            path: Some(file.path.clone()),
            name: None,
            language: Some(file.language.clone()),
            evidence_fingerprint: String::new(),
        });
    }
    for fact in &facts {
        let file_id = file_ids
            .get(&fact.path)
            .ok_or_else(|| format!("Missing file node for {}", fact.path))?;
        for declaration in &fact.declarations {
            let normalized_kind = normalize_symbol_kind(&declaration.kind);
            let qualified_name = format!("{normalized_kind}:{}", declaration.name);
            let id = node_id("symbol", &fact.path, &qualified_name);
            if !nodes.iter().any(|node| node.id == id) {
                nodes.push(GraphNode {
                    id: id.clone(),
                    kind: normalized_kind,
                    path: Some(fact.path.clone()),
                    name: Some(declaration.name.clone()),
                    language: Some(fact.language.clone()),
                    evidence_fingerprint: String::new(),
                });
            }
            let ids = declaration_ids.entry(declaration.name.clone()).or_default();
            if !ids.contains(&id) {
                ids.push(id.clone());
            }
            edges.push(GraphEdge {
                from: file_id.clone(),
                to: id,
                kind: "declares".to_string(),
                evidence: "top-level-typescript-declaration".to_string(),
            });
        }
    }
    for fact in &facts {
        let file_id = file_ids
            .get(&fact.path)
            .ok_or_else(|| format!("Missing file node for {}", fact.path))?;
        for import in &fact.imports {
            if let Some(target_path) = resolve_relative(&fact.path, &import.specifier, &known_paths)
            {
                let target_id = file_ids
                    .get(&target_path)
                    .ok_or_else(|| format!("Missing imported file node for {target_path}"))?;
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id.clone(),
                    kind: "imports".to_string(),
                    evidence: import.kind.clone(),
                });
            } else {
                let target_id = node_id("external-module", &import.specifier, "");
                if !nodes.iter().any(|node| node.id == target_id) {
                    nodes.push(GraphNode {
                        id: target_id.clone(),
                        kind: "external-module".to_string(),
                        path: None,
                        name: Some(import.specifier.clone()),
                        language: None,
                        evidence_fingerprint: String::new(),
                    });
                }
                edges.push(GraphEdge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: "imports-external".to_string(),
                    evidence: import.kind.clone(),
                });
            }
        }
        for call in &fact.calls {
            let Some(callee) = call.callee.as_deref() else {
                continue;
            };
            if call.dynamic {
                continue;
            }
            let Some(targets) = declaration_ids.get(callee) else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            edges.push(GraphEdge {
                from: file_id.clone(),
                to: targets[0].clone(),
                kind: "calls".to_string(),
                evidence: "unique-direct-callee-name".to_string(),
            });
        }
    }
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    edges.dedup();
    assign_node_fingerprints(&mut nodes, &edges, &facts);
    if nodes.len() > MAX_GRAPH_NODES {
        nodes.truncate(MAX_GRAPH_NODES);
        truncated = true;
        omissions.push(format!("graph nodes capped at {MAX_GRAPH_NODES}"));
    }
    if edges.len() > MAX_GRAPH_EDGES {
        edges.truncate(MAX_GRAPH_EDGES);
        truncated = true;
        omissions.push(format!("graph edges capped at {MAX_GRAPH_EDGES}"));
    }

    let project_id = project_id(root);
    let source_revision = source_revision(root);
    let source_fingerprint = exact_source_fingerprint(&files)?;
    let identity_files = files
        .iter()
        .map(|file| (&file.path, &file.language))
        .collect::<Vec<_>>();
    let identity = GraphIdentity {
        project_id: &project_id,
        derivation_id: "typescript-structural-evidence-v2",
        files: &identity_files,
        nodes: &nodes,
        edges: &edges,
    };
    let graph_id = blake3::hash(&serde_json::to_vec(&identity).map_err(|error| error.to_string())?)
        .to_hex()
        .to_string();
    Ok((
        GraphSnapshot {
            schema_version: GRAPH_SCHEMA.to_string(),
            product: PRODUCT_IDENTITY.to_string(),
            project_id,
            graph_id,
            graph_version: 0,
            source_revision,
            source_fingerprint,
            observation_id: String::new(),
            files,
            nodes,
            edges,
            truncated,
            omissions,
        },
        facts,
    ))
}

#[derive(Serialize)]
struct GraphIdentity<'a> {
    project_id: &'a str,
    derivation_id: &'a str,
    files: &'a Vec<(&'a String, &'a String)>,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
}

fn exact_source_fingerprint(files: &[SourceFile]) -> Result<String, String> {
    let canonical = files
        .iter()
        .map(|file| (&file.path, &file.language, file.bytes, &file.hash))
        .collect::<Vec<_>>();
    serde_json::to_vec(&canonical)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|error| format!("Unable to derive source fingerprint: {error}"))
}

fn normalize_symbol_kind(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn assign_node_fingerprints(
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
            let exported = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .and_then(|fact| {
                    fact.declarations.iter().find(|declaration| {
                        declaration.name == node.name.as_deref().unwrap_or_default()
                            && normalize_symbol_kind(&declaration.kind) == node.kind
                    })
                })
                .is_some_and(|declaration| declaration.exported);
            let mut declaration_fingerprints = node
                .path
                .as_deref()
                .and_then(|path| facts.iter().find(|fact| fact.path == path))
                .and_then(|fact| {
                    Some(
                        fact.declarations
                            .iter()
                            .filter(|declaration| {
                                declaration.name == node.name.as_deref().unwrap_or_default()
                                    && normalize_symbol_kind(&declaration.kind) == node.kind
                            })
                            .map(|declaration| {
                                format!("{}:{}", declaration.ast_fingerprint, declaration.exported)
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap_or_default();
            declaration_fingerprints.sort();
            let declaration_fingerprint = declaration_fingerprints.join("|");
            format!(
                "symbol\0{}\0{}\0{}\0exported={exported}\0ast={declaration_fingerprint}",
                node.kind,
                node.path.as_deref().unwrap_or_default(),
                node.name.as_deref().unwrap_or_default()
            )
        };
        intrinsic.insert(node.id.clone(), value);
    }
    for node in nodes {
        let mut signatures = edges
            .iter()
            .filter_map(|edge| {
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
        let mut canonical = intrinsic.remove(&node.id).unwrap_or_default();
        for signature in signatures {
            canonical.push('\0');
            canonical.push_str(&signature);
        }
        node.evidence_fingerprint = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    }
}

fn canonical_fact(fact: &TypeScriptFacts) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        fact.language, fact.parser, fact.parse_status, fact.canonical_fingerprint
    )
}

pub fn node_id(kind: &str, path: &str, name: &str) -> String {
    let input = format!("flopeek-node-v1\0{kind}\0{path}\0{name}");
    format!("node_{}", blake3::hash(input.as_bytes()).to_hex())
}

pub fn project_id(root: &Path) -> String {
    let identity = root
        .canonicalize()
        .ok()
        .and_then(|path| path.to_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    format!(
        "project_{}",
        blake3::hash(format!("flopeek-project-v1\0{identity}").as_bytes()).to_hex()
    )
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
    if !specifier.starts_with('.') {
        return None;
    }
    let current = Path::new(current);
    let parent = current.parent().unwrap_or_else(|| Path::new(""));
    let joined = parent.join(specifier);
    let candidates = [
        joined.clone(),
        PathBuf::from(format!("{}.ts", joined.to_string_lossy())),
        PathBuf::from(format!("{}.tsx", joined.to_string_lossy())),
        joined.join("index.ts"),
        joined.join("index.tsx"),
    ];
    candidates.iter().find_map(|candidate| {
        let mut components = Vec::new();
        for component in candidate.components() {
            let text = component.as_os_str().to_str()?;
            if text == ".." {
                return None;
            }
            if text != "." {
                components.push(text);
            }
        }
        let normalized = components.join("/");
        known_paths.contains(&normalized).then_some(normalized)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_only_relative_typescript_modules() {
        let paths = ["src/a.ts", "src/lib.ts", "src/index.tsx"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            resolve_relative("src/a.ts", "./lib", &paths),
            Some("src/lib.ts".to_string())
        );
        assert_eq!(resolve_relative("src/a.ts", "react", &paths), None);
    }

    #[test]
    fn graph_identity_is_reproducible() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flopeek-graph-{suffix}"));
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(root.join("src/a.ts"), "export const a = 1;").expect("write");
        let (first, _) = build(&root).expect("first graph");
        let (second, _) = build(&root).expect("second graph");
        assert_eq!(first.graph_id, second.graph_id);
        assert_eq!(first.nodes, second.nodes);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn structural_graph_ignores_comments_and_whitespace_but_tracks_exact_source() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flopeek-graph-freshness-{suffix}"));
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("write");
        let first = build(&root).expect("first graph").0;
        fs::write(
            root.join("src/a.ts"),
            "// comment\n\n export   const a = 1;\n",
        )
        .expect("format");
        let second = build(&root).expect("second graph").0;
        assert_eq!(first.graph_id, second.graph_id);
        assert_ne!(first.source_fingerprint, second.source_fingerprint);
        assert_eq!(first.nodes, second.nodes);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
