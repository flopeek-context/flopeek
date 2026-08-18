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

mod manifest;
mod related_tests;
#[cfg(test)]
mod tests;
mod traversal;

pub fn derive(
    root: &Path,
    project_id: &str,
    files: &[crate::model::SourceFile],
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<FlowDerivation, String> {
    let (mut entry_evidence, _) = manifest::parse_manifest(root, files)?;
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
    let related_test_evidence = related_tests::derive_related_tests(nodes, edges);
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
        let flow = traversal::project_flow(
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

pub use traversal::flow_id;
