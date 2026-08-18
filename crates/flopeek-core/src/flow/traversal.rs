//! Bounded static flow traversal.

use super::*;
pub(super) fn project_flow(
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
