use super::*;

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
