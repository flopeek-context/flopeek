use super::*;

#[test]
fn fresh_and_upgraded_v12_schema_match_and_migration_failure_rolls_back() {
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
    initialize_v5_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v5");
    connection
        .execute(
            "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated, omissions_json)
             VALUES(1, 'graph-v5', ?1, 'revision-v5', 1, 0, '[]')",
            params![project_id],
        )
        .expect("graph row");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-v5', ?1, 1, '{\"contextSentinel\":\"keep\"}', 1)",
            params![project_id],
        )
        .expect("context row");
    connection
        .execute(
            "INSERT INTO diagnostic_assertions(id, context_id, revision, kind, status, actor, payload_json, created_at)
             VALUES('assertion-v5', 'context-v5', 1, 'observation', 'proposed', 'test', '{\"assertionSentinel\":\"keep\"}', 1)",
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
    for table in [
        "last_known_good_candidates",
        "last_known_good_events",
        "last_known_good_state",
    ] {
        assert_eq!(
            upgraded
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    params![table],
                    |row| row.get::<_, i64>(0),
                )
                .expect("LKG table"),
            1,
            "missing canonical LKG table {table}"
        );
    }
    let migrated_context = upgraded
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id='context-v5'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("context payload");
    assert!(migrated_context.contains("contextSentinel"));
    assert!(migrated_context.contains(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA));
    assert!(migrated_context.contains("focusFlowRefs"));
    assert_eq!(
        upgraded
            .query_row(
                "SELECT payload_json FROM diagnostic_assertions WHERE id='assertion-v5'",
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
    initialize_v5_database(&failed_root);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v5");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-fail', ?1, 1, '{}', 1)",
            params![graph::project_id(&failed_root)],
        )
        .expect("failure context");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_v6_context_update BEFORE UPDATE ON diagnostic_contexts
             BEGIN SELECT RAISE(ABORT, 'forced v6 migration failure'); END;",
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
        5
    );
    let columns = table_columns_from_connection(&connection, "graph_observations");
    assert!(
        !columns
            .iter()
            .any(|column| column == "entry_manifest_status")
    );
    assert!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='flow_refs'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("flow table check")
            == 0
    );
    drop(connection);
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}

