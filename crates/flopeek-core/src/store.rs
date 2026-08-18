//! SQLite authority for graph versions and Context Refs.
//!
//! Writes are transactional and facts contain hashes/structure only.  Source bodies
//! and credentials never enter this store.

use crate::context;
use crate::model::{
    ContextRef, GraphEdge, GraphNode, GraphSnapshot, PRODUCT_IDENTITY, STORE_SCHEMA, ScanResult,
    SourceFile, StoreStatus, TypeScriptFacts,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const STORE_DIRECTORY: &str = ".flopeek";
pub const STORE_FILENAME: &str = "flopeek.sqlite3";

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
    let connection = Connection::open(&path)
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
    initialize_schema(&connection)?;
    Ok(connection)
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA user_version = 2;
             CREATE TABLE IF NOT EXISTS product_metadata (
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
             );
             CREATE INDEX IF NOT EXISTS graph_versions_project_idx ON graph_versions(project_id, graph_version);
             CREATE INDEX IF NOT EXISTS context_refs_project_idx ON context_refs(project_id, graph_version);
             CREATE INDEX IF NOT EXISTS diagnostic_assertions_context_idx ON diagnostic_assertions(context_id, revision);",
        )
        .map_err(|error| format!("Unable to initialize SQLite schema: {error}"))?;
    ensure_column(
        connection,
        "graph_versions",
        "omissions_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(connection, "historical_candidates", "context_id", "TEXT")?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS historical_candidates_context_idx
             ON historical_candidates(context_id, graph_version)",
            [],
        )
        .map_err(|error| format!("Unable to initialize historical candidate index: {error}"))?;
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Unable to inspect {table} schema: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Unable to inspect {table} columns: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode {table} columns: {error}"))?;
    if !columns.iter().any(|existing| existing == column) {
        connection
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|error| format!("Unable to migrate {table} schema: {error}"))?;
    }
    Ok(())
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
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit SQLite graph transaction: {error}"))?;

    snapshot.graph_version = graph_version;
    let connection = open(root)?;
    let refs = context::for_snapshot(&connection, &snapshot)?;
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
                "INSERT INTO graph_nodes(graph_version, node_id, kind, path, name, language)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    graph_version,
                    node.id,
                    node.kind,
                    node.path,
                    node.name,
                    node.language
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
            "SELECT graph_id, graph_version FROM graph_versions WHERE project_id = ?1 ORDER BY graph_version DESC LIMIT 1",
            params![project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
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
    let (node_count, edge_count) = if let Some((_, version)) = current {
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
        current_graph_version: current.map(|value| value.1 as u64),
        graph_count: graph_count as u64,
        node_count: node_count as u64,
        edge_count: edge_count as u64,
    })
}

pub fn resolve_context(root: &Path, uri: &str) -> Result<ContextRef, String> {
    let connection = open(root)?;
    context::resolve(&connection, uri, &crate::graph::project_id(root))
}

pub fn current_graph(root: &Path) -> Result<Option<GraphSnapshot>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let Some((graph_id, graph_version, source_revision, truncated, omissions_json)) = connection
        .query_row(
            "SELECT graph_id, graph_version, source_revision, truncated, omissions_json
             FROM graph_versions WHERE project_id = ?1 ORDER BY graph_version DESC LIMIT 1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
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
            "SELECT node_id, kind, path, name, language FROM graph_nodes
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
    {
        return Err("Diagnostic Context current graph basis is not current.".to_string());
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
            "stale"
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
