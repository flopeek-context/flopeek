//! SQLite schema initialization and migrations.

#[allow(unused_imports)]
use super::*;

pub(super) fn initialize_schema(connection: &mut Connection) -> Result<(), String> {
    let mut version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Unable to read SQLite schema version: {error}"))?;
    if version > CURRENT_USER_VERSION {
        return Err(format!(
            "SQLite database schema version {version} is newer than supported version {CURRENT_USER_VERSION}."
        ));
    }

    while version < CURRENT_USER_VERSION {
        let target = version + 1;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| {
                format!("Unable to begin SQLite migration {version}->{target}: {error}")
            })?;
        match target {
            1 => migration_v1(&transaction)?,
            2 => migration_v2(&transaction)?,
            3 => migration_v3(&transaction)?,
            4 => migration_v4(&transaction)?,
            5 => migration_v5(&transaction)?,
            6 => migration_v6(&transaction)?,
            _ => unreachable!("migration target is bounded by CURRENT_USER_VERSION"),
        }
        transaction
            .execute_batch(&format!("PRAGMA user_version = {target};"))
            .map_err(|error| {
                format!("Unable to record SQLite migration version {target}: {error}")
            })?;
        transaction.commit().map_err(|error| {
            format!("Unable to commit SQLite migration {version}->{target}: {error}")
        })?;
        version = target;
    }
    Ok(())
}

