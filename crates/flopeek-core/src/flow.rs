//! Bounded, framework-neutral TypeScript entry and context-flow evidence.
//!
//! This module deliberately reads only the root package manifest and projects
//! already-proven graph edges.  It never stores command text or manifest bodies,
//! and a flow is a static traversal, not an execution trace.

use crate::discovery::normalize_relative_path;
use crate::graph::node_id;
use crate::model::{
    CONTEXT_FLOW_SCHEMA, ContextFlow, ENTRY_EVIDENCE_SCHEMA, EntryEvidence, EntryManifest,
    EntryRecord, FlowEdge, FlowStep, GraphEdge, GraphNode, RELATED_TEST_EVIDENCE_SCHEMA,
    RelatedTestEvidence, RelatedTestRecord,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_ENTRY_RECORDS: usize = 1_000;
pub const MAX_FLOWS: usize = 1_000;
pub const MAX_FLOW_STEPS: usize = 256;
pub const MAX_FLOW_EDGES: usize = 512;
pub const MAX_FLOW_DEPTH: usize = 64;
pub const MAX_RELATED_TEST_RECORDS: usize = 100_000;
pub const MAX_FLOW_REFS: usize = 256;

#[derive(Debug, Clone)]
pub struct FlowDerivation {
    pub entry_evidence: EntryEvidence,
    pub related_test_evidence: RelatedTestEvidence,
    pub flows: Vec<ContextFlow>,
    pub entry_nodes: Vec<GraphNode>,
    pub entry_edges: Vec<GraphEdge>,
    pub truncated: bool,
    pub omissions: Vec<String>,
}

pub fn derive(
    root: &Path,
    project_id: &str,
    files: &[crate::model::SourceFile],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<FlowDerivation, String> {
    let (mut entry_evidence, _) = parse_manifest(root, files)?;
    let file_ids = files
        .iter()
        .map(|file| (file.path.clone(), node_id("file", &file.path, "")))
        .collect::<BTreeMap<_, _>>();
    let mut entry_nodes = Vec::new();
    let mut entry_edges = Vec::new();
    let mut global_truncated = entry_evidence.truncated;
    let mut omissions = entry_evidence.omissions.clone();
    entry_evidence
        .records
        .sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));
    entry_evidence.records.dedup();
    if entry_evidence.records.len() > MAX_ENTRY_RECORDS {
        entry_evidence.records.truncate(MAX_ENTRY_RECORDS);
        entry_evidence.truncated = true;
        entry_evidence.status = "truncated".to_string();
        entry_evidence
            .omissions
            .push(format!("entry records capped at {MAX_ENTRY_RECORDS}"));
        global_truncated = true;
        omissions.push(format!("entry records capped at {MAX_ENTRY_RECORDS}"));
    }

    for record in &mut entry_evidence.records {
        if record.status != "resolved" {
            continue;
        }
        let Some(target_path) = record.target_path.clone() else {
            record.status = "unresolved".to_string();
            record.reason = "missing-entry-target".to_string();
            continue;
        };
        let Some(target_id) = file_ids.get(&target_path).cloned() else {
            record.status = "unresolved".to_string();
            record.reason = "entry-target-not-scanned".to_string();
            continue;
        };
        record.target_node_id = Some(target_id.clone());
        let id = node_id(
            "entry",
            "package.json",
            &format!("{}:{}", record.kind, record.key),
        );
        if !entry_nodes.iter().any(|node: &GraphNode| node.id == id) {
            entry_nodes.push(GraphNode {
                id: id.clone(),
                kind: "entry".to_string(),
                path: Some("package.json".to_string()),
                name: Some(format!("{}:{}", record.kind, record.key)),
                language: None,
                evidence_fingerprint: String::new(),
            });
        }
        entry_edges.push(GraphEdge {
            from: id,
            to: target_id,
            kind: "entry-targets".to_string(),
            evidence: "package-json-entry".to_string(),
        });
    }
    entry_evidence.records.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.status.cmp(&b.status))
            .then_with(|| a.reason.cmp(&b.reason))
    });
    entry_evidence.records.dedup();
    let related_test_evidence = derive_related_tests(nodes, edges);
    if related_test_evidence.truncated {
        global_truncated = true;
        omissions.extend(related_test_evidence.omissions.clone());
    }
    let mut flows = Vec::new();
    let mut resolved_records = entry_evidence
        .records
        .iter()
        .filter(|record| record.status == "resolved" && record.target_node_id.is_some())
        .cloned()
        .collect::<Vec<_>>();
    resolved_records.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));
    if resolved_records.len() > MAX_FLOWS {
        resolved_records.truncate(MAX_FLOWS);
        global_truncated = true;
        omissions.push(format!("flows capped at {MAX_FLOWS}"));
    }
    for record in resolved_records {
        let flow_id = flow_id(project_id, &record.kind, &record.key);
        let entry_node_id = node_id(
            "entry",
            "package.json",
            &format!("{}:{}", record.kind, record.key),
        );
        let flow = project_flow(
            &flow_id,
            &record,
            &entry_node_id,
            nodes,
            &entry_edges,
            edges,
            &related_test_evidence.records,
        );
        if flow.truncated {
            global_truncated = true;
            omissions.extend(flow.omissions.clone());
        }
        flows.push(flow);
    }
    flows.sort_by(|a, b| a.flow_id.cmp(&b.flow_id));
    if entry_evidence.truncated {
        entry_evidence.status = "truncated".to_string();
    } else if entry_evidence.status != "unavailable" {
        entry_evidence.status = "complete".to_string();
    }
    omissions.sort();
    omissions.dedup();
    Ok(FlowDerivation {
        entry_evidence,
        related_test_evidence,
        flows,
        entry_nodes,
        entry_edges,
        truncated: global_truncated,
        omissions,
    })
}

