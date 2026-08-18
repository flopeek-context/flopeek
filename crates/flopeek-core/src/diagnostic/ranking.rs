//! Historical candidate ranking.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(super) struct CommitRecord {
    pub(super) sha: String,
    pub(super) parents: Vec<String>,
    pub(super) summary: String,
}

pub(super) type FocusPathSets = (
    BTreeSet<String>,
    BTreeSet<String>,
    BTreeSet<String>,
    Vec<String>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn historical_delta_reasons(
    root: &Path,
    commit: &CommitRecord,
    focus_paths: &BTreeSet<String>,
    cone_paths: &BTreeSet<String>,
    focus_flow_ids: &BTreeSet<String>,
    limits: &DiagnosticLimits,
    cache: &mut BTreeMap<String, HistoricalSnapshot>,
) -> Result<(Vec<String>, u32, Vec<String>), String> {
    let current = load_or_build_historical_snapshot(root, &commit.sha, limits, cache)?;
    let parent = load_or_build_historical_snapshot(root, &commit.parents[0], limits, cache)?;
    let mut reasons = Vec::new();
    let mut score = 0;
    let mut notes = Vec::new();
    let current_flows = current
        .flows
        .iter()
        .map(|flow| (flow.flow_id.as_str(), flow.fingerprint.as_str()))
        .collect::<BTreeMap<_, _>>();
    let parent_flows = parent
        .flows
        .iter()
        .map(|flow| (flow.flow_id.as_str(), flow.fingerprint.as_str()))
        .collect::<BTreeMap<_, _>>();
    for flow_id in focus_flow_ids {
        if current_flows.get(flow_id.as_str()) != parent_flows.get(flow_id.as_str()) {
            reasons.push("focused-flow-changed".to_string());
            score += 75;
        }
        let current_flow = current.flows.iter().find(|flow| flow.flow_id == *flow_id);
        let parent_flow = parent.flows.iter().find(|flow| flow.flow_id == *flow_id);
        if current_flow.map(|flow| (&flow.entry_kind, &flow.entry_key))
            != parent_flow.map(|flow| (&flow.entry_kind, &flow.entry_key))
        {
            reasons.push("focused-entry-changed".to_string());
            score += 60;
        }
        let current_related = current_flow
            .map(|flow| flow.related_tests.clone())
            .unwrap_or_default();
        let parent_related = parent_flow
            .map(|flow| flow.related_tests.clone())
            .unwrap_or_default();
        if current_related != parent_related {
            reasons.push("related-test-structure-changed".to_string());
            score += 25;
        }
    }
    let focused_entry_changed = focus_flow_ids.iter().any(|flow_id| {
        let current_flow = current.flows.iter().find(|flow| flow.flow_id == *flow_id);
        let parent_flow = parent.flows.iter().find(|flow| flow.flow_id == *flow_id);
        let Some(current_flow) = current_flow else {
            return parent_flow.is_some();
        };
        let current_entry = current.entry_evidence.records.iter().find(|record| {
            record.kind == current_flow.entry_kind && record.key == current_flow.entry_key
        });
        let parent_entry = parent.entry_evidence.records.iter().find(|record| {
            record.kind == current_flow.entry_kind && record.key == current_flow.entry_key
        });
        current_entry != parent_entry
    });
    if focused_entry_changed
        && !reasons
            .iter()
            .any(|reason| reason == "focused-entry-changed")
    {
        reasons.push("focused-entry-changed".to_string());
        score += 60;
    }
    let current_files = current
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let parent_files = parent
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut changed_files = BTreeSet::new();
    changed_files.extend(current_files.keys().copied());
    changed_files.extend(parent_files.keys().copied());
    for path in changed_files {
        if current_files.get(path) != parent_files.get(path) {
            if focus_paths.contains(path) {
                reasons.push("focused-node-changed".to_string());
                score += 35;
            } else if cone_paths.contains(path) {
                reasons.push("dependency-cone-node-changed".to_string());
                score += 20;
            }
        }
    }
    let current_edges = current
        .edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str(), edge.kind.as_str()))
        .collect::<BTreeSet<_>>();
    let parent_edges = parent
        .edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str(), edge.kind.as_str()))
        .collect::<BTreeSet<_>>();
    let current_node_paths = current
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .as_ref()
                .map(|path| (node.id.as_str(), path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let parent_node_paths = parent
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .as_ref()
                .map(|path| (node.id.as_str(), path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut changed_edges = current_edges.clone();
    changed_edges.extend(parent_edges.iter().copied());
    for (from, to, _) in changed_edges {
        if current_edges.contains(&(from, to, "calls"))
            != parent_edges.contains(&(from, to, "calls"))
            || current_edges.contains(&(from, to, "constructs"))
                != parent_edges.contains(&(from, to, "constructs"))
            || current_edges.contains(&(from, to, "imports"))
                != parent_edges.contains(&(from, to, "imports"))
        {
            let from_path = current_node_paths
                .get(from)
                .or_else(|| parent_node_paths.get(from));
            let to_path = current_node_paths
                .get(to)
                .or_else(|| parent_node_paths.get(to));
            if from_path.is_some_and(|path| focus_paths.contains(*path))
                || to_path.is_some_and(|path| focus_paths.contains(*path))
            {
                if !reasons
                    .iter()
                    .any(|reason| reason == "focused-edge-changed")
                {
                    reasons.push("focused-edge-changed".to_string());
                    score += 45;
                }
            } else if (from_path.is_some_and(|path| cone_paths.contains(*path))
                || to_path.is_some_and(|path| cone_paths.contains(*path)))
                && !reasons
                    .iter()
                    .any(|reason| reason == "dependency-cone-edge-changed")
            {
                reasons.push("dependency-cone-edge-changed".to_string());
                score += 25;
            }
        }
    }
    if current.truncated || parent.truncated {
        notes.push("historical graph snapshot was truncated by declared bounds".to_string());
    }
    Ok((reasons, score, notes))
}
