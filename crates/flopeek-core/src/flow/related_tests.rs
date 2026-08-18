//! Related-test evidence derived from proven graph edges.

use super::manifest;
use super::*;
pub(super) fn derive_related_tests(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> RelatedTestEvidence {
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
        if !manifest::is_test_path(test_path) || manifest::is_test_path(target_path) {
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
