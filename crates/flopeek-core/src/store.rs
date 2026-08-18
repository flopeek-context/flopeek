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
            "PRAGMA user_version = 1;
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
                 truncated INTEGER NOT NULL CHECK (truncated IN (0, 1))
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
                 graph_version INTEGER NOT NULL REFERENCES graph_versions(graph_version),
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS graph_versions_project_idx ON graph_versions(project_id, graph_version);
             CREATE INDEX IF NOT EXISTS context_refs_project_idx ON context_refs(project_id, graph_version);",
        )
        .map_err(|error| format!("Unable to initialize SQLite schema: {error}"))
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
                "INSERT INTO graph_versions(graph_version, graph_id, project_id, source_revision, created_at, truncated)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    version,
                    snapshot.graph_id,
                    snapshot.project_id,
                    snapshot.source_revision,
                    now_seconds(),
                    i64::from(snapshot.truncated),
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
    let Some((graph_id, graph_version, source_revision, truncated)) = connection
        .query_row(
            "SELECT graph_id, graph_version, source_revision, truncated
             FROM graph_versions WHERE project_id = ?1 ORDER BY graph_version DESC LIMIT 1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
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
        omissions: Vec::new(),
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
}
