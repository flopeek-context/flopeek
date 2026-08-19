//! Validation rules for last-known-good bindings.

#[allow(unused_imports)]
use super::*;
use crate::model::{LastKnownGoodBinding, LastKnownGoodValidation};

/// Validate only the syntactic relationship between optional observation/event
/// fields. Resolvable-but-inconsistent evidence is handled by `validate_binding`
/// so proposals can be retained as invalid evidence.
pub(super) fn validate_optional_basis(
    _transaction: &Transaction<'_>,
    binding: &LastKnownGoodBinding,
    _project_id: &str,
) -> Result<(), String> {
    if binding.event_id.is_some() && binding.observation_id.is_none() {
        return Err(
            "LastKnownGoodBinding eventId requires observationId for deterministic provenance."
                .to_string(),
        );
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
        crate::diagnostic::resolve_last_known_good_revision(root, &binding.git_revision).is_ok();
    if !revision_available {
        limitations.push("git-revision-unavailable".to_string());
    }
    let first_parent_validation = revision_available
        .then(|| crate::diagnostic::validate_first_parent_range(root, &binding.git_revision));
    let first_parent_range_available = first_parent_validation
        .as_ref()
        .is_some_and(|result| result.is_ok());
    if let Some(Err(reason)) = first_parent_validation {
        limitations.push(reason);
    }

    let mut basis_provenance_consistent = true;
    let mut evidence_contract_compatible = true;
    if let Some(basis) = binding.graph_basis.as_ref() {
        if basis.project_id != binding.project_id
            || basis.source_revision != binding.git_revision
            || basis.observation_id.is_empty()
        {
            basis_provenance_consistent = false;
        }

        let graph_row = connection
            .query_row(
                "SELECT graph_id, project_id, source_revision, graph_schema_version,
                        graph_derivation_id, node_fingerprint_contract
                 FROM graph_versions WHERE graph_version = ?1",
                params![basis.graph_version as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("Unable to validate last-known-good graph basis: {error}"))?;
        match graph_row {
            Some((graph_id, project_id, source_revision, schema, derivation, contract)) => {
                if graph_id != basis.graph_id
                    || project_id != basis.project_id
                    || source_revision != basis.source_revision
                {
                    basis_provenance_consistent = false;
                }
                evidence_contract_compatible = schema == crate::model::GRAPH_SCHEMA
                    && derivation == crate::graph::GRAPH_DERIVATION_ID
                    && contract == crate::temporal::NODE_FINGERPRINT_CONTRACT;
            }
            None => {
                basis_provenance_consistent = false;
                evidence_contract_compatible = false;
            }
        }

        if let Some(observation_id) = binding.observation_id.as_deref() {
            let observation = connection
                .query_row(
                    "SELECT project_id, graph_version, git_revision
                     FROM graph_observations WHERE observation_id = ?1",
                    params![observation_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| {
                    format!("Unable to validate last-known-good observation: {error}")
                })?;
            match observation {
                Some((project_id, graph_version, git_revision)) => {
                    if project_id != binding.project_id
                        || git_revision != binding.git_revision
                        || graph_version != basis.graph_version as i64
                        || project_id != basis.project_id
                        || observation_id != basis.observation_id
                    {
                        basis_provenance_consistent = false;
                    }
                }
                None => basis_provenance_consistent = false,
            }
        } else {
            basis_provenance_consistent = false;
        }

        if let Some(event_id) = binding.event_id.as_deref() {
            let event = connection
                .query_row(
                    "SELECT project_id, observation_id
                     FROM observation_events WHERE event_id = ?1",
                    params![event_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|error| format!("Unable to validate last-known-good event: {error}"))?;
            if event.as_ref().map(|value| value.0.as_str()) != Some(binding.project_id.as_str())
                || event.as_ref().map(|value| value.1.as_str()) != binding.observation_id.as_deref()
            {
                basis_provenance_consistent = false;
            }
        }
    } else if binding.observation_id.is_some() || binding.event_id.is_some() {
        basis_provenance_consistent = false;
    }

    if !basis_provenance_consistent {
        limitations.push("last-known-good-basis-provenance-mismatch".to_string());
    }
    if !evidence_contract_compatible {
        limitations.push("evidence-contract-incompatible".to_string());
    }
    let valid = repository_match
        && revision_available
        && first_parent_range_available
        && evidence_contract_compatible
        && basis_provenance_consistent;
    Ok(LastKnownGoodValidation {
        status: if valid { "valid" } else { "invalid" }.to_string(),
        revision_available,
        repository_match,
        first_parent_range_available,
        evidence_contract_compatible,
        basis_provenance_consistent,
        limitations,
    })
}
