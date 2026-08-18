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
        });
    }
    for fact in &facts {
        let file_id = file_ids
            .get(&fact.path)
            .ok_or_else(|| format!("Missing file node for {}", fact.path))?;
        for declaration in &fact.declarations {
            let id = node_id("symbol", &fact.path, &declaration.name);
            nodes.push(GraphNode {
                id: id.clone(),
                kind: declaration.kind.clone(),
                path: Some(fact.path.clone()),
                name: Some(declaration.name.clone()),
                language: Some(fact.language.clone()),
            });
            declaration_ids
                .entry(declaration.name.clone())
                .or_default()
                .push(id.clone());
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
    let identity = GraphIdentity {
        project_id: &project_id,
        source_revision: &source_revision,
        files: &files,
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
    source_revision: &'a str,
    files: &'a [SourceFile],
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
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
            "--untracked-files=no",
        ])
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
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
}
