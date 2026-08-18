//! SQLite authority for graph versions and Context Refs.
//!
//! Writes are transactional and facts contain hashes/structure only.  Source bodies
//! and credentials never enter this store.

use crate::context;
use crate::model::{
    ContextRef, GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, STORE_SCHEMA, ScanResult,
    SourceFile, StoreStatus, TypeScriptFacts,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const STORE_DIRECTORY: &str = ".flopeek";
pub const STORE_FILENAME: &str = "flopeek.sqlite3";
pub const CURRENT_USER_VERSION: i64 = 4;

pub fn database_path(root: &Path) -> PathBuf {
    root.join(STORE_DIRECTORY).join(STORE_FILENAME)
}

pub fn open(root: &Path) -> Result<Connection, String> {
    let directory = root.join(STORE_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Unable to create SQLite directory {}: {error}",
            directory.display()
        )
    })?;
    let path = database_path(root);
    let mut connection = Connection::open(&path)
        .map_err(|error| format!("Unable to open SQLite database {}: {error}", path.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("Unable to set SQLite busy timeout: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|error| format!("Unable to configure SQLite: {error}"))?;
    initialize_schema(&mut connection)?;
    Ok(connection)
}

fn initialize_schema(connection: &mut Connection) -> Result<(), String> {
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

fn migration_v1(transaction: &Transaction<'_>) -> Result<(), String> {
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

fn migration_v2(transaction: &Transaction<'_>) -> Result<(), String> {
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

fn migration_v3(transaction: &Transaction<'_>) -> Result<(), String> {
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

fn migration_v4(transaction: &Transaction<'_>) -> Result<(), String> {
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
        let observation_id = observation_id(
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

fn observation_id(
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

fn table_columns(transaction: &Transaction<'_>, table: &str) -> Result<Vec<String>, String> {
    let mut statement = transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Unable to inspect {table} schema: {error}"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Unable to inspect {table} columns: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode {table} columns: {error}"))
}

pub fn persist_scan(
    root: &Path,
    mut snapshot: GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<ScanResult, String> {
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin SQLite transaction: {error}"))?;
    transaction
        .execute(
            "INSERT INTO product_metadata(key, value) VALUES('product', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![PRODUCT_IDENTITY],
        )
        .map_err(|error| format!("Unable to record product identity: {error}"))?;
    transaction
        .execute(
            "INSERT INTO product_metadata(key, value) VALUES('project_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![snapshot.project_id],
        )
        .map_err(|error| format!("Unable to record project identity: {error}"))?;

    let existing = transaction
        .query_row(
            "SELECT graph_version FROM graph_versions WHERE graph_id = ?1",
            params![snapshot.graph_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read graph identity: {error}"))?;
    let graph_version = if let Some(version) = existing {
        let stored_counts = transaction
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM source_files WHERE graph_version = ?1),
                    (SELECT COUNT(*) FROM graph_nodes WHERE graph_version = ?1),
                    (SELECT COUNT(*) FROM graph_edges WHERE graph_version = ?1)",
                rusqlite::params![version],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(|error| format!("Unable to inspect reusable graph rows: {error}"))?;
        let expected_counts = (
            snapshot.files.len() as i64,
            snapshot.nodes.len() as i64,
            snapshot.edges.len() as i64,
        );
        if stored_counts != expected_counts {
            transaction
                .execute(
                    "DELETE FROM graph_edges WHERE graph_version = ?1",
                    rusqlite::params![version],
                )
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM graph_nodes WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM source_files WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .map_err(|error| format!("Unable to recover corrupted graph rows: {error}"))?;
            persist_graph_rows(&transaction, version, &snapshot, facts)?;
        }
        version as u64
    } else {
        let version = transaction
            .query_row(
                "SELECT COALESCE(MAX(graph_version), 0) + 1 FROM graph_versions",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("Unable to allocate graph version: {error}"))?;
        transaction
            .execute(
                "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated, omissions_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    version,
                    snapshot.graph_id,
                    snapshot.project_id,
                    snapshot.source_revision,
                    now_seconds(),
                    i64::from(snapshot.truncated),
                    serde_json::to_string(&snapshot.omissions)
                        .map_err(|error| format!("Unable to encode graph omissions: {error}"))?,
                ],
            )
            .map_err(|error| format!("Unable to persist graph version: {error}"))?;
        persist_graph_rows(&transaction, version, &snapshot, facts)?;
        version as u64
    };
    snapshot.graph_version = graph_version;
    let dirty = snapshot.source_revision.ends_with("+dirty");
    let git_revision = snapshot
        .source_revision
        .strip_suffix("+dirty")
        .unwrap_or(&snapshot.source_revision)
        .to_string();
    let observation = observation_id(
        &snapshot.project_id,
        &snapshot.source_revision,
        &snapshot.source_fingerprint,
        &snapshot.graph_id,
    );
    let source_manifest_json = serde_json::to_string(&snapshot.files)
        .map_err(|error| format!("Unable to encode graph observation manifest: {error}"))?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO graph_observations(
                observation_id, project_id, graph_version, git_revision,
                source_fingerprint, source_manifest_json, dirty, observed_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                observation,
                snapshot.project_id,
                graph_version as i64,
                git_revision,
                snapshot.source_fingerprint,
                source_manifest_json,
                i64::from(dirty),
                now_seconds()
            ],
        )
        .map_err(|error| format!("Unable to persist graph observation: {error}"))?;
    snapshot.observation_id = observation;
    transaction
        .execute(
            "INSERT INTO project_state(project_id, current_observation_id)
             VALUES(?1, ?2)
             ON CONFLICT(project_id) DO UPDATE SET current_observation_id = excluded.current_observation_id",
            params![snapshot.project_id, snapshot.observation_id],
        )
        .map_err(|error| format!("Unable to update current project observation: {error}"))?;
    let refs = context::for_snapshot(&transaction, &snapshot)?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit SQLite graph/context transaction: {error}"))?;
    Ok(ScanResult {
        schema_version: STORE_SCHEMA.to_string(),
        product: PRODUCT_IDENTITY.to_string(),
        project_id: snapshot.project_id.clone(),
        graph: snapshot,
        context_refs: refs,
        limitations: vec![
            "Evidence is static TypeScript/TSX syntax and Git identity; runtime behavior is unavailable.".to_string(),
            "Dynamic dispatch, reflection, generated files and package-manager execution are unsupported.".to_string(),
            "Historical change ranking is not inferred from this scan and must remain an explicit candidate.".to_string(),
        ],
    })
}

fn persist_graph_rows(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<(), String> {
    for file in &snapshot.files {
        let fact = facts
            .iter()
            .find(|fact| fact.path == file.path)
            .ok_or_else(|| format!("Missing facts for {}", file.path))?;
        let facts_json = serde_json::to_string(fact)
            .map_err(|error| format!("Unable to encode facts for {}: {error}", file.path))?;
        transaction
            .execute(
                "INSERT INTO source_files(graph_version, path, language, bytes, hash, facts_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    graph_version,
                    file.path,
                    file.language,
                    file.bytes as i64,
                    file.hash,
                    facts_json
                ],
            )
            .map_err(|error| {
                format!(
                    "Unable to persist source evidence for {}: {error}",
                    file.path
                )
            })?;
    }
    for node in &snapshot.nodes {
        transaction
            .execute(
                "INSERT INTO graph_nodes(graph_version, node_id, kind, path, name, language, evidence_fingerprint)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    graph_version,
                    node.id,
                    node.kind,
                    node.path,
                    node.name,
                    node.language,
                    node.evidence_fingerprint
                ],
            )
            .map_err(|error| format!("Unable to persist graph node {}: {error}", node.id))?;
    }
    for edge in &snapshot.edges {
        transaction
            .execute(
                "INSERT INTO graph_edges(graph_version, from_id, to_id, kind, evidence)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![graph_version, edge.from, edge.to, edge.kind, edge.evidence],
            )
            .map_err(|error| format!("Unable to persist graph edge: {error}"))?;
    }
    Ok(())
}

pub fn status(root: &Path) -> Result<StoreStatus, String> {
    let connection = open(root)?;
    let project_id = connection
        .query_row(
            "SELECT value FROM product_metadata WHERE key = 'project_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read SQLite project identity: {error}"))?
        .unwrap_or_else(|| crate::graph::project_id(root));
    let current = connection
        .query_row(
            "SELECT graph.graph_id, graph.graph_version, observation.observation_id
             FROM project_state state
             JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            )),
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph: {error}"))?;
    let graph_count = connection
        .query_row(
            "SELECT COUNT(*) FROM graph_versions WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Unable to count graphs: {error}"))?;
    let (node_count, edge_count) = if let Some((_, version, _)) = current {
        (
            connection
                .query_row(
                    "SELECT COUNT(*) FROM graph_nodes WHERE graph_version = ?1",
                    params![version],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("Unable to count nodes: {error}"))?,
            connection
                .query_row(
                    "SELECT COUNT(*) FROM graph_edges WHERE graph_version = ?1",
                    params![version],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("Unable to count edges: {error}"))?,
        )
    } else {
        (0, 0)
    };
    Ok(StoreStatus {
        schema_version: STORE_SCHEMA.to_string(),
        path: database_path(root).to_string_lossy().into_owned(),
        project_id,
        current_graph_id: current.as_ref().map(|value| value.0.clone()),
        current_graph_version: current.as_ref().map(|value| value.1 as u64),
        graph_count: graph_count as u64,
        node_count: node_count as u64,
        edge_count: edge_count as u64,
        current_observation_id: current.map(|value| value.2),
    })
}

pub fn resolve_context(root: &Path, uri: &str) -> Result<ContextRef, String> {
    let connection = open(root)?;
    context::resolve(&connection, uri, &crate::graph::project_id(root))
}

pub fn current_graph(root: &Path) -> Result<Option<GraphSnapshot>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let Some((graph_id, graph_version, source_revision, source_fingerprint, observation_id, truncated, omissions_json)) = connection
        .query_row(
            "SELECT graph.graph_id, graph.graph_version,
                    CASE WHEN observation.dirty = 1 THEN observation.git_revision || '+dirty'
                         ELSE observation.git_revision END,
                    observation.source_fingerprint, observation.observation_id,
                    graph.truncated, graph.omissions_json
             FROM project_state state
             JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph: {error}"))?
    else {
        return Ok(None);
    };
    let mut files = connection
        .prepare(
            "SELECT path, language, bytes, hash FROM source_files
             WHERE graph_version = ?1 ORDER BY path",
        )
        .map_err(|error| format!("Unable to prepare source query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(SourceFile {
                path: row.get(0)?,
                language: row.get(1)?,
                bytes: row.get::<_, i64>(2)? as u64,
                hash: row.get(3)?,
            })
        })
        .map_err(|error| format!("Unable to query source evidence: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode source evidence: {error}"))?;
    let mut nodes = connection
        .prepare(
            "SELECT node_id, kind, path, name, language, evidence_fingerprint FROM graph_nodes
             WHERE graph_version = ?1 ORDER BY node_id",
        )
        .map_err(|error| format!("Unable to prepare node query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(GraphNode {
                id: row.get(0)?,
                kind: row.get(1)?,
                path: row.get(2)?,
                name: row.get(3)?,
                language: row.get(4)?,
                evidence_fingerprint: row.get(5)?,
            })
        })
        .map_err(|error| format!("Unable to query graph nodes: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode graph nodes: {error}"))?;
    let mut edges = connection
        .prepare(
            "SELECT from_id, to_id, kind, evidence FROM graph_edges
             WHERE graph_version = ?1 ORDER BY from_id, to_id, kind, evidence",
        )
        .map_err(|error| format!("Unable to prepare edge query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(GraphEdge {
                from: row.get(0)?,
                to: row.get(1)?,
                kind: row.get(2)?,
                evidence: row.get(3)?,
            })
        })
        .map_err(|error| format!("Unable to query graph edges: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode graph edges: {error}"))?;
    files.shrink_to_fit();
    nodes.shrink_to_fit();
    edges.shrink_to_fit();
    let omissions = serde_json::from_str::<Vec<String>>(&omissions_json)
        .map_err(|error| format!("Unable to decode graph omissions: {error}"))?;
    Ok(Some(GraphSnapshot {
        schema_version: crate::model::GRAPH_SCHEMA.to_string(),
        product: PRODUCT_IDENTITY.to_string(),
        project_id,
        graph_id,
        graph_version: graph_version as u64,
        source_revision,
        source_fingerprint,
        observation_id,
        files,
        nodes,
        edges,
        truncated: truncated != 0,
        omissions,
    }))
}