#[test]
fn v12_to_v13_backfills_context_basis_memory_and_candidates_transactionally() {
    let fresh_root = fixture_root();
    let fresh = open(&fresh_root).expect("fresh v13");
    let fresh_schema = schema_snapshot(&fresh);
    drop(fresh);

    let upgraded_root = fixture_root();
    initialize_v12_database(&upgraded_root);
    let project_id = graph::project_id(&upgraded_root);
    let legacy_context = serde_json::json!({
        "schemaVersion": "flopeek-diagnostic-context/v6",
        "id": "context-v13",
        "projectId": project_id,
        "revision": 9,
        "intent": "diagnose",
        "symptom": "timeout",
        "expectedBehavior": "completes",
        "focusContextRefs": [],
        "focusFlowRefs": [],
        "currentGraphBasis": {
            "projectId": project_id,
            "graphId": "graph-v13",
            "graphVersion": 1,
            "sourceRevision": "0000000000000000000000000000000000000000",
            "observationId": "observation-v13"
        },
        "lastKnownGoodBasis": null,
        "lastKnownGoodBindingId": null,
        "lastKnownGoodCandidateId": null,
        "constraints": ["bounded"],
        "acceptanceCriteria": ["verified"],
        "unresolvedQuestions": [],
        "actor": "migration-test",
        "createdAt": 1,
        "status": "open",
        "supersedes": null
    });
    let expected = crate::model::expected_behavior_fingerprint("completes");
    let legacy_candidate = serde_json::json!({
        "schemaVersion": "flopeek-last-known-good-candidate/v1",
        "candidateId": "candidate-v13",
        "repositoryId": "repo_123e4567-e89b-12d3-a456-426614174000",
        "projectId": project_id,
        "contextId": "context-v13",
        "contextRevision": 9,
        "expectedBehaviorFingerprint": expected,
        "gitRevision": "0000000000000000000000000000000000000000",
        "observationId": null,
        "graphBasis": null,
        "evidenceContract": null,
        "proposedBy": "agent",
        "proposedAt": 1,
        "evidence": [],
        "reason": "legacy candidate",
        "integrity": {
            "status": "partial",
            "revisionAvailable": true,
            "observationAvailable": false,
            "graphBasisAvailable": false,
            "evidenceContractCompatible": false,
            "limitations": ["legacy"]
        }
    });
    let connection = rusqlite::Connection::open(database_path(&upgraded_root)).expect("v12");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-v13', ?1, 9, ?2, 1)",
            params![project_id, legacy_context.to_string()],
        )
        .expect("legacy Context");
    connection
        .execute(
            "INSERT INTO diagnostic_assertions(id, context_id, revision, kind, status, actor, payload_json, created_at)
             VALUES('assertion-v13', 'context-v13', 3, 'observation', 'proposed', 'agent', '{}', 1)",
            [],
        )
        .expect("legacy assertion");
    connection
        .execute(
            "INSERT INTO last_known_good_candidates(
                 candidate_id, repository_id, project_id, context_id, context_revision,
                 expected_behavior_fingerprint, git_revision, proposed_by, proposed_at,
                 evidence_json, reason, integrity_json, payload_json
             ) VALUES('candidate-v13', 'repo_123e4567-e89b-12d3-a456-426614174000', ?1,
                      'context-v13', 9, ?2, '0000000000000000000000000000000000000000',
                      'agent', 1, '[]', 'legacy candidate', ?3, ?4)",
            params![
                project_id,
                expected,
                legacy_candidate["integrity"].to_string(),
                legacy_candidate.to_string()
            ],
        )
        .expect("legacy candidate");
    drop(connection);

    let upgraded = open(&upgraded_root).expect("upgrade v13");
    assert_eq!(schema_snapshot(&upgraded), fresh_schema);
    let context_payload: String = upgraded
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = 'context-v13'",
            [],
            |row| row.get(0),
        )
        .expect("Context payload");
    let context: crate::model::DiagnosticContext =
        serde_json::from_str(&context_payload).expect("Context v7");
    assert_eq!(context.context_definition_revision, 1);
    assert_eq!(context.memory_revision, 3);
    assert_eq!(
        context.context_basis_fingerprint,
        crate::model::diagnostic_context_basis_fingerprint(&context)
    );
    let candidate_payload: String = upgraded
        .query_row(
            "SELECT payload_json FROM last_known_good_candidates WHERE candidate_id = 'candidate-v13'",
            [],
            |row| row.get(0),
        )
        .expect("candidate payload");
    let candidate: crate::model::LastKnownGoodCandidate =
        serde_json::from_str(&candidate_payload).expect("candidate v2");
    assert_eq!(candidate.context_definition_revision, 1);
    assert_eq!(
        candidate.context_basis_fingerprint,
        context.context_basis_fingerprint
    );
    drop(upgraded);

    let failed_root = fixture_root();
    initialize_v12_database(&failed_root);
    let project_id = graph::project_id(&failed_root);
    let mut failure_context = legacy_context.clone();
    failure_context["id"] = serde_json::json!("context-v13-fail");
    failure_context["projectId"] = serde_json::json!(&project_id);
    failure_context["currentGraphBasis"]["projectId"] = serde_json::json!(&project_id);
    let connection = rusqlite::Connection::open(database_path(&failed_root)).expect("failed v12");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('context-v13-fail', ?1, 1, ?2, 1)",
            params![project_id, failure_context.to_string()],
        )
        .expect("failure Context");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_v13_context_update BEFORE UPDATE ON diagnostic_contexts
             BEGIN SELECT RAISE(ABORT, 'forced v13 failure'); END;",
        )
        .expect("failure trigger");
    drop(connection);
    assert!(open(&failed_root).is_err());
    let failed = rusqlite::Connection::open(database_path(&failed_root)).expect("reopen failed");
    assert_eq!(
        failed
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        12
    );
    assert!(
        !table_columns_from_connection(&failed, "diagnostic_contexts")
            .iter()
            .any(|column| column == "context_basis_fingerprint")
    );
    assert_eq!(
        failed
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='last_known_good_command_receipts'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("receipt table"),
        0
    );
    drop(failed);
    fs::remove_dir_all(fresh_root).expect("cleanup fresh");
    fs::remove_dir_all(upgraded_root).expect("cleanup upgraded");
    fs::remove_dir_all(failed_root).expect("cleanup failed");
}

