//! Adjacent Git snapshot continuity for one focused Context Ref.

use super::*;
use crate::model::{
    GraphEdge, GraphNode, HISTORICAL_CONTEXT_CONTINUITY_SCHEMA, HistoricalContextContinuity,
    HistoricalContinuityCounts, HistoricalEdgeChange, HistoricalFlowChange, HistoricalNodeChange,
    HistoricalSnapshot,
};
use continuity_evidence::{
    bound_path_lineage, bound_paths, graph_basis_from_reference, path_changes,
    path_lineage_candidates, snapshot_basis, snapshot_basis_relations, unavailable_basis_relations,
};

const MAX_CONTINUITY_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoricalContinuityLimits {
    pub max_paths: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_flows: usize,
}

pub fn get_historical_context_continuity(
    root: &Path,
    uri: &str,
    from_revision: Option<&str>,
    to_revision: Option<&str>,
    limits: HistoricalContinuityLimits,
) -> Result<HistoricalContextContinuity, String> {
    let graph = store::current_graph(root)?
        .ok_or_else(|| "A current graph is required for historical continuity.".to_string())?;
    let reference = store::resolve_context(root, uri)?;
    if reference.project_id != graph.project_id {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "wrong-project",
            "context-ref-project-does-not-match-current-repository",
        ));
    }
    if git_is_dirty(root) {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "historical-continuity-unavailable-for-dirty-source",
        ));
    }
    let current_identity = crate::identity::resolve(root)?;
    let to = to_revision
        .map(|revision| resolve_revision(root, revision))
        .transpose()?
        .unwrap_or(current_head(root)?);
    let direct_parent = first_parent(root, &to)?;
    let from = match from_revision {
        Some(revision) => Some(resolve_revision(root, revision)?),
        None => direct_parent.clone(),
    };
    let Some(from) = from else {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "predecessor-revision-unavailable",
        ));
    };
    if from_revision.is_some() && direct_parent.as_deref() != Some(from.as_str()) {
        let mut response = unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "non-adjacent-first-parent-range",
        );
        response.from_revision = Some(from);
        response.to_revision = Some(to);
        return Ok(response);
    }
    let snapshot_limits = DiagnosticLimits {
        max_paths: limits.max_paths.max(1),
        max_snapshot_bytes: MAX_CONTINUITY_SNAPSHOT_BYTES,
        ..DiagnosticLimits::default()
    };
    let mut cache = BTreeMap::new();
    let before = load_or_build_historical_snapshot(root, &from, &snapshot_limits, &mut cache)?;
    let after = load_or_build_historical_snapshot(root, &to, &snapshot_limits, &mut cache)?;
    if before.project_id != graph.project_id || after.project_id != graph.project_id {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "historical-snapshot-project-mismatch",
        ));
    }
    let Some(repository_id) = current_identity.repository_id.as_deref() else {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "historical-repository-identity-unavailable",
        ));
    };
    if before.repository_identity_id.is_none() || after.repository_identity_id.is_none() {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "legacy-repository-identity-unavailable",
        ));
    }
    if before.repository_identity_id.as_deref() != Some(repository_id)
        || after.repository_identity_id.as_deref() != Some(repository_id)
    {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "historical-repository-identity-mismatch",
        ));
    }
    let Some(before_contract) = before.evidence_contract.as_ref() else {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "legacy-evidence-contract-unavailable",
        ));
    };
    let Some(after_contract) = after.evidence_contract.as_ref() else {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "legacy-evidence-contract-unavailable",
        ));
    };
    if before_contract != after_contract {
        return Ok(unavailable(
            graph.project_id,
            uri,
            "unavailable",
            "incompatible-evidence-contract",
        ));
    }
    let origin_basis = Some(graph_basis_from_reference(&reference));
    let from_basis = Some(snapshot_basis(&before));
    let to_basis = Some(snapshot_basis(&after));
    let basis_relations = snapshot_basis_relations(&before, &after);
    let focused = graph.nodes.iter().find(|node| node.id == reference.node_id);
    let target_path = focused.and_then(|node| node.path.clone());
    let target_kind = focused.map(|node| node.kind.clone());
    let mut omissions = Vec::new();
    let mut truncated = before.truncated || after.truncated;
    if before.truncated || after.truncated {
        omissions.push("historical-snapshot-truncated".to_string());
    }

    let path_changes_all = path_changes(&before, &after);
    let path_total = path_changes_all.len();
    let path_lineage_all = path_lineage_candidates(&path_changes_all);
    let path_lineage_total = path_lineage_all.len();
    let path_changes = bound_paths(
        path_changes_all,
        limits.max_paths,
        &mut truncated,
        &mut omissions,
    );
    let path_lineage_candidates = bound_path_lineage(
        path_lineage_all,
        limits.max_paths,
        &mut truncated,
        &mut omissions,
    );

    let before_node = before
        .nodes
        .iter()
        .find(|node| node.id == reference.node_id);
    let after_node = after.nodes.iter().find(|node| node.id == reference.node_id);
    let (node_status, fingerprint_relation, node_changes_all, lineage_candidates) =
        focused_node_changes(
            reference.node_id.as_str(),
            before_node,
            after_node,
            &after.nodes,
            target_kind.as_deref(),
            target_path.as_deref(),
        );
    let node_total = node_changes_all.len();
    let node_changes = bound_nodes(
        node_changes_all,
        limits.max_nodes,
        &mut truncated,
        &mut omissions,
    );
    let edge_changes_all = direct_edge_changes(&before.edges, &after.edges, &reference.node_id);
    let edge_total = edge_changes_all.len();
    let edge_changes = bound_edges(
        edge_changes_all,
        limits.max_edges,
        &mut truncated,
        &mut omissions,
    );
    let flow_changes_all = focused_flow_changes(&before, &after, &reference.node_id);
    let flow_total = flow_changes_all.len();
    let flow_changes = bound_flows(
        flow_changes_all,
        limits.max_flows,
        &mut truncated,
        &mut omissions,
    );
    let candidate_total = lineage_candidates.len();
    let mut lineage_candidates = lineage_candidates;
    if lineage_candidates.len() > limits.max_nodes {
        lineage_candidates.truncate(limits.max_nodes);
        truncated = true;
        omissions.push(format!("lineage candidates capped at {}", limits.max_nodes));
    }
    if limits.max_nodes == 0 && candidate_total > 0 {
        lineage_candidates.clear();
        truncated = true;
        omissions.push("lineage candidates capped at zero".to_string());
    }
    omissions.sort();
    omissions.dedup();
    Ok(HistoricalContextContinuity {
        schema_version: HISTORICAL_CONTEXT_CONTINUITY_SCHEMA.to_string(),
        project_id: graph.project_id,
        reference_uri: uri.to_string(),
        status: if reference.status == "current" {
            "available".to_string()
        } else {
            "stale".to_string()
        },
        reason: if reference.status == "current" {
            "adjacent-first-parent-snapshot-compared".to_string()
        } else {
            "origin-context-ref-is-stale".to_string()
        },
        relation: "observed-after-adjacent-first-parent".to_string(),
        from_revision: Some(from),
        to_revision: Some(to),
        origin_basis,
        from_basis,
        to_basis,
        basis_relations,
        node_status,
        fingerprint_relation,
        path_changes,
        path_lineage_candidates,
        node_changes,
        edge_changes,
        flow_changes,
        lineage_candidates,
        counts: HistoricalContinuityCounts {
            path_changes: path_total,
            path_lineage_candidates: path_lineage_total,
            node_changes: node_total,
            edge_changes: edge_total,
            flow_changes: flow_total,
            lineage_candidates: candidate_total,
        },
        truncated,
        omissions,
        limitations: vec![
            "Git snapshot continuity is adjacent static evidence, not runtime sequence or causality.".to_string(),
            "Lineage candidates are not successor proof and never create automatic supersession.".to_string(),
        ],
    })
}