pub fn node_details(root: &Path, node_id: &str) -> Result<serde_json::Value, String> {
    let graph = current_graph(root)?.ok_or_else(|| "No graph has been scanned yet.".to_string())?;
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| "Node is unavailable in the current graph.".to_string())?;
    let outgoing = graph
        .edges
        .iter()
        .filter(|edge| edge.from == node_id)
        .cloned()
        .collect::<Vec<_>>();
    let incoming = graph
        .edges
        .iter()
        .filter(|edge| edge.to == node_id)
        .cloned()
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "schemaVersion": crate::model::GRAPH_SCHEMA,
        "projectId": graph.project_id,
        "graphId": graph.graph_id,
        "graphVersion": graph.graph_version,
        "node": node,
        "outgoing": outgoing,
        "incoming": incoming,
        "evidenceClass": "static",
        "limitations": ["Node details describe source structure only; runtime behavior and causality are unavailable."],
    }))
}

pub fn create_diagnostic_context(
    root: &Path,
    mut context: crate::model::DiagnosticContext,
) -> Result<crate::model::DiagnosticContext, String> {
    crate::diagnostic::validate_context(&context)?;
    let project_id = crate::graph::project_id(root);
    if context.project_id != project_id || context.current_graph_basis.project_id != project_id {
        return Err(
            "Diagnostic Context project identity does not match this repository.".to_string(),
        );
    }
    let current = current_graph(root)?
        .ok_or_else(|| "Scan the repository before creating a Diagnostic Context.".to_string())?;
    if context.current_graph_basis.graph_id != current.graph_id
        || context.current_graph_basis.graph_version != current.graph_version
        || context.current_graph_basis.source_revision != current.source_revision
        || context.current_graph_basis.observation_id != current.observation_id
    {
        return Err(
            "Diagnostic Context current graph basis, including source revision, is not current."
                .to_string(),
        );
    }
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin Diagnostic Context transaction: {error}"))?;
    if transaction
        .query_row(
            "SELECT 1 FROM diagnostic_contexts WHERE id = ?1",
            params![context.id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to check Diagnostic Context identity: {error}"))?
        .is_some()
    {
        return Err(format!("Diagnostic Context {} already exists.", context.id));
    }
    if let Some(supersedes) = &context.supersedes {
        let superseded_project = transaction
            .query_row(
                "SELECT project_id FROM diagnostic_contexts WHERE id = ?1",
                params![supersedes],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to validate superseded Context: {error}"))?;
        if superseded_project.as_deref() != Some(project_id.as_str()) {
            return Err(
                "Diagnostic Context supersedes an unavailable or wrong-project Context."
                    .to_string(),
            );
        }
    }
    context.revision = 1;
    if context.created_at == 0 {
        context.created_at = now_seconds() as u64;
    }
    let payload = serde_json::to_string(&context)
        .map_err(|error| format!("Unable to encode Diagnostic Context: {error}"))?;
    transaction
        .execute(
            "INSERT INTO diagnostic_contexts(id, project_id, revision, payload_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                context.id,
                context.project_id,
                context.revision,
                payload,
                context.created_at as i64
            ],
        )
        .map_err(|error| format!("Unable to persist Diagnostic Context: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit Diagnostic Context: {error}"))?;
    Ok(context)
}

pub fn get_diagnostic_context(
    root: &Path,
    context_id: &str,
) -> Result<crate::model::DiagnosticContext, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let payload = connection
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1 AND project_id = ?2",
            params![context_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read Diagnostic Context: {error}"))?
        .ok_or_else(|| format!("Diagnostic Context {context_id} is unavailable."))?;
    let context = serde_json::from_str::<crate::model::DiagnosticContext>(&payload)
        .map_err(|error| format!("Diagnostic Context {context_id} is corrupted: {error}"))?;
    crate::diagnostic::validate_context(&context)?;
    if context.revision == 0 {
        return Err("Diagnostic Context has a zero revision.".to_string());
    }
    Ok(context)
}

pub fn list_diagnostic_assertions(
    root: &Path,
    context_id: &str,
) -> Result<Vec<crate::model::DiagnosticAssertion>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let exists = connection
        .query_row(
            "SELECT 1 FROM diagnostic_contexts WHERE id = ?1 AND project_id = ?2",
            params![context_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to check Diagnostic Context: {error}"))?;
    if exists.is_none() {
        return Err(format!("Diagnostic Context {context_id} is unavailable."));
    }
    let mut statement = connection
        .prepare(
            "SELECT payload_json FROM diagnostic_assertions
             WHERE context_id = ?1 ORDER BY revision, id",
        )
        .map_err(|error| format!("Unable to prepare Diagnostic Assertion query: {error}"))?;
    statement
        .query_map(params![context_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to query Diagnostic Assertions: {error}"))?
        .map(|payload| {
            let payload =
                payload.map_err(|error| format!("Unable to read Diagnostic Assertion: {error}"))?;
            let assertion = serde_json::from_str::<crate::model::DiagnosticAssertion>(&payload)
                .map_err(|error| format!("Diagnostic Assertion is corrupted: {error}"))?;
            crate::diagnostic::validate_assertion(&assertion)?;
            if assertion.revision == 0 {
                return Err("Diagnostic Assertion has a zero revision.".to_string());
            }
            Ok(assertion)
        })
        .collect::<Result<Vec<_>, String>>()
}

pub fn append_diagnostic_assertion(
    root: &Path,
    assertion: crate::model::DiagnosticAssertion,
) -> Result<crate::model::DiagnosticAssertion, String> {
    crate::diagnostic::validate_assertion(&assertion)?;
    let context = get_diagnostic_context(root, &assertion.context_id)?;
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin Diagnostic Assertion transaction: {error}"))?;
    if transaction
        .query_row(
            "SELECT 1 FROM diagnostic_assertions WHERE id = ?1",
            params![assertion.id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to check Diagnostic Assertion identity: {error}"))?
        .is_some()
    {
        return Err(format!(
            "Diagnostic Assertion {} already exists.",
            assertion.id
        ));
    }
    let expected_revision = transaction
        .query_row(
            "SELECT COALESCE(MAX(revision), ?2) + 1 FROM diagnostic_assertions WHERE context_id = ?1",
            params![assertion.context_id, context.revision],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Unable to allocate Diagnostic Assertion revision: {error}"))?
        as u64;
    if assertion.revision != 0 && assertion.revision != expected_revision {
        return Err(format!(
            "Diagnostic Assertion revision must be {expected_revision}."
        ));
    }
    let mut assertion = assertion;
    assertion.revision = expected_revision;
    if assertion.created_at == 0 {
        assertion.created_at = now_seconds() as u64;
    }
    if let Some(supersedes) = &assertion.supersedes {
        let same_context = transaction
            .query_row(
                "SELECT context_id FROM diagnostic_assertions WHERE id = ?1",
                params![supersedes],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to validate superseded Assertion: {error}"))?;
        if same_context.as_deref() != Some(assertion.context_id.as_str()) {
            return Err(
                "Diagnostic Assertion supersedes an unavailable or different Context assertion."
                    .to_string(),
            );
        }
    }
    let payload = serde_json::to_string(&assertion)
        .map_err(|error| format!("Unable to encode Diagnostic Assertion: {error}"))?;
    transaction
        .execute(
            "INSERT INTO diagnostic_assertions(id, context_id, revision, kind, status, actor, payload_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                assertion.id,
                assertion.context_id,
                assertion.revision,
                assertion.kind,
                assertion.status,
                assertion.actor,
                payload,
                assertion.created_at as i64
            ],
        )
        .map_err(|error| format!("Unable to persist Diagnostic Assertion: {error}"))?;
    let mut updated_context = context;
    updated_context.revision = assertion.revision;
    let updated_payload = serde_json::to_string(&updated_context)
        .map_err(|error| format!("Unable to encode updated Diagnostic Context: {error}"))?;
    transaction
        .execute(
            "UPDATE diagnostic_contexts SET revision = ?1, payload_json = ?2 WHERE id = ?3",
            params![
                updated_context.revision,
                updated_payload,
                updated_context.id
            ],
        )
        .map_err(|error| format!("Unable to advance Diagnostic Context revision: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit Diagnostic Assertion: {error}"))?;
    Ok(assertion)
}

pub fn persist_historical_candidates(
    root: &Path,
    diagnosis: &crate::model::HistoricalDiagnosis,
) -> Result<(), String> {
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin historical candidate transaction: {error}"))?;
    transaction
        .execute(
            "DELETE FROM historical_candidates WHERE context_id = ?1 AND graph_version = ?2",
            params![
                diagnosis.context_id,
                diagnosis.current_graph_basis.graph_version
            ],
        )
        .map_err(|error| format!("Unable to replace historical candidates: {error}"))?;
    for candidate in &diagnosis.candidates {
        let payload = serde_json::to_string(candidate)
            .map_err(|error| format!("Unable to encode historical candidate: {error}"))?;
        transaction
            .execute(
                "INSERT INTO historical_candidates(id, project_id, context_id, graph_version, payload_json, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    candidate.id,
                    candidate.project_id,
                    candidate.context_id,
                    candidate.current_graph_basis.graph_version,
                    payload,
                    now_seconds()
                ],
            )
            .map_err(|error| format!("Unable to persist historical candidate: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit historical candidates: {error}"))
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flopeek-store-{suffix}"));
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write");
        root
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn persists_graph_atomically_and_reuses_version() {
        let root = fixture_root();
        let (snapshot, facts) = graph::build(&root).expect("build");
        let first = persist_scan(&root, snapshot, &facts).expect("persist");
        let (snapshot, facts) = graph::build(&root).expect("build again");
        let second = persist_scan(&root, snapshot, &facts).expect("persist again");
        assert_eq!(first.graph.graph_version, 1);
        assert_eq!(second.graph.graph_version, 1);
        assert_eq!(status(&root).expect("status").graph_count, 1);
        assert!(database_path(&root).is_file());
        let connection = open(&root).expect("open");
        let facts_json = connection
            .query_row("SELECT facts_json FROM source_files LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("facts");
        assert!(!facts_json.contains("export const main = 1"));
        assert!(!facts_json.contains("Promise"));
        drop(connection);
        let current = current_graph(&root).expect("current graph").expect("graph");
        let node = current
            .nodes
            .iter()
            .find(|node| node.kind == "variable")
            .expect("variable node");
        let details = node_details(&root, &node.id).expect("node details");
        assert_eq!(details["evidenceClass"], "static");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn context_ref_becomes_stale_after_graph_changes() {
        let root = fixture_root();
        let (snapshot, facts) = graph::build(&root).expect("build");
        let first = persist_scan(&root, snapshot, &facts).expect("persist");
        let uri = first.context_refs[0].uri.clone();
        fs::write(root.join("src/other.ts"), "export const other = 2;").expect("write");
        let (snapshot, facts) = graph::build(&root).expect("build changed");
        let second = persist_scan(&root, snapshot, &facts).expect("persist changed");
        assert_eq!(second.graph.graph_version, 2);
        assert_eq!(
            resolve_context(&root, &uri).expect("resolve").status,
            "current"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn focused_symbol_and_direct_edge_changes_are_stale_but_unrelated_nodes_stay_current() {
        let root = fixture_root();
        fs::write(root.join("src/helper.ts"), "export const helper = 1;\n").expect("helper");
        let (snapshot, facts) = graph::build(&root).expect("build");
        let first = persist_scan(&root, snapshot, &facts).expect("persist");
        let focused = first
            .context_refs
            .iter()
            .find(|reference| {
                first.graph.nodes.iter().any(|node| {
                    node.id == reference.node_id
                        && node.path.as_deref() == Some("src/main.ts")
                        && node.kind == "file"
                })
            })
            .expect("focused file")
            .uri
            .clone();
        fs::write(
            root.join("src/unrelated.ts"),
            "export const unrelated = 1;\n",
        )
        .expect("unrelated");
        let (snapshot, facts) = graph::build(&root).expect("unrelated graph");
        persist_scan(&root, snapshot, &facts).expect("persist unrelated");
        assert_eq!(
            resolve_context(&root, &focused)
                .expect("resolve unrelated")
                .status,
            "current"
        );
        fs::write(root.join("src/main.ts"), "export const main = 2;\n").expect("focused change");
        let (snapshot, facts) = graph::build(&root).expect("focused graph");
        persist_scan(&root, snapshot, &facts).expect("persist focused");
        let resolved = resolve_context(&root, &focused).expect("resolve focused");
        assert_eq!(resolved.status, "stale");
        assert!(resolved.freshness_reason.contains("fingerprint"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn identical_structure_across_revision_gets_new_observation_and_reuses_graph_version() {
        let root = fixture_root();
        git(&root, &["init"]);
        git(
            &root,
            &["config", "user.email", "flopeek-test@example.invalid"],
        );
        git(&root, &["config", "user.name", "Flopeek Test"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "source A"]);
        let (snapshot, facts) = graph::build(&root).expect("build A");
        let first = persist_scan(&root, snapshot, &facts).expect("scan A");
        let first_ref = first.context_refs[0].clone();
        let first_resolved = resolve_context(&root, &first_ref.uri).expect("resolve first ref");
        assert_eq!(first_ref, first_resolved);
        assert_eq!(first_ref.origin_observation_id, first.graph.observation_id);
        assert_eq!(
            first_ref
                .current_basis
                .as_ref()
                .expect("first current basis")
                .observation_id,
            first.graph.observation_id
        );
        fs::write(root.join("README.md"), "documentation-only change\n").expect("README");
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "README only"]);
        let (snapshot, facts) = graph::build(&root).expect("build README");
        let second = persist_scan(&root, snapshot, &facts).expect("scan README");
        assert_eq!(first.graph.graph_id, second.graph.graph_id);
        assert_eq!(first.graph.graph_version, second.graph.graph_version);
        assert_ne!(first.graph.observation_id, second.graph.observation_id);
        let second_ref = second
            .context_refs
            .iter()
            .find(|reference| reference.uri == first_ref.uri)
            .expect("same Context Ref URI");
        assert_eq!(second_ref.origin_observation_id, first.graph.observation_id);
        assert_eq!(
            second_ref
                .current_basis
                .as_ref()
                .expect("second current basis")
                .observation_id,
            second.graph.observation_id
        );
        assert_eq!(second_ref.status, "current");
        assert_eq!(
            second_ref.freshness_reason,
            "node AST and direct-edge fingerprint matches"
        );
        assert_eq!(
            second_ref,
            &resolve_context(&root, &second_ref.uri).expect("resolve canonical second ref")
        );
        let connection = open(&root).expect("open");
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM graph_observations", [], |row| row
                    .get::<_, i64>(0))
                .expect("observations"),
            2
        );
        let manifest = connection
            .query_row(
                "SELECT source_manifest_json FROM graph_observations ORDER BY observed_at LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("manifest");
        assert!(!manifest.contains("export const main"));
        assert_eq!(
            resolve_context(&root, &first.context_refs[0].uri)
                .expect("resolve old ref")
                .status,
            "current"
        );
        drop(connection);
        fs::remove_dir_all(root).expect("cleanup");
    }

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
        drop(connection);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
