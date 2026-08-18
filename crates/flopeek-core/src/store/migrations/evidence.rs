//! Evidence-contract provenance migration.

#[allow(unused_imports)]
use super::*;

pub(crate) fn migration_v8(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column(
        transaction,
        "graph_versions",
        "graph_schema_version",
        "TEXT NOT NULL DEFAULT 'legacy-evidence-contract-unavailable'",
    )?;
    add_column(
        transaction,
        "graph_versions",
        "graph_derivation_id",
        "TEXT NOT NULL DEFAULT 'legacy-evidence-contract-unavailable'",
    )?;
    add_column(
        transaction,
        "graph_versions",
        "node_fingerprint_contract",
        "TEXT NOT NULL DEFAULT 'legacy-evidence-contract-unavailable'",
    )?;
    transaction
        .execute(
            "UPDATE graph_versions
             SET graph_schema_version = COALESCE(NULLIF(graph_schema_version, ''), ?1),
                 graph_derivation_id = COALESCE(NULLIF(graph_derivation_id, ''), ?1),
                 node_fingerprint_contract = COALESCE(NULLIF(node_fingerprint_contract, ''), ?1)",
            params![crate::temporal::LEGACY_EVIDENCE_CONTRACT],
        )
        .map_err(|error| format!("Unable to normalize legacy evidence contracts: {error}"))?;
    Ok(())
}
