//! Repository/checkout identity provenance migration.

use super::*;

pub(crate) fn migration_v9(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "graph_observations",
        "repository_identity_status",
        "TEXT NOT NULL DEFAULT 'legacy-checkout-local'",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "repository_identity_id",
        "TEXT",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "repository_manifest_path",
        "TEXT",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "repository_manifest_bytes",
        "INTEGER",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "repository_manifest_hash",
        "TEXT",
    )?;
    add_column(
        transaction,
        "graph_observations",
        "checkout_id_hash",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS repository_identities (
                 repository_id TEXT PRIMARY KEY NOT NULL,
                 manifest_path TEXT NOT NULL,
                 manifest_bytes INTEGER NOT NULL,
                 manifest_hash TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS project_identity_aliases (
                 legacy_project_id TEXT PRIMARY KEY NOT NULL,
                 repository_project_id TEXT NOT NULL,
                 checkout_id_hash TEXT NOT NULL,
                 alias_kind TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS project_identity_aliases_repository_idx
                 ON project_identity_aliases(repository_project_id, legacy_project_id);
             CREATE INDEX IF NOT EXISTS graph_observations_repository_identity_idx
                 ON graph_observations(repository_identity_id, observed_at, graph_version);",
        )
        .map_err(|error| format!("Unable to initialize repository identity schema: {error}"))
}