pub(super) fn migration_v1(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS product_metadata (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS graph_versions (
                 graph_version INTEGER PRIMARY KEY NOT NULL,
                 graph_id TEXT NOT NULL UNIQUE,
                 project_id TEXT NOT NULL,
                 source_revision TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
                 omissions_json TEXT NOT NULL DEFAULT '[]'
             );
             CREATE TABLE IF NOT EXISTS source_files (
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 path TEXT NOT NULL,
                 language TEXT NOT NULL CHECK (language IN ('typescript', 'tsx')),
                 bytes INTEGER NOT NULL,
                 hash TEXT NOT NULL,
                 facts_json TEXT NOT NULL,
                 PRIMARY KEY (graph_version, path)
             );
             CREATE TABLE IF NOT EXISTS graph_nodes (
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 node_id TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 path TEXT,
                 name TEXT,
                 language TEXT,
                 PRIMARY KEY (graph_version, node_id)
             );
             CREATE TABLE IF NOT EXISTS graph_edges (
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 from_id TEXT NOT NULL,
                 to_id TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 evidence TEXT NOT NULL,
                 PRIMARY KEY (graph_version, from_id, to_id, kind, evidence)
             );
             CREATE TABLE IF NOT EXISTS context_refs (
                 uri TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 graph_id TEXT NOT NULL,
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 node_id TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS diagnostic_contexts (
                 id TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS diagnostic_assertions (
                 id TEXT PRIMARY KEY NOT NULL,
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 revision INTEGER NOT NULL,
                 kind TEXT NOT NULL,
                 status TEXT NOT NULL,
                 actor TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS historical_candidates (
                 id TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );",
        )
        .map_err(|error| format!("Unable to initialize SQLite base schema: {error}"))
}

pub(super) fn migration_v2(transaction: &Transaction<'_>) -> Result<(), String> {
    if !table_columns(transaction, "graph_versions")?
        .iter()
        .any(|column| column == "omissions_json")
    {
        transaction
            .execute(
                "ALTER TABLE graph_versions ADD COLUMN omissions_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )
            .map_err(|error| format!("Unable to migrate graph_versions: {error}"))?;
    }
    transaction
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS graph_versions_project_idx ON graph_versions(project_id, graph_version);
             CREATE INDEX IF NOT EXISTS context_refs_project_idx ON context_refs(project_id, graph_version);
             CREATE INDEX IF NOT EXISTS diagnostic_assertions_context_idx ON diagnostic_assertions(context_id, revision);",
        )
        .map_err(|error| format!("Unable to create SQLite indexes: {error}"))
}

pub(super) fn migration_v3(transaction: &Transaction<'_>) -> Result<(), String> {
    if table_exists(transaction, "historical_candidates")? {
        let columns = table_columns(transaction, "historical_candidates")?;
        transaction
            .execute_batch(
                "ALTER TABLE historical_candidates RENAME TO historical_candidates_v2;
                 CREATE TABLE historical_candidates (
                     id TEXT PRIMARY KEY NOT NULL,
                     project_id TEXT NOT NULL,
                     context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                     graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                     payload_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );",
            )
            .map_err(|error| format!("Unable to rebuild historical_candidates: {error}"))?;
        if columns.iter().any(|column| column == "context_id") {
            transaction
                .execute(
                    "INSERT INTO historical_candidates(id, project_id, context_id, graph_version, payload_json, created_at)
                     SELECT old.id, old.project_id, old.context_id, old.graph_version, old.payload_json, old.created_at
                     FROM historical_candidates_v2 old
                     WHERE old.context_id IS NOT NULL
                       AND EXISTS (SELECT 1 FROM diagnostic_contexts context WHERE context.id = old.context_id)
                       AND EXISTS (SELECT 1 FROM graph_versions graph WHERE graph.graph_version = old.graph_version)",
                    [],
                )
                .map_err(|error| format!("Unable to retain valid historical candidates: {error}"))?;
        }
        transaction
            .execute_batch("DROP TABLE historical_candidates_v2;")
            .map_err(|error| {
                format!("Unable to remove derived historical candidate backup: {error}")
            })?;
    } else {
        transaction
            .execute_batch(
                "CREATE TABLE historical_candidates (
                     id TEXT PRIMARY KEY NOT NULL,
                     project_id TEXT NOT NULL,
                     context_id TEXT NOT NULL REFERENCES diagnostic_contexts(id),
                     graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                     payload_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );",
            )
            .map_err(|error| format!("Unable to create historical_candidates: {error}"))?;
    }
    transaction
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS historical_candidates_context_idx
                 ON historical_candidates(context_id, graph_version);",
        )
        .map_err(|error| format!("Unable to create historical candidate index: {error}"))
}

pub(super) fn migration_v4(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "graph_nodes",
        "evidence_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "context_refs",
        "origin_observation_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "context_refs",
        "origin_source_revision",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "context_refs",
        "origin_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "context_refs",
        "fingerprint_scope",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS graph_observations (
                 observation_id TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 git_revision TEXT NOT NULL,
                 source_fingerprint TEXT NOT NULL,
                 source_manifest_json TEXT NOT NULL DEFAULT '[]',
                 dirty INTEGER NOT NULL CHECK (dirty IN (0, 1)),
                 observed_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS graph_observations_project_idx
                 ON graph_observations(project_id, observed_at, graph_version);
             CREATE TABLE IF NOT EXISTS project_state (
                 project_id TEXT PRIMARY KEY NOT NULL,
                 current_observation_id TEXT NOT NULL REFERENCES graph_observations(observation_id)
             );
             CREATE INDEX IF NOT EXISTS context_refs_origin_idx
                 ON context_refs(project_id, graph_version, node_id);",
        )
        .map_err(|error| format!("Unable to initialize observation schema: {error}"))?;
    add_column(
        transaction,
        "graph_observations",
        "source_manifest_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;

    let graph_rows = {
        let mut statement = transaction
            .prepare(
                "SELECT graph_version, graph_id, project_id, source_revision
                 FROM graph_versions ORDER BY graph_version",
            )
            .map_err(|error| format!("Unable to inspect graph observations: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("Unable to enumerate graph observations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode graph observations: {error}"))?
    };
    for (graph_version, graph_id, project_id, source_revision) in graph_rows {
        let source_rows = {
            let mut statement = transaction
                .prepare(
                    "SELECT path, language, bytes, hash FROM source_files
                     WHERE graph_version = ?1 ORDER BY path",
                )
                .map_err(|error| format!("Unable to inspect source observation: {error}"))?;
            statement
                .query_map(params![graph_version], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| format!("Unable to enumerate source observation: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("Unable to decode source observation: {error}"))?
        };
        let source_fingerprint = blake3::hash(
            &serde_json::to_vec(&source_rows)
                .map_err(|error| format!("Unable to encode source observation: {error}"))?,
        )
        .to_hex()
        .to_string();
        let source_manifest = source_rows
            .iter()
            .map(|(path, language, bytes, hash)| SourceFile {
                path: path.clone(),
                language: language.clone(),
                bytes: *bytes as u64,
                hash: hash.clone(),
            })
            .collect::<Vec<_>>();
        let source_manifest_json = serde_json::to_string(&source_manifest)
            .map_err(|error| format!("Unable to encode source observation manifest: {error}"))?;
        let dirty = source_revision.ends_with("+dirty");
        let git_revision = source_revision
            .strip_suffix("+dirty")
            .unwrap_or(&source_revision)
            .to_string();
        let observation_id = legacy_observation_id(
            &project_id,
            &source_revision,
            &source_fingerprint,
            &graph_id,
        );
        transaction
            .execute(
                "INSERT OR IGNORE INTO graph_observations(
                    observation_id, project_id, graph_version, git_revision,
                    source_fingerprint, source_manifest_json, dirty, observed_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    observation_id,
                    project_id,
                    graph_version,
                    git_revision,
                    source_fingerprint,
                    source_manifest_json,
                    i64::from(dirty),
                    now_seconds()
                ],
            )
            .map_err(|error| format!("Unable to backfill graph observation: {error}"))?;
    }

    transaction
        .execute_batch(
            "UPDATE context_refs
             SET origin_observation_id = COALESCE((
                 SELECT observation_id FROM graph_observations observation
                 WHERE observation.project_id = context_refs.project_id
                   AND observation.graph_version = context_refs.graph_version
             ), ''),
                 origin_source_revision = COALESCE((
                 SELECT git_revision FROM graph_observations observation
                 WHERE observation.project_id = context_refs.project_id
                   AND observation.graph_version = context_refs.graph_version
             ), ''),
                 fingerprint_scope = CASE
                     WHEN EXISTS (
                         SELECT 1 FROM graph_nodes node
                         WHERE node.graph_version = context_refs.graph_version
                           AND node.node_id = context_refs.node_id
                           AND node.evidence_fingerprint <> ''
                     ) THEN 'ast-and-direct-edges'
                     ELSE 'legacy-file-v1'
                 END,
                 origin_fingerprint = COALESCE((
                     SELECT NULLIF(node.evidence_fingerprint, '')
                     FROM graph_nodes node
                     WHERE node.graph_version = context_refs.graph_version
                       AND node.node_id = context_refs.node_id
                 ), COALESCE((
                     SELECT source.hash FROM source_files source
                     JOIN graph_nodes node ON node.graph_version = source.graph_version
                         AND node.path = source.path
                     WHERE node.graph_version = context_refs.graph_version
                       AND node.node_id = context_refs.node_id
                 ), ''));
             DELETE FROM historical_candidates;",
        )
        .map_err(|error| format!("Unable to backfill Context Ref provenance: {error}"))?;

    let projects = {
        let mut statement = transaction
            .prepare("SELECT DISTINCT project_id FROM graph_observations ORDER BY project_id")
            .map_err(|error| format!("Unable to inspect project observations: {error}"))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("Unable to enumerate project observations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode project observations: {error}"))?
    };
    for project in projects {
        transaction
            .execute(
                "INSERT INTO project_state(project_id, current_observation_id)
                 SELECT ?1, observation_id FROM graph_observations
                 WHERE project_id = ?1 ORDER BY observed_at DESC, graph_version DESC LIMIT 1
                 ON CONFLICT(project_id) DO UPDATE SET current_observation_id = excluded.current_observation_id",
                params![project],
            )
            .map_err(|error| format!("Unable to backfill current project observation: {error}"))?;
    }
    migrate_context_payloads(transaction)
}

pub(super) fn migration_v5(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "graph_observations",
        "module_resolution_status",
        "TEXT NOT NULL DEFAULT 'legacy-config-basis-unavailable'",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "module_resolution_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "module_resolution_effective_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "module_resolution_manifest_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )
}

fn migration_v6(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "graph_observations",
        "entry_manifest_status",
        "TEXT NOT NULL DEFAULT 'legacy-entry-basis-unavailable'",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "entry_manifest_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "entry_effective_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "entry_manifest_json",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS graph_flow_evidence (
                 graph_version INTEGER PRIMARY KEY NOT NULL REFERENCES graph_versions(graph_version),
                 entry_json TEXT NOT NULL,
                 related_tests_json TEXT NOT NULL,
                 truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
                 omissions_json TEXT NOT NULL DEFAULT '[]'
             );
             CREATE TABLE IF NOT EXISTS graph_flows (
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 flow_id TEXT NOT NULL,
                 fingerprint TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 PRIMARY KEY(graph_version, flow_id)
             );
             CREATE TABLE IF NOT EXISTS flow_refs (
                 uri TEXT PRIMARY KEY NOT NULL,
                 project_id TEXT NOT NULL,
                 graph_id TEXT NOT NULL,
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 flow_id TEXT NOT NULL,
                 origin_observation_id TEXT NOT NULL,
                 origin_source_revision TEXT NOT NULL,
                 origin_fingerprint TEXT NOT NULL,
                 fingerprint_scope TEXT NOT NULL,
                 freshness_reason TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS graph_flows_id_idx ON graph_flows(flow_id, graph_version);
             CREATE INDEX IF NOT EXISTS flow_refs_project_idx ON flow_refs(project_id, graph_version, flow_id);",
        )
        .map_err(|error| format!("Unable to initialize flow evidence schema: {error}"))?;
    migrate_context_flow_refs(transaction)
}

fn migrate_context_flow_refs(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts")
            .map_err(|error| format!("Unable to inspect Diagnostic Context payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Context payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Context payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schemaVersion".to_string(),
                serde_json::json!(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA),
            );
            object
                .entry("focusFlowRefs")
                .or_insert_with(|| serde_json::json!([]));
            let encoded = serde_json::to_string(&value).map_err(|error| {
                format!("Unable to encode Diagnostic Context migration: {error}")
            })?;
            transaction
                .execute(
                    "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
                    params![encoded, id],
                )
                .map_err(|error| format!("Unable to migrate Diagnostic Context {id}: {error}"))?;
        }
    }
    Ok(())
}

fn add_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if !table_columns(transaction, table)?
        .iter()
        .any(|name| name == column)
    {
        transaction
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|error| format!("Unable to migrate {table}.{column}: {error}"))?;
    }
    Ok(())
}

pub(super) fn observation_id(
    project_id: &str,
    source_revision: &str,
    source_fingerprint: &str,
    module_resolution_fingerprint: &str,
    entry_manifest_fingerprint: &str,
    graph_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-v3\0{project_id}\0{source_revision}\0{source_fingerprint}\0{module_resolution_fingerprint}\0{entry_manifest_fingerprint}\0{graph_id}"
    );
    format!("observation_{}", blake3::hash(input.as_bytes()).to_hex())
}

