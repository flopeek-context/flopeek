//! Validation rules for last-known-good bindings.

#[allow(unused_imports)]
use super::*;
use crate::model::{LastKnownGoodBinding, LastKnownGoodValidation};

pub(super) fn validate_optional_basis(
    transaction: &Transaction<'_>,
    binding: &LastKnownGoodBinding,
    project_id: &str,
) -> Result<(), String> {
    if binding.observation_id.is_some() || binding.event_id.is_some() {
        let observation = binding.observation_id.as_deref().ok_or_else(|| {
            "LastKnownGoodBinding eventId requires observationId for deterministic provenance."
                .to_string()
        })?;
        let row = transaction
            .query_row(
                "SELECT project_id, git_revision FROM graph_observations WHERE observation_id = ?1",
                params![observation],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| format!("Unable to validate last-known-good observation: {error}"))?;
        if row.as_ref().map(|value| value.0.as_str()) != Some(project_id) {
            return Err(
                "LastKnownGoodBinding observation is unavailable or wrong-project.".to_string(),
            );
        }
        if row.as_ref().map(|value| value.1.as_str()) != Some(binding.git_revision.as_str()) {
            return Err(
                "LastKnownGoodBinding observation revision does not match gitRevision.".to_string(),
            );
        }
        if let Some(event_id) = binding.event_id.as_deref() {
            let event = transaction
                .query_row(
                    "SELECT project_id, observation_id FROM observation_events WHERE event_id = ?1",
                    params![event_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|error| format!("Unable to validate last-known-good event: {error}"))?;
            if event.as_ref().map(|value| value.0.as_str()) != Some(project_id)
                || event.as_ref().map(|value| value.1.as_str()) != Some(observation)
            {
                return Err("LastKnownGoodBinding event provenance is inconsistent.".to_string());
            }
        }
    }
    if let Some(basis) = binding.graph_basis.as_ref()
        && basis.project_id != project_id
    {
        return Err("LastKnownGoodBinding graph basis is wrong-project.".to_string());
    }
    Ok(())
}

pub(super) fn validate_binding(
    connection: &Connection,
    root: &Path,
    binding: &LastKnownGoodBinding,
    repository_id: &str,
) -> Result<LastKnownGoodValidation, String> {
    let mut limitations = Vec::new();
    let repository_match = binding.repository_id == repository_id;
    if !repository_match {
        limitations.push("repository-identity-mismatch".to_string());
    }
    let revision_available =
        crate::diagnostic::validate_revision_range(root, &binding.git_revision).is_ok();
    if !revision_available {
        limitations.push("git-revision-unavailable".to_string());
    }
    let first_parent_range_available = revision_available;
    let evidence_contract_compatible = if let Some(basis) = binding.graph_basis.as_ref() {
        let row = connection
            .query_row(
                "SELECT graph_id, graph_schema_version, graph_derivation_id, node_fingerprint_contract
                 FROM graph_versions WHERE graph_version = ?1 AND project_id = ?2",
                params![basis.graph_version as i64, basis.project_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("Unable to validate last-known-good graph basis: {error}"))?;
        row.is_some_and(|row| {
            row.0 == basis.graph_id
                && row.1 == crate::model::GRAPH_SCHEMA
                && row.2 == crate::graph::GRAPH_DERIVATION_ID
                && row.3 == crate::temporal::NODE_FINGERPRINT_CONTRACT
        })
    } else {
        true
    };
    if !evidence_contract_compatible {
        limitations.push("evidence-contract-incompatible".to_string());
    }
    let valid = repository_match
        && revision_available
        && first_parent_range_available
        && evidence_contract_compatible;
    Ok(LastKnownGoodValidation {
        status: if valid { "valid" } else { "invalid" }.to_string(),
        revision_available,
        repository_match,
        first_parent_range_available,
        evidence_contract_compatible,
        limitations,
    })
}