fn parse_manifest(
    root: &Path,
    files: &[crate::model::SourceFile],
) -> Result<(EntryEvidence, bool), String> {
    let path = root.join("package.json");
    let Ok(bytes) = fs::read(&path) else {
        return Ok((
            EntryEvidence {
                schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
                status: "complete".to_string(),
                manifest: None,
                exact_fingerprint: blake3::hash(b"missing-package-json").to_hex().to_string(),
                effective_fingerprint: blake3::hash(b"empty-entry-manifest").to_hex().to_string(),
                records: Vec::new(),
                truncated: false,
                omissions: Vec::new(),
                limitations: vec!["root-package-json-absent-no-entry-evidence".to_string()],
            },
            false,
        ));
    };
    let manifest = EntryManifest {
        path: "package.json".to_string(),
        bytes: bytes.len() as u64,
        hash: blake3::hash(&bytes).to_hex().to_string(),
    };
    let exact_fingerprint = manifest.hash.clone();
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Ok((
            EntryEvidence {
                schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
                status: "truncated".to_string(),
                manifest: Some(manifest),
                exact_fingerprint,
                effective_fingerprint: String::new(),
                records: Vec::new(),
                truncated: true,
                omissions: vec![format!("package.json exceeds {MAX_MANIFEST_BYTES} bytes")],
                limitations: vec!["entry-manifest-byte-bound-reached".to_string()],
            },
            true,
        ));
    }
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) if value.is_object() => value,
        Ok(_) => {
            return Ok((
                unavailable_manifest(manifest, exact_fingerprint, "package-json-not-object"),
                true,
            ));
        }
        Err(_) => {
            return Ok((
                unavailable_manifest(manifest, exact_fingerprint, "package-json-invalid"),
                true,
            ));
        }
    };
    let mut records = Vec::new();
    if let Some(scripts) = value.get("scripts").and_then(Value::as_object) {
        for (key, command) in scripts {
            let command = command.as_str();
            let (runner, target, reason) = command.map(parse_script).unwrap_or((
                None,
                None,
                "script-command-not-literal".to_string(),
            ));
            records.push(entry_record("script", key, runner, target, reason, files));
        }
    }
    if let Some(bin) = value.get("bin") {
        match bin {
            Value::String(target) => records.push(entry_record(
                "bin",
                "bin",
                None,
                Some(target.to_string()),
                "".to_string(),
                files,
            )),
            Value::Object(map) => {
                for (key, target) in map {
                    records.push(entry_record(
                        "bin",
                        key,
                        None,
                        target.as_str().map(ToOwned::to_owned),
                        if target.is_string() {
                            String::new()
                        } else {
                            "bin-target-not-string".to_string()
                        },
                        files,
                    ));
                }
            }
            _ => records.push(entry_record(
                "bin",
                "bin",
                None,
                None,
                "bin-not-string-or-object".to_string(),
                files,
            )),
        }
    }
    for field in ["main", "module"] {
        if let Some(value) = value.get(field) {
            records.push(entry_record(
                field,
                field,
                None,
                value.as_str().map(ToOwned::to_owned),
                if value.is_string() {
                    String::new()
                } else {
                    "entry-target-not-string".to_string()
                },
                files,
            ));
        }
    }
    records.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));
    let effective = records
        .iter()
        .map(|record| {
            (
                &record.kind,
                &record.key,
                &record.runner,
                &record.target_path,
                &record.status,
                &record.reason,
            )
        })
        .collect::<Vec<_>>();
    let effective_fingerprint = blake3::hash(&serde_json::to_vec(&effective).unwrap_or_default())
        .to_hex()
        .to_string();
    Ok((
        EntryEvidence {
            schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
            status: "complete".to_string(),
            manifest: Some(manifest),
            exact_fingerprint,
            effective_fingerprint,
            records,
            truncated: false,
            omissions: Vec::new(),
            limitations: vec![
                "only-root-package-json-is-considered".to_string(),
                "package-exports-and-package-manager-wrappers-unsupported".to_string(),
            ],
        },
        true,
    ))
}

