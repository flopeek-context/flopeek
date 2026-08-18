//! Pure temporal identity and adjacent-observation comparison rules.

use crate::model::{
    EdgeChange, EvidenceContract, FlowChange, GraphBasis, GraphEdge, GraphNode, NodeChange,
    OBSERVATION_DELTA_SCHEMA, ObservationBasisRelations, ObservationDelta, ObservationDeltaCounts,
    SourceChange, SourceFile,
};
use std::collections::BTreeMap;

pub const NODE_FINGERPRINT_CONTRACT: &str = "node-ast-and-direct-edges/v1";
pub const LEGACY_FILE_FINGERPRINT_CONTRACT: &str = "legacy-file-v1";
pub const LEGACY_EVIDENCE_CONTRACT: &str = "legacy-evidence-contract-unavailable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaLimits {
    pub max_source_changes: usize,
    pub max_node_changes: usize,
    pub max_edge_changes: usize,
    pub max_flow_changes: usize,
}

impl Default for DeltaLimits {
    fn default() -> Self {
        Self {
            max_source_changes: 256,
            max_node_changes: 512,
            max_edge_changes: 1024,
            max_flow_changes: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowFingerprint {
    pub flow_id: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationEvidence {
    pub basis: GraphBasis,
    pub contract: EvidenceContract,
    pub source_fingerprint: String,
    pub source_files: Vec<SourceFile>,
    pub module_resolution_exact_fingerprint: String,
    pub module_resolution_effective_fingerprint: String,
    pub entry_manifest_fingerprint: String,
    pub entry_effective_fingerprint: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub flows: Vec<FlowFingerprint>,
}

pub fn compare_observations(
    project_id: &str,
    from_event_id: &str,
    to_event_id: &str,
    from: &ObservationEvidence,
    to: &ObservationEvidence,
    limits: &DeltaLimits,
) -> ObservationDelta {
    let from_contract_available = contract_available(&from.contract);
    let to_contract_available = contract_available(&to.contract);
    let contract_compatible =
        from_contract_available && to_contract_available && from.contract == to.contract;
    let same_graph = from.basis.graph_id == to.basis.graph_id;
    let graph_relation = if same_graph && contract_compatible {
        "same-structural-graph"
    } else if contract_compatible {
        "structural-graph-changed"
    } else {
        "unavailable"
    };
    let basis_relations = ObservationBasisRelations {
        typescript_source: fingerprint_relation(&from.source_fingerprint, &to.source_fingerprint),
        module_resolution_exact: fingerprint_relation(
            &from.module_resolution_exact_fingerprint,
            &to.module_resolution_exact_fingerprint,
        ),
        module_resolution_effective: fingerprint_relation(
            &from.module_resolution_effective_fingerprint,
            &to.module_resolution_effective_fingerprint,
        ),
        entry_manifest_exact: fingerprint_relation(
            &from.entry_manifest_fingerprint,
            &to.entry_manifest_fingerprint,
        ),
        entry_manifest_effective: fingerprint_relation(
            &from.entry_effective_fingerprint,
            &to.entry_effective_fingerprint,
        ),
    };

    let mut counts = ObservationDeltaCounts::default();
    let source_changes = source_changes(from, to, &mut counts);
    let node_changes = if contract_compatible {
        node_changes(from, to, &mut counts)
    } else {
        Vec::new()
    };
    let edge_changes = if contract_compatible {
        edge_changes(from, to, &mut counts)
    } else {
        Vec::new()
    };
    let flow_changes = if contract_compatible {
        flow_changes(from, to, &mut counts)
    } else {
        Vec::new()
    };

    let mut omissions = Vec::new();
    let (source_changes, source_truncated) = bound(
        source_changes,
        limits.max_source_changes,
        "source changes",
        &mut omissions,
    );
    let (node_changes, node_truncated) = bound(
        node_changes,
        limits.max_node_changes,
        "node changes",
        &mut omissions,
    );
    let (edge_changes, edge_truncated) = bound(
        edge_changes,
        limits.max_edge_changes,
        "edge changes",
        &mut omissions,
    );
    let (flow_changes, flow_truncated) = bound(
        flow_changes,
        limits.max_flow_changes,
        "flow changes",
        &mut omissions,
    );
    let truncated = source_truncated || node_truncated || edge_truncated || flow_truncated;
    let status = if graph_relation == "unavailable" {
        "unavailable"
    } else if truncated {
        "truncated"
    } else {
        "complete"
    };
    let reason = if same_graph && !contract_compatible {
        "evidence-contract-unavailable"
    } else if graph_relation == "unavailable" {
        "incompatible-evidence-contract"
    } else if same_graph {
        "same-structural-graph"
    } else if truncated {
        "change-bounds-reached"
    } else {
        "adjacent-structural-graph-change"
    };
    if graph_relation == "unavailable" {
        omissions.push(
            "structural changes omitted because evidence contracts are incompatible".to_string(),
        );
    }
    omissions.sort();
    omissions.dedup();
    ObservationDelta {
        schema_version: OBSERVATION_DELTA_SCHEMA.to_string(),
        project_id: project_id.to_string(),
        status: status.to_string(),
        reason: reason.to_string(),
        from_event_id: Some(from_event_id.to_string()),
        to_event_id: Some(to_event_id.to_string()),
        relation: "observed-after".to_string(),
        from_basis: Some(from.basis.clone()),
        to_basis: Some(to.basis.clone()),
        from_contract: Some(from.contract.clone()),
        to_contract: Some(to.contract.clone()),
        contract_compatible,
        graph_relation: graph_relation.to_string(),
        basis_relations,
        counts,
        source_changes,
        node_changes,
        edge_changes,
        flow_changes,
        truncated,
        omissions,
        limitations: vec![
            "Observation deltas describe adjacent local scans, not Git ancestry or runtime execution order.".to_string(),
            "Structural differences are evidence of change only; they do not establish cause, rename, business intent, or runtime behavior.".to_string(),
        ],
    }
}

fn contract_available(contract: &EvidenceContract) -> bool {
    !contract.graph_schema_version.is_empty()
        && !contract.graph_derivation_id.is_empty()
        && !contract.node_fingerprint_contract.is_empty()
        && contract.graph_schema_version != LEGACY_EVIDENCE_CONTRACT
        && contract.graph_derivation_id != LEGACY_EVIDENCE_CONTRACT
        && contract.node_fingerprint_contract != LEGACY_EVIDENCE_CONTRACT
}

fn fingerprint_relation(before: &str, after: &str) -> String {
    if before.is_empty() || after.is_empty() {
        "unavailable".to_string()
    } else if before == after {
        "same".to_string()
    } else {
        "changed".to_string()
    }
}

fn source_changes(
    from: &ObservationEvidence,
    to: &ObservationEvidence,
    counts: &mut ObservationDeltaCounts,
) -> Vec<SourceChange> {
    let before = from
        .source_files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let after = to
        .source_files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for path in before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (before.get(&path), after.get(&path)) {
            (None, Some(file)) => {
                counts.source_added += 1;
                changes.push(SourceChange {
                    path,
                    status: "added".to_string(),
                    before_hash: None,
                    after_hash: Some(file.hash.clone()),
                    before_bytes: None,
                    after_bytes: Some(file.bytes),
                });
            }
            (Some(file), None) => {
                counts.source_removed += 1;
                changes.push(SourceChange {
                    path,
                    status: "removed".to_string(),
                    before_hash: Some(file.hash.clone()),
                    after_hash: None,
                    before_bytes: Some(file.bytes),
                    after_bytes: None,
                });
            }
            (Some(before), Some(after)) if *before != *after => {
                counts.source_changed += 1;
                changes.push(SourceChange {
                    path,
                    status: "changed".to_string(),
                    before_hash: Some(before.hash.clone()),
                    after_hash: Some(after.hash.clone()),
                    before_bytes: Some(before.bytes),
                    after_bytes: Some(after.bytes),
                });
            }
            _ => {}
        }
    }
    changes
}

fn node_changes(
    from: &ObservationEvidence,
    to: &ObservationEvidence,
    counts: &mut ObservationDeltaCounts,
) -> Vec<NodeChange> {
    let before = from
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let after = to
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for node_id in before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (before.get(&node_id), after.get(&node_id)) {
            (None, Some(node)) => {
                counts.node_added += 1;
                changes.push(NodeChange {
                    node_id,
                    status: "added".to_string(),
                    before: None,
                    after: Some((*node).clone()),
                });
            }
            (Some(node), None) => {
                counts.node_removed += 1;
                changes.push(NodeChange {
                    node_id,
                    status: "removed".to_string(),
                    before: Some((*node).clone()),
                    after: None,
                });
            }
            (Some(before), Some(after)) if *before != *after => {
                counts.node_changed += 1;
                changes.push(NodeChange {
                    node_id,
                    status: "changed".to_string(),
                    before: Some((*before).clone()),
                    after: Some((*after).clone()),
                });
            }
            _ => {}
        }
    }
    changes
}

fn edge_changes(
    from: &ObservationEvidence,
    to: &ObservationEvidence,
    counts: &mut ObservationDeltaCounts,
) -> Vec<EdgeChange> {
    let before = from
        .edges
        .iter()
        .map(|edge| (edge_key(edge), edge))
        .collect::<BTreeMap<_, _>>();
    let after = to
        .edges
        .iter()
        .map(|edge| (edge_key(edge), edge))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for key in before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (before.get(&key), after.get(&key)) {
            (None, Some(edge)) => {
                counts.edge_added += 1;
                changes.push(EdgeChange {
                    status: "added".to_string(),
                    edge: (*edge).clone(),
                });
            }
            (Some(edge), None) => {
                counts.edge_removed += 1;
                changes.push(EdgeChange {
                    status: "removed".to_string(),
                    edge: (*edge).clone(),
                });
            }
            _ => {}
        }
    }
    changes
}

fn edge_key(edge: &GraphEdge) -> (String, String, String, String) {
    (
        edge.from.clone(),
        edge.to.clone(),
        edge.kind.clone(),
        edge.evidence.clone(),
    )
}

fn flow_changes(
    from: &ObservationEvidence,
    to: &ObservationEvidence,
    counts: &mut ObservationDeltaCounts,
) -> Vec<FlowChange> {
    let before = from
        .flows
        .iter()
        .map(|flow| (flow.flow_id.clone(), flow.fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let after = to
        .flows
        .iter()
        .map(|flow| (flow.flow_id.clone(), flow.fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for flow_id in before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (before.get(&flow_id), after.get(&flow_id)) {
            (None, Some(fingerprint)) => {
                counts.flow_added += 1;
                changes.push(FlowChange {
                    flow_id,
                    status: "added".to_string(),
                    before_fingerprint: None,
                    after_fingerprint: Some(fingerprint.clone()),
                });
            }
            (Some(fingerprint), None) => {
                counts.flow_removed += 1;
                changes.push(FlowChange {
                    flow_id,
                    status: "removed".to_string(),
                    before_fingerprint: Some(fingerprint.clone()),
                    after_fingerprint: None,
                });
            }
            (Some(before), Some(after)) if before != after => {
                counts.flow_changed += 1;
                changes.push(FlowChange {
                    flow_id,
                    status: "changed".to_string(),
                    before_fingerprint: Some(before.clone()),
                    after_fingerprint: Some(after.clone()),
                });
            }
            _ => {}
        }
    }
    changes
}

fn bound<T>(
    mut values: Vec<T>,
    limit: usize,
    category: &str,
    omissions: &mut Vec<String>,
) -> (Vec<T>, bool) {
    if values.len() > limit {
        values.truncate(limit);
        omissions.push(format!("{category} omitted after max bound {limit}"));
        (values, true)
    } else {
        (values, false)
    }
}

pub fn observation_event_id(
    project_id: &str,
    predecessor_event_id: Option<&str>,
    observation_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-event-v1\0{project_id}\0{}\0{observation_id}",
        predecessor_event_id.unwrap_or_default()
    );
    format!(
        "observation_event_{}",
        blake3::hash(input.as_bytes()).to_hex()
    )
}

pub fn fingerprint_contract(scope: &str) -> &'static str {
    match scope {
        "ast-and-direct-edges" => NODE_FINGERPRINT_CONTRACT,
        "legacy-file-v1" => LEGACY_FILE_FINGERPRINT_CONTRACT,
        _ => LEGACY_FILE_FINGERPRINT_CONTRACT,
    }
}