fn legacy_observation_id(
    project_id: &str,
    source_revision: &str,
    source_fingerprint: &str,
    graph_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-v1\0{project_id}\0{source_revision}\0{source_fingerprint}\0{graph_id}"
    );
    format!("observation_{}", blake3::hash(input.as_bytes()).to_hex())
}

fn migrate_context_payloads(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_contexts")
            .map_err(|error| format!("Unable to inspect Diagnostic Context payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Context payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Context payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        let Some(basis) = value
            .get_mut("currentGraphBasis")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let Some(graph_version) = basis
            .get("graphVersion")
            .and_then(serde_json::Value::as_i64)
        else {
            continue;
        };
        let observation = transaction
            .query_row(
                "SELECT observation_id FROM graph_observations WHERE graph_version = ?1 ORDER BY observed_at DESC LIMIT 1",
                params![graph_version],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to resolve Diagnostic Context observation: {error}"))?;
        basis.insert(
            "observationId".to_string(),
            serde_json::Value::String(observation.unwrap_or_default()),
        );
        if let Some(schema) = value.get_mut("schemaVersion") {
            *schema =
                serde_json::Value::String(crate::model::DIAGNOSTIC_CONTEXT_SCHEMA.to_string());
        }
        let updated = serde_json::to_string(&value)
            .map_err(|error| format!("Unable to encode migrated Diagnostic Context: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
                params![updated, id],
            )
            .map_err(|error| format!("Unable to persist migrated Diagnostic Context: {error}"))?;
    }
    migrate_assertion_payloads(transaction)
}

fn migrate_assertion_payloads(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT id, payload_json FROM diagnostic_assertions")
            .map_err(|error| format!("Unable to inspect Diagnostic Assertion payloads: {error}"))?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to enumerate Diagnostic Assertion payloads: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode Diagnostic Assertion payloads: {error}"))?
    };
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        if let Some(schema) = value.get_mut("schemaVersion") {
            *schema =
                serde_json::Value::String(crate::model::DIAGNOSTIC_ASSERTION_SCHEMA.to_string());
        }
        let updated = serde_json::to_string(&value)
            .map_err(|error| format!("Unable to encode migrated Diagnostic Assertion: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_assertions SET payload_json = ?1 WHERE id = ?2",
                params![updated, id],
            )
            .map_err(|error| format!("Unable to persist migrated Diagnostic Assertion: {error}"))?;
    }
    Ok(())
}

fn table_exists(transaction: &Transaction<'_>, table: &str) -> Result<bool, String> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(|error| format!("Unable to inspect SQLite table {table}: {error}"))
}

pub(super) fn table_columns(
    transaction: &Transaction<'_>,
    table: &str,
) -> Result<Vec<String>, String> {
    let mut statement = transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Unable to inspect {table} schema: {error}"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Unable to inspect {table} columns: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode {table} columns: {error}"))
}