fn unavailable_manifest(
    manifest: EntryManifest,
    exact_fingerprint: String,
    reason: &str,
) -> EntryEvidence {
    EntryEvidence {
        schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
        status: "unavailable".to_string(),
        manifest: Some(manifest),
        exact_fingerprint,
        effective_fingerprint: String::new(),
        records: Vec::new(),
        truncated: false,
        omissions: vec![reason.to_string()],
        limitations: vec!["entry-manifest-cannot-be-parsed".to_string()],
    }
}

fn parse_script(command: &str) -> (Option<String>, Option<String>, String) {
    if command.is_empty()
        || command
            .chars()
            .any(|c| matches!(c, '&' | '|' | ';' | '>' | '<' | '`' | '$' | '\n' | '\r'))
    {
        return (
            None,
            None,
            "script-command-complex-or-shell-composed".to_string(),
        );
    }
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    if tokens
        .iter()
        .any(|token| token.starts_with('-') || token.contains('"') || token.contains('\''))
    {
        return (
            None,
            None,
            "script-command-flags-or-quoting-unsupported".to_string(),
        );
    }
    let (runner, target) = match tokens.as_slice() {
        [runner, target]
            if matches!(*runner, "tsx" | "ts-node" | "ts-node-esm" | "node" | "bun") =>
        {
            ((*runner).to_string(), *target)
        }
        ["bun", "run", target] | ["deno", "run", target] => (format!("{} run", tokens[0]), *target),
        _ => return (None, None, "unsupported-script-runner-or-arity".to_string()),
    };
    (Some(runner), Some(target.to_string()), String::new())
}