fn unavailable(
    project_id: String,
    uri: &str,
    status: &str,
    reason: &str,
) -> HistoricalContextContinuity {
    HistoricalContextContinuity {
        schema_version: HISTORICAL_CONTEXT_CONTINUITY_SCHEMA.to_string(),
        project_id,
        reference_uri: uri.to_string(),
        status: status.to_string(),
        reason: reason.to_string(),
        relation: "observed-after-adjacent-first-parent".to_string(),
        from_revision: None,
        to_revision: None,
        origin_basis: None,
        from_basis: None,
        to_basis: None,
        basis_relations: unavailable_basis_relations(),
        node_status: "unavailable".to_string(),
        fingerprint_relation: "unavailable".to_string(),
        path_changes: Vec::new(),
        path_lineage_candidates: Vec::new(),
        node_changes: Vec::new(),
        edge_changes: Vec::new(),
        flow_changes: Vec::new(),
        lineage_candidates: Vec::new(),
        counts: HistoricalContinuityCounts::default(),
        truncated: false,
        omissions: Vec::new(),
        limitations: vec![reason.to_string()],
    }
}

fn focused_node_changes(
    node_id: &str,
    before: Option<&GraphNode>,
    after: Option<&GraphNode>,
    after_nodes: &[GraphNode],
    target_kind: Option<&str>,
    _target_path: Option<&str>,
) -> (String, String, Vec<HistoricalNodeChange>, Vec<String>) {
    let (status, fingerprint_relation, reason) = match (before, after) {
        (Some(left), Some(right)) if left.evidence_fingerprint == right.evidence_fingerprint => (
            "retained",
            "identical",
            "node-identity-and-fingerprint-match",
        ),
        (Some(_), Some(_)) => ("changed", "changed", "node-fingerprint-changed"),
        (Some(_), None) => ("removed", "unavailable", "node-identity-missing"),
        (None, Some(_)) => ("added", "unavailable", "node-added"),
        (None, None) => ("unavailable", "unavailable", "node-identity-unavailable"),
    };
    let change = if status == "retained" {
        Vec::new()
    } else {
        vec![HistoricalNodeChange {
            node_id: node_id.to_string(),
            status: status.to_string(),
            before: before.cloned(),
            after: after.cloned(),
            reason: reason.to_string(),
        }]
    };
    let mut candidates = if before.is_some() && after.is_none() {
        let fingerprint = before
            .map(|node| node.evidence_fingerprint.as_str())
            .unwrap_or_default();
        after_nodes
            .iter()
            .filter(|node| {
                Some(node.kind.as_str()) == target_kind
                    && !fingerprint.is_empty()
                    && node.evidence_fingerprint == fingerprint
                    && node.id != node_id
            })
            .map(|node| node.id.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    candidates.sort();
    candidates.dedup();
    (
        status.to_string(),
        fingerprint_relation.to_string(),
        change,
        candidates,
    )
}

fn direct_edge_changes(
    before: &[GraphEdge],
    after: &[GraphEdge],
    node_id: &str,
) -> Vec<HistoricalEdgeChange> {
    let before = before
        .iter()
        .filter(|edge| edge.from == node_id || edge.to == node_id)
        .cloned()
        .collect::<BTreeSet<_>>();
    let after = after
        .iter()
        .filter(|edge| edge.from == node_id || edge.to == node_id)
        .cloned()
        .collect::<BTreeSet<_>>();
    before
        .difference(&after)
        .map(|edge| HistoricalEdgeChange {
            status: "removed".to_string(),
            edge: edge.clone(),
        })
        .chain(after.difference(&before).map(|edge| HistoricalEdgeChange {
            status: "added".to_string(),
            edge: edge.clone(),
        }))
        .collect()
}

fn focused_flow_changes(
    before: &HistoricalSnapshot,
    after: &HistoricalSnapshot,
    node_id: &str,
) -> Vec<HistoricalFlowChange> {
    let before = before
        .flows
        .iter()
        .filter(|flow| {
            flow.entry_node_id == node_id || flow.steps.iter().any(|step| step.node_id == node_id)
        })
        .map(|flow| (flow.flow_id.clone(), flow.fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .flows
        .iter()
        .filter(|flow| {
            flow.entry_node_id == node_id || flow.steps.iter().any(|step| step.node_id == node_id)
        })
        .map(|flow| (flow.flow_id.clone(), flow.fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|flow_id| {
            let before_fingerprint = before.get(&flow_id).cloned();
            let after_fingerprint = after.get(&flow_id).cloned();
            let status = match (&before_fingerprint, &after_fingerprint) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(left), Some(right)) if left != right => "changed",
                _ => return None,
            };
            Some(HistoricalFlowChange {
                flow_id,
                status: status.to_string(),
                before_fingerprint,
                after_fingerprint,
            })
        })
        .collect()
}

fn bound_nodes(
    mut values: Vec<HistoricalNodeChange>,
    limit: usize,
    truncated: &mut bool,
    omissions: &mut Vec<String>,
) -> Vec<HistoricalNodeChange> {
    if values.len() > limit {
        values.truncate(limit);
        *truncated = true;
        omissions.push(format!("historical node changes capped at {limit}"));
    }
    values
}

fn bound_edges(
    mut values: Vec<HistoricalEdgeChange>,
    limit: usize,
    truncated: &mut bool,
    omissions: &mut Vec<String>,
) -> Vec<HistoricalEdgeChange> {
    if values.len() > limit {
        values.truncate(limit);
        *truncated = true;
        omissions.push(format!("historical edge changes capped at {limit}"));
    }
    values
}

fn bound_flows(
    mut values: Vec<HistoricalFlowChange>,
    limit: usize,
    truncated: &mut bool,
    omissions: &mut Vec<String>,
) -> Vec<HistoricalFlowChange> {
    if values.len() > limit {
        values.truncate(limit);
        *truncated = true;
        omissions.push(format!("historical flow changes capped at {limit}"));
    }
    values
}
