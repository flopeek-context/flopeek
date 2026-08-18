use super::*;

#[test]
fn failed_v3_to_v4_migration_preserves_user_version_and_existing_rows() {
    let root = fixture_root();
    let connection = open(&root).expect("fresh database");
    connection
        .execute(
            "INSERT INTO graph_versions(
                graph_version, graph_id, project_id, source_revision,
                created_at, truncated, omissions_json
             ) VALUES(1, 'graph-old', 'project-old', 'revision-old', 1, 0, '[]')",
            [],
        )
        .expect("old graph");
    connection
        .execute_batch(
            "PRAGMA user_version = 3;
             CREATE TRIGGER fail_observation_migration
             BEFORE INSERT ON graph_observations
             BEGIN SELECT RAISE(ABORT, 'forced migration failure'); END;",
        )
        .expect("prepare failed migration");
    drop(connection);
    assert!(open(&root).is_err());
    let connection = rusqlite::Connection::open(database_path(&root)).expect("inspect");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version"),
        3
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_versions", [], |row| row
                .get::<_, i64>(0))
            .expect("graph row"),
        1
    );
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn legacy_context_ref_is_unresolved_without_evidence_and_uses_file_fallback_with_hash() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let result = persist_scan(&root, snapshot, &facts).expect("persist");
    let reference = &result.context_refs[0];
    let connection = open(&root).expect("open");
    connection
        .execute(
            "UPDATE context_refs
             SET origin_observation_id = '', origin_fingerprint = '', fingerprint_scope = 'legacy-file-v1'
             WHERE uri = ?1",
            params![reference.uri],
        )
        .expect("legacy unresolved");
    drop(connection);
    assert_eq!(
        resolve_context(&root, &reference.uri)
            .expect("resolve unresolved")
            .status,
        "unresolved"
    );
    let (snapshot, facts) = graph::build(&root).expect("rebuild legacy graph");
    let rescanned = persist_scan(&root, snapshot, &facts).expect("persist unresolved legacy");
    assert_eq!(
        rescanned
            .context_refs
            .iter()
            .find(|candidate| candidate.uri == reference.uri)
            .expect("legacy Context Ref after rescan")
            .status,
        "unresolved"
    );
    let connection = open(&root).expect("reopen");
    let (observation_id, hash) = connection
        .query_row(
            "SELECT observation.observation_id, source.hash
             FROM graph_observations observation
             JOIN source_files source ON source.graph_version = observation.graph_version
             JOIN graph_nodes node ON node.graph_version = source.graph_version
                 AND node.path = source.path
             WHERE observation.graph_version = ?1 AND node.node_id = ?2 LIMIT 1",
            params![result.graph.graph_version as i64, reference.node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("legacy fallback evidence");
    connection
        .execute(
            "UPDATE context_refs
             SET origin_observation_id = ?1, origin_fingerprint = ?2, fingerprint_scope = 'legacy-file-v1'
             WHERE uri = ?3",
            params![observation_id, hash, reference.uri],
        )
        .expect("legacy evidence");
    drop(connection);
    assert_eq!(
        resolve_context(&root, &reference.uri)
            .expect("resolve legacy current")
            .status,
        "current"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn legacy_v4_facts_are_readable_but_resolution_evidence_is_unavailable() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    persist_scan(&root, snapshot, &facts).expect("persist");
    let connection = open(&root).expect("open");
    let facts_json = connection
        .query_row("SELECT facts_json FROM source_files LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("facts");
    let mut legacy: serde_json::Value = serde_json::from_str(&facts_json).expect("json");
    let object = legacy.as_object_mut().expect("facts object");
    object.remove("schema_version");
    object.remove("resolution_records");
    connection
        .execute(
            "UPDATE source_files SET facts_json = ?1",
            params![serde_json::to_string(&legacy).expect("legacy facts")],
        )
        .expect("write legacy facts");
    drop(connection);

    let current = current_graph(&root)
        .expect("read legacy graph")
        .expect("current graph");
    assert_eq!(current.schema_version, crate::model::GRAPH_SCHEMA);
    assert_eq!(current.resolution_evidence.status, "unavailable");
    assert!(
        current
            .resolution_evidence
            .omissions
            .iter()
            .any(|omission| omission == "legacy-facts-without-resolution-evidence")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corrupted_graph_rows_are_rebuilt_transactionally() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("persist");
    let connection = open(&root).expect("open");
    connection
        .execute(
            "DELETE FROM graph_nodes WHERE graph_version = ?1",
            params![first.graph.graph_version],
        )
        .expect("corrupt rows");
    drop(connection);
    let (snapshot, facts) = graph::build(&root).expect("build again");
    let recovered = persist_scan(&root, snapshot, &facts).expect("recover");
    assert_eq!(recovered.graph.graph_version, first.graph.graph_version);
    assert_eq!(
        status(&root).expect("status").node_count,
        recovered.graph.nodes.len() as u64
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn context_ref_failure_rolls_back_graph_rows_observation_and_current_state() {
    let root = fixture_root();
    let (snapshot, facts) = graph::build(&root).expect("build");
    let first = persist_scan(&root, snapshot, &facts).expect("first persist");
    fs::write(root.join("src/other.ts"), "export const other = 2;\n").expect("change");
    let (snapshot, facts) = graph::build(&root).expect("changed build");
    let connection = open(&root).expect("open");
    connection
        .execute_batch(
            "CREATE TRIGGER fail_context_ref BEFORE INSERT ON context_refs
             WHEN NEW.graph_version > 1
             BEGIN SELECT RAISE(ABORT, 'forced Context Ref failure'); END;",
        )
        .expect("trigger");
    drop(connection);
    assert!(persist_scan(&root, snapshot, &facts).is_err());
    let connection = open(&root).expect("reopen");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM graph_versions", [], |row| row
                .get::<_, i64>(0))
            .expect("graphs"),
        1
    );
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
    drop(connection);
    fs::remove_dir_all(root).expect("cleanup");
}