fn entry_record(
    kind: &str,
    key: &str,
    runner: Option<String>,
    target: Option<String>,
    reason: String,
    files: &[crate::model::SourceFile],
) -> EntryRecord {
    let mut record = EntryRecord {
        key: key.to_string(),
        kind: kind.to_string(),
        runner,
        target_path: None,
        target_node_id: None,
        status: "unresolved".to_string(),
        reason,
    };
    if record.reason.is_empty() {
        let Some(target) = target else {
            record.reason = "entry-target-missing".to_string();
            return record;
        };
        match resolve_target(&target, files) {
            Ok(path) => {
                record.target_path = Some(path);
                record.status = "resolved".to_string();
                record.reason = "known-typescript-target".to_string();
            }
            Err(reason) => record.reason = reason,
        }
    }
    record
}

fn resolve_target(target: &str, files: &[crate::model::SourceFile]) -> Result<String, String> {
    if target.is_empty()
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.contains(':')
    {
        return Err("entry-target-absolute-or-invalid".to_string());
    }
    let target = target.replace('\\', "/");
    let path = PathBuf::from(&target);
    let normalized = normalize_relative_path(&path)
        .map_err(|_| "entry-target-escapes-repository".to_string())?;
    let known = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let extension = Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension == "js" || extension == "jsx" || normalized.ends_with(".d.ts") {
        return Err("entry-target-javascript-or-declaration-output-unsupported".to_string());
    }
    let mut candidates = Vec::new();
    if known.contains(normalized.as_str()) {
        candidates.push(normalized.clone());
    } else if extension.is_empty() {
        for suffix in [".ts", ".tsx"] {
            candidates.push(format!("{normalized}{suffix}"));
        }
        candidates.push(format!("{normalized}/index.ts"));
        candidates.push(format!("{normalized}/index.tsx"));
    } else {
        candidates.push(normalized.clone());
    }
    if let Some(candidate) = candidates
        .iter()
        .find(|candidate| known.contains(candidate.as_str()))
    {
        return Ok(candidate.clone());
    }
    let declaration_candidates = [
        format!("{normalized}.d.ts"),
        format!("{normalized}/index.d.ts"),
    ];
    if declaration_candidates
        .iter()
        .any(|candidate| known.contains(candidate.as_str()))
    {
        return Err("entry-target-declaration-file-unsupported".to_string());
    }
    Err("entry-target-missing-or-not-typescript".to_string())
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    file.ends_with(".test.ts")
        || file.ends_with(".test.tsx")
        || file.ends_with(".spec.ts")
        || file.ends_with(".spec.tsx")
        || lower
            .split('/')
            .any(|part| matches!(part, "test" | "tests" | "__tests__"))
}

fn derive_related_tests(nodes: &[GraphNode], edges: &[GraphEdge]) -> RelatedTestEvidence {
    let mut records = Vec::new();
    for edge in edges {
        if !matches!(edge.kind.as_str(), "calls" | "constructs" | "imports") {
            continue;
        }
        let Some(test_node) = nodes.iter().find(|node| node.id == edge.from) else {
            continue;
        };
        let Some(target_node) = nodes.iter().find(|node| node.id == edge.to) else {
            continue;
        };
        let Some(test_path) = test_node.path.as_deref() else {
            continue;
        };
        let Some(target_path) = target_node.path.as_deref() else {
            continue;
        };
        if !is_test_path(test_path) || is_test_path(target_path) {
            continue;
        }
        let relation = match edge.kind.as_str() {
            "calls" => "direct-call",
            "constructs" => "direct-construct",
            _ => "direct-import",
        };
        let strength = if relation == "direct-import" {
            "weak"
        } else {
            "strong"
        };
        if relation == "direct-import" && edge.evidence.contains("type") {
            continue;
        }
        records.push(RelatedTestRecord {
            test_path: test_path.to_string(),
            test_node_id: test_node.id.clone(),
            target_node_id: target_node.id.clone(),
            relation: relation.to_string(),
            strength: strength.to_string(),
            status: "proven".to_string(),
            reason: edge.evidence.clone(),
        });
    }
    records.sort_by(|a, b| {
        a.test_path
            .cmp(&b.test_path)
            .then_with(|| a.test_node_id.cmp(&b.test_node_id))
            .then_with(|| a.target_node_id.cmp(&b.target_node_id))
            .then_with(|| a.relation.cmp(&b.relation))
    });
    records.dedup();
    let mut truncated = false;
    let mut omissions = Vec::new();
    if records.len() > MAX_RELATED_TEST_RECORDS {
        records.truncate(MAX_RELATED_TEST_RECORDS);
        truncated = true;
        omissions.push(format!(
            "related-test records capped at {MAX_RELATED_TEST_RECORDS}"
        ));
    }
    RelatedTestEvidence {
        schema_version: RELATED_TEST_EVIDENCE_SCHEMA.to_string(),
        status: if truncated { "truncated" } else { "complete" }.to_string(),
        records,
        truncated,
        omissions,
    }
}

