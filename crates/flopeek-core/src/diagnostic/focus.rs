//! Diagnostic focus and graph-basis validation.

#[allow(unused_imports)]
use super::*;

pub(super) fn focus_paths(
    root: &Path,
    context: &DiagnosticContext,
    graph_snapshot: &crate::model::GraphSnapshot,
    limits: &DiagnosticLimits,
) -> Result<FocusPathSets, String> {
    let mut focus = BTreeSet::new();
    let mut focus_flow_ids = BTreeSet::new();
    let mut limitations = Vec::new();
    let mut starts = Vec::new();
    for uri in context
        .focus_context_refs
        .iter()
        .take(limits.max_context_refs)
    {
        let resolved = store::resolve_context(root, uri)?;
        starts.push(resolved.node_id.clone());
        if resolved.status != "current" {
            limitations.push(format!("focus Context Ref {uri} is {}.", resolved.status));
        }
        if let Some(node) = graph_snapshot
            .nodes
            .iter()
            .find(|node| node.id == resolved.node_id)
        {
            if let Some(path) = &node.path {
                focus.insert(path.clone());
            }
        } else {
            limitations.push(format!(
                "focus node {} is unavailable in the current graph.",
                resolved.node_id
            ));
        }
    }
    if context.focus_context_refs.len() > limits.max_context_refs {
        limitations.push(format!(
            "focus Context Refs capped at {}.",
            limits.max_context_refs
        ));
    }
    for uri in context.focus_flow_refs.iter().take(limits.max_context_refs) {
        let resolved = store::resolve_flow(root, uri)?;
        focus_flow_ids.insert(resolved.flow_id.clone());
        if resolved.status != "current" {
            limitations.push(format!("focus Flow Ref {uri} is {}.", resolved.status));
        }
        if let Some(flow) = graph_snapshot
            .flows
            .iter()
            .find(|flow| flow.flow_id == resolved.flow_id)
        {
            starts.push(flow.entry_node_id.clone());
            for step in &flow.steps {
                starts.push(step.node_id.clone());
                if let Some(path) = &step.path
                    && path != "package.json"
                {
                    focus.insert(path.clone());
                }
            }
        } else {
            limitations.push(format!(
                "focus flow {} is unavailable in the current graph.",
                resolved.flow_id
            ));
        }
    }
    if context.focus_flow_refs.len() > limits.max_context_refs {
        limitations.push(format!(
            "focus Flow Refs capped at {}.",
            limits.max_context_refs
        ));
    }
    let mut cone = focus.clone();
    let mut queue = starts.into_iter().collect::<VecDeque<_>>();
    let mut visited = BTreeSet::new();
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone())
            || visited.len() > limits.max_paths.saturating_mul(8).max(1)
        {
            continue;
        }
        for edge in graph_snapshot
            .edges
            .iter()
            .filter(|edge| edge.from == node_id || edge.to == node_id)
        {
            let neighbour = if edge.from == node_id {
                &edge.to
            } else {
                &edge.from
            };
            queue.push_back(neighbour.clone());
            if let Some(node) = graph_snapshot
                .nodes
                .iter()
                .find(|node| node.id == *neighbour)
                && let Some(path) = &node.path
            {
                cone.insert(path.clone());
            }
        }
    }
    Ok((focus, cone, focus_flow_ids, limitations))
}

pub(crate) fn validate_basis(basis: &GraphBasis) -> Result<(), String> {
    validate_id("graph project id", &basis.project_id)?;
    validate_id("graph id", &basis.graph_id)?;
    validate_id("observation id", &basis.observation_id)?;
    if basis.graph_version == 0 {
        return Err("currentGraphBasis.graphVersion must be greater than zero.".to_string());
    }
    validate_revision(&basis.source_revision)
}
