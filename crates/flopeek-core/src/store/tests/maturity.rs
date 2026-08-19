use super::*;
use std::fs;

#[test]
fn identical_repository_manifest_across_checkouts_keeps_portable_evidence_stable() {
    let left = fixture_root();
    let right = fixture_root();
    let manifest = r#"{"schemaVersion":"flopeek-repository-identity/v1","repositoryId":"repo_123e4567-e89b-12d3-a456-426614174000"}"#;
    fs::write(left.join(crate::identity::MANIFEST_PATH), manifest).expect("left manifest");
    fs::write(right.join(crate::identity::MANIFEST_PATH), manifest).expect("right manifest");
    let (left_snapshot, left_facts) = graph::build(&left).expect("left build");
    let left_scan = persist_scan(&left, left_snapshot, &left_facts).expect("left scan");
    let (right_snapshot, right_facts) = graph::build(&right).expect("right build");
    let right_scan = persist_scan(&right, right_snapshot, &right_facts).expect("right scan");
    assert_eq!(left_scan.project_id, right_scan.project_id);
    assert_eq!(left_scan.graph.graph_id, right_scan.graph.graph_id);
    assert_eq!(
        left_scan.graph.graph_version,
        right_scan.graph.graph_version
    );
    assert_eq!(
        left_scan.graph.observation_id,
        right_scan.graph.observation_id
    );
    let left_uris = left_scan
        .context_refs
        .iter()
        .map(|reference| reference.uri.clone())
        .collect::<Vec<_>>();
    let right_uris = right_scan
        .context_refs
        .iter()
        .map(|reference| reference.uri.clone())
        .collect::<Vec<_>>();
    assert_eq!(left_uris, right_uris);
    let encoded = serde_json::to_string(&left_scan).expect("portable scan");
    assert!(!encoded.contains(left.to_string_lossy().as_ref()));
    assert!(!encoded.contains(right.to_string_lossy().as_ref()));
    fs::remove_dir_all(left).expect("cleanup left");
    fs::remove_dir_all(right).expect("cleanup right");
}