fn project_flow(
    flow_id: &str,
    entry: &EntryRecord,
    entry_node_id: &str,
    nodes: &[GraphNode],
    entry_edges: &[GraphEdge],
    edges: &[GraphEdge],
    related: &[RelatedTestRecord],
) -> ContextFlow {
    let target = entry.target_node_id.clone().unwrap_or_default();
    let node_by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = BTreeMap::<String, Vec<&GraphEdge>>::new();
    for edge in edges
        .iter()
        .filter(|edge| matches!(edge.kind.as_str(), "calls" | "constructs"))
    {
        adjacency.entry(edge.from.clone()).or_default().push(edge);
    }
    for values in adjacency.values_mut() {
        values.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| a.evidence.cmp(&b.evidence))
        });
    }
    let mut steps = Vec::new();
    let mut traversed = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    let synthetic_entry;
    let entry_ref = if let Some(node) = nodes.iter().find(|node| node.id == entry_node_id) {
        node
    } else {
        synthetic_entry = GraphNode {
            id: entry_node_id.to_string(),
            kind: "entry".to_string(),
            path: Some("package.json".to_string()),
            name: Some(format!("{}:{}", entry.kind, entry.key)),
            language: None,
            evidence_fingerprint: String::new(),
        };
        &synthetic_entry
    };
    visited.insert(entry_ref.id.clone());
    steps.push(step(0, entry_ref, "entry"));
    if !target.is_empty() {
        queue.push_back((target.clone(), 0usize));
    }
    let mut truncated = false;
    let mut omissions = Vec::new();
    while let Some((current, depth)) = queue.pop_front() {
        if visited.contains(&current) {
            continue;
        }
        let Some(node) = node_by_id.get(&current).copied() else {
            continue;
        };
        if steps.len() >= MAX_FLOW_STEPS {
            truncated = true;
            omissions.push(format!("flow steps capped at {MAX_FLOW_STEPS}"));
            break;
        }
        visited.insert(current.clone());
        steps.push(step(
            steps.len(),
            node,
            if current == target {
                "target"
            } else {
                "callee"
            },
        ));
        if depth >= MAX_FLOW_DEPTH {
            truncated = true;
            omissions.push(format!("flow depth capped at {MAX_FLOW_DEPTH}"));
            continue;
        }
        for edge in adjacency.get(&current).into_iter().flatten() {
            if traversed.len() >= MAX_FLOW_EDGES {
                truncated = true;
                omissions.push(format!("flow edges capped at {MAX_FLOW_EDGES}"));
                break;
            }
            traversed.push(FlowEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind: edge.kind.clone(),
                evidence: edge.evidence.clone(),
            });
            if !visited.contains(&edge.to) {
                queue.push_back((edge.to.clone(), depth + 1));
            }
        }
        if traversed.len() >= MAX_FLOW_EDGES {
            break;
        }
    }
    let step_ids = steps
        .iter()
        .map(|step| step.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let related_tests = related
        .iter()
        .filter(|record| {
            step_ids.contains(record.target_node_id.as_str()) || record.target_node_id == target
        })
        .cloned()
        .collect::<Vec<_>>();
    let fingerprint_input = (
        entry,
        &entry.kind,
        &entry.key,
        &entry.target_path,
        &steps,
        &traversed,
        &related_tests,
        truncated,
        &omissions,
    );
    let fingerprint = blake3::hash(&serde_json::to_vec(&fingerprint_input).unwrap_or_default())
        .to_hex()
        .to_string();
    omissions.sort();
    omissions.dedup();
    let status = if truncated { "truncated" } else { "complete" };
    let _ = entry_edges;
    ContextFlow {
        schema_version: CONTEXT_FLOW_SCHEMA.to_string(),
        flow_id: flow_id.to_string(),
        entry_node_id: entry_node_id.to_string(),
        entry_kind: entry.kind.clone(),
        entry_key: entry.key.clone(),
        steps,
        traversed_edges: traversed,
        related_tests,
        fingerprint,
        status: status.to_string(),
        truncated,
        omissions,
        limitations: vec![
            "static-traversal-not-execution-order".to_string(),
            "dynamic-dispatch-and-runtime-behavior-unsupported".to_string(),
        ],
    }
}

