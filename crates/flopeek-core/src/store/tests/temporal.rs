use super::*;

#[test]
fn observation_continuity_is_idempotent_and_preserves_a_b_a_order() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build A");
    let first = persist_scan(&root, snapshot, &facts).expect("persist A");
    let (snapshot, facts) = graph::build(&root).expect("build A again");
    persist_scan(&root, snapshot, &facts).expect("persist A idempotently");
    let same = get_observation_continuity(&root, 128).expect("continuity A");
    assert_eq!(same.events.len(), 1);
    assert_eq!(same.events[0].predecessor_event_id, None);

    fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("write B");
    let (snapshot, facts) = graph::build(&root).expect("build B");
    let second = persist_scan(&root, snapshot, &facts).expect("persist B");
    fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write A again");
    let (snapshot, facts) = graph::build(&root).expect("build A again");
    persist_scan(&root, snapshot, &facts).expect("persist A again");
    let continuity = get_observation_continuity(&root, 128).expect("continuity A-B-A");
    assert_eq!(continuity.events.len(), 3);
    assert_eq!(
        continuity.events[1].predecessor_event_id.as_deref(),
        Some(continuity.events[0].event_id.as_str())
    );
    assert_eq!(
        continuity.events[2].predecessor_event_id.as_deref(),
        Some(continuity.events[1].event_id.as_str())
    );
    assert_eq!(continuity.graph_relation, "structural-graph-changed");
    assert_eq!(
        get_observation_continuity(&root, 0)
            .expect("zero-bound continuity")
            .current_basis
            .as_ref()
            .map(|basis| basis.observation_id.as_str()),
        Some(first.graph.observation_id.as_str())
    );
    assert!(
        get_observation_continuity(&root, 0)
            .expect("zero-bound continuity")
            .truncated
    );
    assert_ne!(first.graph.observation_id, second.graph.observation_id);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn moved_exact_node_is_superseded_and_ambiguous_successors_stay_stale() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build origin");
    let first = persist_scan(&root, snapshot, &facts).expect("persist origin");
    let origin_node = first
        .graph
        .nodes
        .iter()
        .find(|node| node.kind == "variable")
        .expect("origin variable");
    let origin = first
        .context_refs
        .iter()
        .find(|reference| reference.node_id == origin_node.id)
        .expect("origin variable ref")
        .clone();
    fs::rename(root.join("src/main.ts"), root.join("src/moved.ts")).expect("move source");
    let (snapshot, facts) = graph::build(&root).expect("build moved");
    persist_scan(&root, snapshot, &facts).expect("persist moved");
    let moved = resolve_context(&root, &origin.uri).expect("resolve moved origin");
    assert_eq!(moved.status, "superseded");
    assert_eq!(
        moved.freshness_reason,
        "unique-exact-compatible-fingerprint"
    );
    assert!(moved.successor_uri.is_some());
    assert_eq!(moved.origin_observation_id, origin.origin_observation_id);
    let reconciliation = reconcile_context(&root, &origin.uri).expect("reconcile moved origin");
    assert_eq!(
        reconciliation.schema_version,
        crate::model::CONTEXT_RECONCILIATION_SCHEMA
    );
    assert_eq!(reconciliation.status, "superseded");
    assert_eq!(reconciliation.candidates.len(), 1);
    assert_eq!(reconciliation.successor, moved.successor_uri);

    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build ambiguous origin");
    let first = persist_scan(&root, snapshot, &facts).expect("persist ambiguous origin");
    let origin_node = first
        .graph
        .nodes
        .iter()
        .find(|node| node.kind == "variable")
        .expect("ambiguous origin variable");
    let origin = first
        .context_refs
        .iter()
        .find(|reference| reference.node_id == origin_node.id)
        .expect("ambiguous origin ref")
        .clone();
    fs::rename(root.join("src/main.ts"), root.join("src/one.ts")).expect("move one");
    fs::copy(root.join("src/one.ts"), root.join("src/two.ts")).expect("copy two");
    let (snapshot, facts) = graph::build(&root).expect("build ambiguous successors");
    persist_scan(&root, snapshot, &facts).expect("persist ambiguous successors");
    let ambiguous = resolve_context(&root, &origin.uri).expect("resolve ambiguous");
    assert_eq!(ambiguous.status, "stale");
    assert_eq!(ambiguous.freshness_reason, "exact-successor-ambiguous");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn continuity_bounds_and_missing_history_are_explicit() {
    let root = fixture_root();
    let unavailable = get_observation_continuity(&root, 0).expect("unscanned continuity");
    assert_eq!(unavailable.graph_relation, "unavailable");
    assert!(!unavailable.truncated);

    let (snapshot, facts) = graph::build(&root).expect("build");
    persist_scan(&root, snapshot, &facts).expect("persist");
    let bounded = get_observation_continuity(&root, 257).expect("bounded continuity");
    assert!(bounded.truncated);
    assert!(
        bounded
            .omissions
            .iter()
            .any(|omission| omission.contains("capped at 256"))
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn fresh_and_upgraded_v7_schema_match_and_migration_failure_rolls_back() {
    let fresh_root = fixture_root();
    let fresh = open(&fresh_root).expect("fresh schema");
    let fresh_schema = schema_snapshot(&fresh);
    assert_eq!(
        fresh
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("fresh version"),
        CURRENT_USER_VERSION
    );
    drop(fresh);

    let upgraded_root = fixture_root();
    initialize_v6_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v6");
    connection
        .execute(
            "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated, omissions_json)
             VALUES(1, 'graph-v6', ?1, 'revision-v6', 1, 0, '[]')",
            params![project_id],
        )
        .expect("graph row");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-v6', ?1, 1, '{\"schemaVersion\":\"flopeek-diagnostic-context/v3\",\"contextSentinel\":\"keep\",\"focusFlowRefs\":[]}', 1)",
            params![project_id],
        )
        .expect("context row");
    connection
        .execute(
            "INSERT INTO diagnostic_assertions(id, context_id, revision, kind, status, actor, payload_json, created_at)
             VALUES('assertion-v6', 'context-v6', 1, 'observation', 'proposed', 'test', '{\"assertionSentinel\":\"keep\"}', 1)",
            [],
        )
        .expect("assertion row");
    drop(connection);
    let upgraded = open(&upgraded_root).expect("upgrade schema");
    assert_eq!(schema_snapshot(&upgraded), fresh_schema);
    assert_eq!(
        upgraded
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("upgraded version"),
        CURRENT_USER_VERSION
    );
    let migrated_context = upgraded
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id='context-v6'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("context payload");
    assert!(migrated_context.contains("contextSentinel"));
    assert_eq!(
        upgraded
            .query_row(
                "SELECT payload_json FROM diagnostic_assertions WHERE id='assertion-v6'",
                [],
                |row| row.get::<_, String>(0)
            )
            .expect("assertion payload"),
        "{\"assertionSentinel\":\"keep\"}"
    );
    drop(upgraded);
    fs::remove_dir_all(fresh_root).expect("cleanup fresh");
    fs::remove_dir_all(upgraded_root).expect("cleanup upgraded");

    let failed_root = fixture_root();
    initialize_v6_database(&failed_root);
    let project_id = graph::project_id(&failed_root);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v6");
    connection
        .execute(
            "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated, omissions_json)
             VALUES(1, 'graph-fail', ?1, 'revision-fail', 1, 0, '[]')",
            params![project_id],
        )
        .expect("failure graph");
    connection
        .execute(
            "INSERT INTO context_refs(uri, project_id, graph_id, graph_version, node_id, created_at,
                 origin_observation_id, origin_source_revision, origin_fingerprint, fingerprint_scope)
             VALUES('fp://failure', ?1, 'graph-fail', 1, 'node-failure', 1, '', '', '', '')",
            params![project_id],
        )
        .expect("failure Context Ref");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_v7_context_update BEFORE UPDATE ON context_refs
             BEGIN SELECT RAISE(ABORT, 'forced v7 migration failure'); END;",
        )
        .expect("failure trigger");
    drop(connection);
    assert!(open(&failed_root).is_err());
    let connection =
        rusqlite::Connection::open(database_path(&failed_root)).expect("reopen failed");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("failed version"),
        6
    );
    assert!(
        !table_columns_from_connection(&connection, "context_refs")
            .iter()
            .any(|column| column == "fingerprint_contract")
    );
    assert!(
        !table_columns_from_connection(&connection, "project_state")
            .iter()
            .any(|column| column == "current_event_id")
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='observation_events'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("event table check"),
        0
    );
    drop(connection);
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}
