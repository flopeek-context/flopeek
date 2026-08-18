//! Versioned feature migrations.

#[allow(unused_imports)]
use super::*;

pub(crate) fn migration_v5(transaction: &Transaction<'_>) -> Result<(), String> {
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

pub(crate) fn migration_v6(transaction: &Transaction<'_>) -> Result<(), String> {
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