fn step(index: usize, node: &GraphNode, role: &str) -> FlowStep {
    FlowStep {
        index,
        node_id: node.id.clone(),
        path: node.path.clone(),
        name: node.name.clone(),
        role: role.to_string(),
        evidence_fingerprint: node.evidence_fingerprint.clone(),
    }
}

pub fn flow_id(project_id: &str, kind: &str, key: &str) -> String {
    format!(
        "flow_{}",
        blake3::hash(format!("flopeek-flow-v1\0{project_id}\0{kind}\0{key}").as_bytes()).to_hex()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GraphEdge, GraphNode, SourceFile};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("flopeek-flow-{suffix}"))
    }

    #[test]
    fn package_entries_and_static_bfs_are_deterministic_without_command_body() {
        let root = temp_root();
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"start":"tsx src/main"},"main":"src/main.ts"}"#,
        )
        .expect("manifest");
        let files = vec![SourceFile {
            path: "src/main.ts".to_string(),
            language: "typescript".to_string(),
            bytes: 1,
            hash: "source".to_string(),
        }];
        let file = node_id("file", "src/main.ts", "");
        let callee = node_id("symbol", "src/main.ts", "function:main");
        let nodes = vec![
            GraphNode {
                id: file.clone(),
                kind: "file".to_string(),
                path: Some("src/main.ts".to_string()),
                name: None,
                language: Some("typescript".to_string()),
                evidence_fingerprint: "file-fp".to_string(),
            },
            GraphNode {
                id: callee.clone(),
                kind: "function".to_string(),
                path: Some("src/main.ts".to_string()),
                name: Some("main".to_string()),
                language: Some("typescript".to_string()),
                evidence_fingerprint: "callee-fp".to_string(),
            },
        ];
        let edges = vec![GraphEdge {
            from: file.clone(),
            to: callee.clone(),
            kind: "calls".to_string(),
            evidence: "direct".to_string(),
        }];
        let first = derive(&root, "project_test", &files, &nodes, &edges).expect("derive");
        let second = derive(&root, "project_test", &files, &nodes, &edges).expect("derive again");
        assert_eq!(
            first.entry_evidence.effective_fingerprint,
            second.entry_evidence.effective_fingerprint
        );
        assert_eq!(first.flows, second.flows);
        assert!(
            first
                .entry_evidence
                .records
                .iter()
                .all(|record| record.reason != "script-command-body-stored")
        );
        assert!(first.flows.iter().all(|flow| {
            flow.traversed_edges
                .iter()
                .all(|edge| edge.kind == "calls" || edge.kind == "constructs")
        }));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unsupported_manifest_is_explicitly_unavailable() {
        let root = temp_root();
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("package.json"), "{ invalid").expect("manifest");
        let result = derive(&root, "project_test", &[], &[], &[]).expect("derive");
        assert_eq!(result.entry_evidence.status, "unavailable");
        assert!(
            result
                .entry_evidence
                .omissions
                .iter()
                .any(|reason| reason == "package-json-invalid")
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
