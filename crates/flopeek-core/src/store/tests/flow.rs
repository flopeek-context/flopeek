use super::*;

#[test]
fn flow_ref_round_trip_is_canonical_and_origin_is_immutable() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("first persist");
    assert!(!first.flow_refs.is_empty());
    let first_ref = first.flow_refs[0].clone();
    assert_eq!(first_ref.status, "current");
    assert_eq!(first_ref.freshness_reason, "origin-observation-current");
    assert_eq!(first_ref.origin_observation_id, first.graph.observation_id);
    assert_eq!(
        first_ref
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&first.graph.observation_id)
    );
    assert_eq!(
        resolve_flow(&root, &first_ref.uri).expect("resolve"),
        first_ref
    );

    fs::write(
        root.join("package.json"),
        "{\n  \"scripts\": {\"start\": \"tsx src/main.ts\", \"unsupported\": \"tsx src/main.ts && echo credential-sentinel\"},\n  \"bin\": {\"checkout\": \"src/main.ts\"},\n  \"main\": \"src/main\",\n  \"module\": \"src/main.ts\"\n}\n",
    )
    .expect("format package");
    let (snapshot, facts) = graph::build(&root).expect("second build");
    let second = persist_scan(&root, snapshot, &facts).expect("second persist");
    assert_eq!(second.graph.graph_id, first.graph.graph_id);
    assert_eq!(second.graph.graph_version, first.graph.graph_version);
    assert_ne!(second.graph.observation_id, first.graph.observation_id);
    let second_ref = second
        .flow_refs
        .iter()
        .find(|reference| reference.uri == first_ref.uri)
        .expect("same flow ref");
    assert_eq!(
        second_ref.origin_observation_id,
        first_ref.origin_observation_id
    );
    assert_eq!(second_ref.status, "current");
    assert_eq!(second_ref.freshness_reason, "flow-fingerprint-match");
    assert_eq!(
        second_ref
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&second.graph.observation_id)
    );
    assert_eq!(
        resolve_flow(&root, &first_ref.uri).expect("resolve second"),
        *second_ref
    );

    fs::write(
        root.join("package.json"),
        "{\"scripts\":{\"start\":\"tsx src/other.ts\",\"unsupported\":\"tsx src/main.ts && echo credential-sentinel\"},\"bin\":{\"checkout\":\"src/main.ts\"},\"main\":\"src/main\",\"module\":\"src/main.ts\"}",
    )
    .expect("changed entry target");
    fs::write(
        root.join("src/other.ts"),
        "export function other() { return 'other'; }\n",
    )
    .expect("other target");
    let (snapshot, facts) = graph::build(&root).expect("third build");
    let third = persist_scan(&root, snapshot, &facts).expect("third persist");
    let stale = resolve_flow(&root, &first_ref.uri).expect("stale flow");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-fingerprint-changed");
    assert_eq!(stale.origin_observation_id, first_ref.origin_observation_id);
    assert_eq!(
        stale
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&third.graph.observation_id)
    );
    let connection = open(&root).expect("open");
    let payloads = connection
        .prepare("SELECT payload_json FROM graph_flows UNION ALL SELECT entry_json FROM graph_flow_evidence")
        .expect("flow payloads")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("flow rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("flow payload values")
        .join("\n");
    assert!(!payloads.contains("credential-sentinel"));
    assert!(!payloads.contains("source-body-sentinel"));
    assert!(!payloads.contains(root.to_string_lossy().as_ref()));
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_ref_reports_missing_identity_and_wrong_project_explicitly() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    let connection = open(&root).expect("open");
    let wrong_uri = flow_ref::uri("different-project", &reference.graph_id, &reference.flow_id);
    let wrong =
        flow_ref::resolve(&connection, &wrong_uri, &first.project_id).expect("wrong project");
    assert_eq!(wrong.status, "wrong-project");
    drop(connection);
    let connection = open(&root).expect("open again");
    connection
        .execute(
            "DELETE FROM graph_flows WHERE graph_version = ?1 AND flow_id = ?2",
            params![first.graph.graph_version as i64, reference.flow_id],
        )
        .expect("remove current flow");
    drop(connection);
    let stale = resolve_flow(&root, &reference.uri).expect("stale resolve");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-identity-missing");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn flow_ref_remains_current_for_unrelated_source_but_tracks_related_test_changes() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    assert!(!first.graph.flows[0].related_tests.is_empty());

    fs::write(
        root.join("src/unrelated.ts"),
        "export const unrelated = 'unrelated';\n",
    )
    .expect("unrelated source");
    let (snapshot, facts) = graph::build(&root).expect("unrelated build");
    let unrelated = persist_scan(&root, snapshot, &facts).expect("unrelated persist");
    let current = resolve_flow(&root, &reference.uri).expect("current flow");
    assert_eq!(current.status, "current");
    assert_eq!(current.freshness_reason, "flow-fingerprint-match");
    assert_eq!(
        current.origin_observation_id,
        reference.origin_observation_id
    );
    assert_ne!(unrelated.graph.observation_id, first.graph.observation_id);

    fs::write(
        root.join("tests/main.test.ts"),
        "export const unrelatedTest = true;\n",
    )
    .expect("related test change");
    let (snapshot, facts) = graph::build(&root).expect("test change build");
    let changed = persist_scan(&root, snapshot, &facts).expect("test change persist");
    let stale = resolve_flow(&root, &reference.uri).expect("related test stale");
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.freshness_reason, "flow-fingerprint-changed");
    assert_eq!(stale.origin_observation_id, reference.origin_observation_id);
    assert_eq!(
        stale
            .current_basis
            .as_ref()
            .map(|basis| &basis.observation_id),
        Some(&changed.graph.observation_id)
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corrupted_flow_ref_metadata_rolls_back_the_entire_scan() {
    let root = flow_fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = first.flow_refs.first().expect("flow ref").clone();
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE flow_refs SET origin_fingerprint = 'corrupted' WHERE uri = ?1",
            params![reference.uri],
        )
        .expect("corrupt flow ref");
    drop(connection);
    fs::write(
        root.join("package.json"),
        "{\n  \"scripts\": {\"start\": \"tsx src/main.ts\", \"unsupported\": \"tsx src/main.ts && echo credential-sentinel\"},\n  \"bin\": {\"checkout\": \"src/main.ts\"},\n  \"main\": \"src/main\",\n  \"module\": \"src/main.ts\"\n}\n",
    )
    .expect("format package");
    let (snapshot, facts) = graph::build(&root).expect("second build");
    assert!(persist_scan(&root, snapshot, &facts).is_err());
    let connection = open(&root).expect("reopen");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                .get::<_, i64>(0))
            .expect("observations"),
        1
    );
    assert_eq!(
        status(&root).expect("status").current_graph_version,
        Some(first.graph.graph_version)
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT origin_fingerprint FROM flow_refs WHERE uri = ?1",
                params![reference.uri],
                |row| row.get::<_, String>(0),
            )
            .expect("flow fingerprint"),
        "corrupted"
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}
