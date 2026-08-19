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
fn observation_delta_reports_adjacent_changes_and_zero_bounds() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build A");
    let first = persist_scan(&root, snapshot, &facts).expect("persist A");
    let root_event = get_observation_continuity(&root, 128)
        .expect("continuity A")
        .current_event_id
        .expect("root event");
    let unavailable = get_observation_delta(
        &root,
        Some(&root_event),
        crate::temporal::DeltaLimits::default(),
    )
    .expect("root delta");
    assert_eq!(unavailable.status, "unavailable");
    assert_eq!(unavailable.reason, "predecessor-event-unavailable");
    assert_eq!(
        unavailable
            .to_basis
            .as_ref()
            .map(|basis| basis.observation_id.as_str()),
        Some(first.graph.observation_id.as_str())
    );

    fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("write B");
    let (snapshot, facts) = graph::build(&root).expect("build B");
    persist_scan(&root, snapshot, &facts).expect("persist B");
    let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("delta B");
    assert_eq!(delta.status, "complete");
    assert_eq!(delta.graph_relation, "structural-graph-changed");
    assert_eq!(delta.relation, "observed-after");
    assert!(delta.counts.source_changed >= 1);
    assert!(delta.counts.node_changed >= 1);
    assert!(
        delta
            .node_changes
            .iter()
            .any(|change| change.status == "changed")
    );

    let zero = get_observation_delta(
        &root,
        None,
        crate::temporal::DeltaLimits {
            max_source_changes: 0,
            max_node_changes: 0,
            max_edge_changes: 0,
            max_flow_changes: 0,
        },
    )
    .expect("zero-bound delta");
    assert_eq!(zero.status, "truncated");
    assert!(zero.truncated);
    assert_eq!(zero.source_changes.len(), 0);
    assert!(zero.counts.source_changed >= 1);
    assert!(
        zero.omissions
            .iter()
            .any(|omission| omission.contains("source changes"))
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn observation_delta_preserves_same_graph_and_a_b_a_adjacency() {
    let root = fixture_root();
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "src/main.ts"]);
    git(&root, &["commit", "-m", "source A"]);
    let (snapshot, facts) = graph::build(&root).expect("build A");
    let first = persist_scan(&root, snapshot, &facts).expect("persist A");
    fs::write(root.join("README.md"), "documentation-only\n").expect("README");
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-m", "README only"]);
    let (snapshot, facts) = graph::build(&root).expect("build README");
    let second = persist_scan(&root, snapshot, &facts).expect("persist README");
    assert_eq!(first.graph.graph_id, second.graph.graph_id);
    let same = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("same graph delta");
    assert_eq!(same.status, "complete");
    assert_eq!(same.reason, "same-structural-graph");
    assert_eq!(same.graph_relation, "same-structural-graph");
    assert_eq!(same.counts, crate::model::ObservationDeltaCounts::default());

    fs::write(root.join("src/main.ts"), "export const main = 3;\n").expect("write B");
    let (snapshot, facts) = graph::build(&root).expect("build B");
    let third = persist_scan(&root, snapshot, &facts).expect("persist B");
    fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write A");
    let (snapshot, facts) = graph::build(&root).expect("build A again");
    let fourth = persist_scan(&root, snapshot, &facts).expect("persist A again");
    let continuity = get_observation_continuity(&root, 128).expect("continuity");
    assert_eq!(continuity.events.len(), 4);
    let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("A delta");
    assert_eq!(
        delta.from_event_id,
        Some(continuity.events[2].event_id.clone())
    );
    assert_eq!(
        delta.to_event_id,
        Some(continuity.events[3].event_id.clone())
    );
    assert_eq!(
        delta.to_basis.as_ref().map(|basis| basis.graph_version),
        Some(fourth.graph.graph_version)
    );
    assert_ne!(third.graph.graph_id, fourth.graph.graph_id);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn observation_delta_uses_immutable_manifest_for_comment_only_source_changes() {
    let root = fixture_root();
    let (snapshot, facts_a) = graph::build(&root).expect("build A");
    let first = persist_scan(&root, snapshot, &facts_a).expect("persist A");
    fs::write(
        root.join("src/main.ts"),
        "export const main = 1;\n// observation-only comment\n",
    )
    .expect("comment-only source");
    let (snapshot, facts_b) = graph::build(&root).expect("build B");
    let second = persist_scan(&root, snapshot, &facts_b).expect("persist B");
    assert_eq!(first.graph.graph_id, second.graph.graph_id);
    assert_eq!(first.graph.graph_version, second.graph.graph_version);

    let connection = open(&root).expect("open");
    let manifest_for = |observation_id: &str| {
        connection
            .query_row(
                "SELECT source_manifest_json FROM graph_observations
                 WHERE observation_id = ?1",
                params![observation_id],
                |row| row.get::<_, String>(0),
            )
            .expect("manifest value")
    };
    let first_files = serde_json::from_str::<Vec<crate::model::SourceFile>>(&manifest_for(
        &first.graph.observation_id,
    ))
    .expect("first manifest");
    let second_files = serde_json::from_str::<Vec<crate::model::SourceFile>>(&manifest_for(
        &second.graph.observation_id,
    ))
    .expect("second manifest");
    assert_ne!(first_files, second_files);
    let first_facts_json = serde_json::to_string(&facts_a[0]).expect("first facts json");
    connection
        .execute(
            "UPDATE source_files SET facts_json = ?1",
            params![first_facts_json],
        )
        .expect("rematerialize stale graph facts");
    drop(connection);
    let current = current_graph(&root)
        .expect("current graph from observation manifest")
        .expect("current graph");
    assert_eq!(current.files, second_files);

    let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("comment-only delta");
    assert_eq!(delta.status, "complete");
    assert_eq!(delta.graph_relation, "same-structural-graph");
    assert_eq!(delta.basis_relations.typescript_source, "changed");
    assert_eq!(delta.counts.source_changed, 1);
    assert_eq!(delta.counts.node_added, 0);
    assert_eq!(delta.counts.node_changed, 0);
    assert_eq!(delta.counts.node_removed, 0);
    assert_eq!(delta.counts.edge_added, 0);
    assert_eq!(delta.counts.edge_removed, 0);
    assert_eq!(delta.counts.flow_added, 0);
    assert_eq!(delta.counts.flow_changed, 0);
    assert_eq!(delta.counts.flow_removed, 0);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn invalid_observation_manifests_are_unavailable_without_structural_guessing() {
    for invalid_manifest in [
        "not-json".to_string(),
        r#"[{"path":"src/main.ts","language":"typescript","bytes":1,"hash":"a"},{"path":"src/main.ts","language":"typescript","bytes":1,"hash":"b"}]"#.to_string(),
        r#"[{"path":"/absolute.ts","language":"typescript","bytes":1,"hash":"a"}]"#.to_string(),
    ] {
        let root = fixture_root();
        let (snapshot, facts) = graph::build(&root).expect("build A");
        persist_scan(&root, snapshot, &facts).expect("persist A");
        fs::write(root.join("README.md"), "observation B\n").expect("README");
        let (snapshot, facts) = graph::build(&root).expect("build B");
        persist_scan(&root, snapshot, &facts).expect("persist B");
        let connection = open(&root).expect("open");
        let predecessor = connection
            .query_row(
                "SELECT observation_id FROM graph_observations ORDER BY observed_at, observation_id LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("predecessor observation");
        connection
            .execute(
                "UPDATE graph_observations SET source_manifest_json = ?1 WHERE observation_id = ?2",
                params![invalid_manifest, predecessor],
            )
            .expect("corrupt source manifest");
        drop(connection);
        let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
            .expect("invalid manifest delta");
        assert_eq!(delta.status, "unavailable");
        assert_eq!(delta.reason, "observation-source-manifest-invalid");
        assert_eq!(delta.graph_relation, "unavailable");
        fs::remove_dir_all(root).expect("cleanup");
    }
}

#[test]
fn observation_delta_rejects_legacy_contracts_and_wrong_project_events() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build A");
    persist_scan(&root, snapshot, &facts).expect("persist A");
    fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("write B");
    let (snapshot, facts) = graph::build(&root).expect("build B");
    persist_scan(&root, snapshot, &facts).expect("persist B");
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE graph_versions
             SET graph_derivation_id = ?1
             WHERE graph_version = (SELECT MIN(graph_version) FROM graph_versions)",
            params![crate::temporal::LEGACY_EVIDENCE_CONTRACT],
        )
        .expect("legacy graph contract");
    drop(connection);
    let unavailable = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("legacy delta");
    assert_eq!(unavailable.status, "unavailable");
    assert_eq!(unavailable.reason, "incompatible-evidence-contract");
    assert!(unavailable.node_changes.is_empty());

    let connection = open(&root).expect("reopen");
    let observation_id = connection
        .query_row(
            "SELECT observation_id FROM graph_observations ORDER BY observed_at LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("observation");
    connection
        .execute(
            "INSERT INTO observation_events(
                event_id, project_id, observation_id, predecessor_event_id, observed_at
             ) VALUES('foreign-event', 'foreign-project', ?1, NULL, 1)",
            params![observation_id],
        )
        .expect("foreign event");
    drop(connection);
    let wrong_project = get_observation_delta(
        &root,
        Some("foreign-event"),
        crate::temporal::DeltaLimits::default(),
    )
    .expect("wrong project delta");
    assert_eq!(wrong_project.status, "wrong-project");
    assert_eq!(wrong_project.reason, "wrong-project-event");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn observation_delta_reports_corrupt_contract_metadata_explicitly() {
    let root = fixture_root();
    git(&root, &["init"]);
    git(
        &root,
        &["config", "user.email", "flopeek-test@example.invalid"],
    );
    git(&root, &["config", "user.name", "Flopeek Test"]);
    git(&root, &["add", "src/main.ts"]);
    git(&root, &["commit", "-m", "source A"]);
    let (snapshot, facts) = graph::build(&root).expect("build A");
    persist_scan(&root, snapshot, &facts).expect("persist A");
    fs::write(root.join("README.md"), "documentation-only\n").expect("README");
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-m", "README only"]);
    let (snapshot, facts) = graph::build(&root).expect("build README");
    persist_scan(&root, snapshot, &facts).expect("persist README");
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE graph_versions SET graph_schema_version = ''
             WHERE graph_version = (SELECT MAX(graph_version) FROM graph_versions)",
            [],
        )
        .expect("corrupt contract");
    drop(connection);
    let delta = get_observation_delta(&root, None, crate::temporal::DeltaLimits::default())
        .expect("corrupt delta");
    assert_eq!(delta.status, "unavailable");
    assert_eq!(delta.reason, "evidence-contract-unavailable");
    assert_eq!(delta.graph_relation, "unavailable");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn moved_exact_node_is_a_stale_candidate_and_ambiguous_successors_stay_stale() {
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
    assert_eq!(moved.status, "stale");
    assert_eq!(
        moved.freshness_reason,
        "unique-exact-compatible-fingerprint-candidate"
    );
    assert!(moved.successor_uri.is_none());
    assert_eq!(moved.origin_observation_id, origin.origin_observation_id);
    let reconciliation = reconcile_context(&root, &origin.uri).expect("reconcile moved origin");
    assert_eq!(
        reconciliation.schema_version,
        crate::model::CONTEXT_RECONCILIATION_SCHEMA
    );
    assert_eq!(reconciliation.status, "stale");
    assert_eq!(reconciliation.candidates.len(), 1);
    assert!(reconciliation.successor.is_none());

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
fn preexisting_identical_nodes_remain_stale_candidates_without_successor_proof() {
    let root = fixture_root();
    fs::remove_file(root.join("src/main.ts")).expect("remove default source");
    fs::write(root.join("src/a.ts"), "export const value = 1;\n").expect("write a");
    fs::write(root.join("src/b.ts"), "export const value = 1;\n").expect("write b");
    let (snapshot, facts) = graph::build(&root).expect("build origin");
    let first = persist_scan(&root, snapshot, &facts).expect("persist origin");
    let origin = first
        .context_refs
        .iter()
        .find(|reference| {
            first.graph.nodes.iter().any(|node| {
                node.id == reference.node_id
                    && node.path.as_deref() == Some("src/a.ts")
                    && node.name.as_deref() == Some("value")
            })
        })
        .expect("a value ref")
        .clone();
    fs::remove_file(root.join("src/a.ts")).expect("remove a");
    let (snapshot, facts) = graph::build(&root).expect("build successor");
    persist_scan(&root, snapshot, &facts).expect("persist successor");

    let resolved = resolve_context(&root, &origin.uri).expect("resolve stale origin");
    assert_eq!(resolved.status, "stale");
    assert_eq!(
        resolved.freshness_reason,
        "unique-exact-compatible-fingerprint-candidate"
    );
    assert!(resolved.successor_uri.is_none());
    let reconciliation = reconcile_context(&root, &origin.uri).expect("reconcile stale origin");
    assert_eq!(reconciliation.status, "stale");
    assert_eq!(
        reconciliation.reason,
        "unique-exact-compatible-fingerprint-candidate"
    );
    assert!(reconciliation.successor.is_none());
    assert_eq!(reconciliation.candidates.len(), 1);
    let candidate =
        resolve_context(&root, &reconciliation.candidates[0]).expect("resolve candidate");
    assert_eq!(candidate.status, "current");
    let connection = open(&root).expect("open candidate");
    let candidate_path = connection
        .query_row(
            "SELECT path FROM graph_nodes WHERE graph_version = ?1 AND node_id = ?2",
            params![candidate.graph_version as i64, candidate.node_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("candidate path");
    assert_eq!(candidate_path.as_deref(), Some("src/b.ts"));
    drop(connection);
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
fn fresh_and_upgraded_v8_schema_match_and_migration_failure_rolls_back() {
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
    initialize_v7_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v7");
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
    let contract = upgraded
        .query_row(
            "SELECT graph_schema_version, graph_derivation_id, node_fingerprint_contract
             FROM graph_versions WHERE graph_version = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("legacy contract");
    assert_eq!(contract.0, crate::temporal::LEGACY_EVIDENCE_CONTRACT);
    assert_eq!(contract.1, crate::temporal::LEGACY_EVIDENCE_CONTRACT);
    assert_eq!(contract.2, crate::temporal::LEGACY_EVIDENCE_CONTRACT);
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
    initialize_v7_database(&failed_root);
    let project_id = graph::project_id(&failed_root);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v7");
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
            "CREATE TRIGGER fail_v8_graph_update BEFORE UPDATE ON graph_versions
             BEGIN SELECT RAISE(ABORT, 'forced v8 migration failure'); END;",
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
        7
    );
    assert!(
        !table_columns_from_connection(&connection, "graph_versions")
            .iter()
            .any(|column| column == "graph_schema_version")
    );
    drop(connection);
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}