#[test]
fn v11_to_v12_migration_failure_rolls_back_canonical_lkg_tables() {
    let root = fixture_root();
    initialize_v11_database(&root);
    let project_id = graph::project_id(&root);
    let connection = rusqlite::Connection::open(database_path(&root)).expect("v11 database");
    connection
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES('legacy-lkg-context', ?1, 1, '{}', 1)",
            params![project_id],
        )
        .expect("context");
    connection
        .execute(
            "INSERT INTO last_known_good_bindings(
                 binding_id, repository_id, project_id, context_id, git_revision,
                 actor, actor_kind, evidence_json, status, payload_json, created_at
             ) VALUES('legacy-invalid', 'repo', ?1, 'legacy-lkg-context', 'revision',
                      'agent', 'agent', '[]', 'proposed', '{}', 1)",
            params![project_id],
        )
        .expect("malformed legacy binding");
    drop(connection);

    assert!(open(&root).is_err(), "v12 must fail closed");
    let connection = rusqlite::Connection::open(database_path(&root)).expect("reopen v11");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        11
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'last_known_good_candidates'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("canonical table check"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT payload_json FROM last_known_good_bindings WHERE binding_id = 'legacy-invalid'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("legacy row"),
        "{}"
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

fn table_columns_from_connection(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("table column rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("table column values")
}

#[test]
fn migrates_graph_and_historical_columns_without_losing_the_database() {
    let root = fixture_root();
    fs::create_dir_all(root.join(STORE_DIRECTORY)).expect("store directory");
    let connection = rusqlite::Connection::open(database_path(&root)).expect("old sqlite");
    connection
        .execute_batch(
            "CREATE TABLE graph_versions (
                graph_version INTEGER PRIMARY KEY NOT NULL,
                graph_id TEXT NOT NULL UNIQUE,
                project_id TEXT NOT NULL,
                source_revision TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                truncated INTEGER NOT NULL
            );
            CREATE TABLE historical_candidates (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                graph_version INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );",
        )
        .expect("old schema");
    drop(connection);
    let connection = open(&root).expect("migrate");
    let graph_columns = connection
        .prepare("PRAGMA table_info(graph_versions)")
        .expect("graph columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("graph query")
        .collect::<Result<Vec<_>, _>>()
        .expect("graph names");
    let history_columns = connection
        .prepare("PRAGMA table_info(historical_candidates)")
        .expect("history columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("history query")
        .collect::<Result<Vec<_>, _>>()
        .expect("history names");
    assert!(
        graph_columns
            .iter()
            .any(|column| column == "omissions_json")
    );
    assert!(history_columns.iter().any(|column| column == "context_id"));
    let observation_columns = connection
        .prepare("PRAGMA table_info(graph_observations)")
        .expect("observation columns")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("observation query")
        .collect::<Result<Vec<_>, _>>()
        .expect("observation names");
    assert!(
        observation_columns
            .iter()
            .any(|column| column == "module_resolution_fingerprint")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        CURRENT_USER_VERSION
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}
