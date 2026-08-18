//! SQLite base schema migrations.

#[allow(unused_imports)]
use super::*;

pub(crate) fn migration_v1(transaction: &Transaction<'_>) -> Result<(), String> {
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

pub(crate) fn migration_v2(transaction: &Transaction<'_>) -> Result<(), String> {
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

pub(crate) fn migration_v3(transaction: &Transaction<'_>) -> Result<(), String> {
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

pub(crate) fn migration_v4(transaction: &Transaction<'_>) -> Result<(), String> {
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
