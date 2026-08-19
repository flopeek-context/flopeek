//! SQLite adapter for attributed last-known-good bindings.

use super::last_known_good_validation::{validate_binding, validate_optional_basis};
#[allow(unused_imports)]
use super::*;
use crate::model::{
    LAST_KNOWN_GOOD_SCHEMA, LastKnownGoodBinding, LastKnownGoodResolution,
    reduce_last_known_good_lifecycle,
};

const ALLOWED_STATUSES: &[&str] = &["proposed", "confirmed", "rejected", "revoked", "superseded"];
const ALLOWED_ACTOR_KINDS: &[&str] = &["human", "agent", "tool"];

pub fn create_last_known_good_binding(
    root: &Path,
    mut binding: LastKnownGoodBinding,
) -> Result<LastKnownGoodBinding, String> {
    let identity = crate::identity::resolve(root)?;
    let repository_id = identity.repository_id.ok_or_else(|| {
        "repository-identity-unavailable: last-known-good bindings require the root manifest."
            .to_string()
    })?;
    if binding.schema_version != LAST_KNOWN_GOOD_SCHEMA {
        return Err("LastKnownGoodBinding schema version is unsupported.".to_string());
    }
    if !ALLOWED_STATUSES.contains(&binding.status.as_str()) {
        return Err("LastKnownGoodBinding status is unsupported.".to_string());
    }
    if !ALLOWED_ACTOR_KINDS.contains(&binding.actor_kind.as_str()) {
        return Err("LastKnownGoodBinding actor kind is unsupported.".to_string());
    }
    if binding.status != "proposed" && binding.actor_kind != "human" {
        return Err(
            "Only a human actor may confirm, reject, revoke, or supersede a last-known-good binding."
                .to_string(),
        );
    }
    if binding.status == "superseded" {
        return Err(
            "Status superseded is legacy read-only; use a confirmed binding with supersedesBindingId."
                .to_string(),
        );
    }
    if binding.repository_id != repository_id || binding.project_id != identity.project_id {
        return Err(
            "LastKnownGoodBinding repository identity does not match this checkout.".to_string(),
        );
    }
    if binding.binding_id.is_empty() || binding.context_id.is_empty() {
        return Err("LastKnownGoodBinding requires bindingId and contextId.".to_string());
    }
    if binding.binding_id.len() > 128
        || binding.context_id.len() > 128
        || binding.binding_id.contains(['/', '\\', '\r', '\n', '\0'])
        || binding.context_id.contains(['/', '\\', '\r', '\n', '\0'])
    {
        return Err("LastKnownGoodBinding identifiers are invalid or unbounded.".to_string());
    }
    for evidence in &binding.evidence {
        crate::diagnostic::validate_evidence(evidence)?;
    }
    let resolved_revision =
        crate::diagnostic::resolve_last_known_good_revision(root, &binding.git_revision)?;
    binding.git_revision = resolved_revision;
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin last-known-good transaction: {error}"))?;
    let context_project = transaction
        .query_row(
            "SELECT project_id FROM diagnostic_contexts WHERE id = ?1",
            params![binding.context_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| {
            format!("Unable to read Diagnostic Context for last-known-good: {error}")
        })?;
    if context_project.as_deref() != Some(identity.project_id.as_str()) {
        return Err("LastKnownGoodBinding Context is unavailable or wrong-project.".to_string());
    }
    if transaction
        .query_row(
            "SELECT 1 FROM last_known_good_bindings WHERE binding_id = ?1",
            params![binding.binding_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to inspect last-known-good identity: {error}"))?
        .is_some()
    {
        return Err(format!(
            "LastKnownGoodBinding {} already exists.",
            binding.binding_id
        ));
    }
    validate_optional_basis(&transaction, &binding, &identity.project_id)?;
    let validation = validate_binding(&transaction, root, &binding, &repository_id)?;
    if binding.status == "confirmed" && validation.status != "valid" {
        return Err(format!(
            "LastKnownGoodBinding confirmation failed: {}",
            validation.limitations.join("; ")
        ));
    }
    binding.validation = validation;
    if binding.created_at == 0 {
        binding.created_at = now_seconds() as u64;
    }
    let lifecycle = reduce_last_known_good_lifecycle(load_binding_history(
        &transaction,
        &binding.context_id,
        &identity.project_id,
    )?)?;
    let expected_predecessor = lifecycle
        .latest_event
        .as_ref()
        .map(|predecessor| predecessor.binding_id.as_str());
    if binding.predecessor_binding_id.is_none() {
        binding.predecessor_binding_id = expected_predecessor.map(ToOwned::to_owned);
    } else if binding.predecessor_binding_id.as_deref() != expected_predecessor {
        return Err(
            "LastKnownGoodBinding predecessor must be the current lifecycle tip.".to_string(),
        );
    }
    if let Some(predecessor) = &binding.predecessor_binding_id {
        let same_context = transaction
            .query_row(
                "SELECT context_id FROM last_known_good_bindings WHERE binding_id = ?1",
                params![predecessor],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to validate last-known-good predecessor: {error}"))?;
        if same_context.as_deref() != Some(binding.context_id.as_str()) {
            return Err(
                "LastKnownGoodBinding predecessor is unavailable or belongs to another Context."
                    .to_string(),
            );
        }
    }
    if let Some(target) = &binding.target_binding_id {
        let same_context = transaction
            .query_row(
                "SELECT context_id FROM last_known_good_bindings WHERE binding_id = ?1",
                params![target],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to validate last-known-good target: {error}"))?;
        if same_context.as_deref() != Some(binding.context_id.as_str()) {
            return Err("LastKnownGoodBinding target is unavailable or cross-context.".to_string());
        }
    }
    if let Some(target) = &binding.supersedes_binding_id {
        let same_context = transaction
            .query_row(
                "SELECT context_id FROM last_known_good_bindings WHERE binding_id = ?1",
                params![target],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                format!("Unable to validate supersedes last-known-good binding: {error}")
            })?;
        if same_context.as_deref() != Some(binding.context_id.as_str()) {
            return Err(
                "LastKnownGoodBinding supersedes target is unavailable or cross-context."
                    .to_string(),
            );
        }
    }
    if binding.status == "confirmed" {
        for (relation, target) in [
            ("target", binding.target_binding_id.as_deref()),
            ("supersedes", binding.supersedes_binding_id.as_deref()),
        ] {
            if let Some(target) = target
                && !binding_target_is_valid(&transaction, target)?
            {
                return Err(format!(
                    "LastKnownGoodBinding {relation} target has invalid or unavailable provenance."
                ));
            }
        }
    }
    let mut candidate_history = lifecycle.history.clone();
    candidate_history.push(binding.clone());
    reduce_last_known_good_lifecycle(candidate_history)?;
    let payload = serde_json::to_string(&binding)
        .map_err(|error| format!("Unable to encode last-known-good binding: {error}"))?;
    let evidence_json = serde_json::to_string(&binding.evidence)
        .map_err(|error| format!("Unable to encode last-known-good evidence: {error}"))?;
    let graph_basis_json = binding
        .graph_basis
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| format!("Unable to encode last-known-good graph basis: {error}"))?;
    transaction
        .execute(
            "INSERT INTO last_known_good_bindings(
                 binding_id, repository_id, project_id, context_id, git_revision,
                 observation_id, event_id, graph_basis_json, actor, actor_kind,
                 evidence_json, status, predecessor_binding_id, superseded_binding_id,
                 target_binding_id, supersedes_binding_id, payload_json, created_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                binding.binding_id,
                binding.repository_id,
                binding.project_id,
                binding.context_id,
                binding.git_revision,
                binding.observation_id,
                binding.event_id,
                graph_basis_json,
                binding.actor,
                binding.actor_kind,
                evidence_json,
                binding.status,
                binding.predecessor_binding_id,
                Option::<String>::None,
                binding.target_binding_id,
                binding.supersedes_binding_id,
                payload,
                binding.created_at as i64,
            ],
        )
        .map_err(|error| format!("Unable to persist last-known-good binding: {error}"))?;
    let effective = reduce_last_known_good_lifecycle(load_binding_history(
        &transaction,
        &binding.context_id,
        &identity.project_id,
    )?)?;
    let effective_binding_id = effective
        .active_confirmed
        .as_ref()
        .map(|value| value.binding_id.clone());
    let context_payload = transaction
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1",
            params![binding.context_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Unable to read Diagnostic Context payload: {error}"))?;
    let mut context = serde_json::from_str::<crate::model::DiagnosticContext>(&context_payload)
        .map_err(|error| format!("Diagnostic Context is corrupted: {error}"))?;
    if context.last_known_good_binding_id != effective_binding_id {
        context.last_known_good_binding_id = effective_binding_id;
        context.revision = context.revision.saturating_add(1);
        let updated_payload = serde_json::to_string(&context)
            .map_err(|error| format!("Unable to encode updated Diagnostic Context: {error}"))?;
        transaction
            .execute(
                "UPDATE diagnostic_contexts SET revision = ?1, payload_json = ?2 WHERE id = ?3",
                params![context.revision, updated_payload, binding.context_id],
            )
            .map_err(|error| format!("Unable to update effective last-known-good: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit last-known-good binding: {error}"))?;
    Ok(binding)
}

pub fn get_last_known_good(
    root: &Path,
    context_id: &str,
) -> Result<LastKnownGoodResolution, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let legacy_payload = connection
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1 AND project_id = ?2",
            params![context_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| {
            format!("Unable to read Diagnostic Context for last-known-good: {error}")
        })?;
    let legacy_basis = legacy_payload
        .map(|payload| {
            serde_json::from_str::<crate::model::DiagnosticContext>(&payload)
                .map_err(|error| format!("Diagnostic Context is corrupted: {error}"))
                .map(|context| context.last_known_good_basis)
        })
        .transpose()?
        .flatten();
    let lifecycle = match reduce_last_known_good_lifecycle(load_binding_history(
        &connection,
        context_id,
        &project_id,
    )?) {
        Ok(lifecycle) => lifecycle,
        Err(reason) => {
            return Ok(LastKnownGoodResolution {
                schema_version: LAST_KNOWN_GOOD_SCHEMA.to_string(),
                context_id: context_id.to_string(),
                status: "unavailable".to_string(),
                binding: None,
                legacy_basis,
                limitations: vec![
                    "Last-known-good lifecycle is unavailable and no active binding was inferred."
                        .to_string(),
                    reason,
                ],
            });
        }
    };
    let has_active = lifecycle.active_confirmed.is_some();
    let has_latest = lifecycle.latest_event.is_some();
    let has_pending = lifecycle.pending_proposal.is_some();
    let binding = lifecycle
        .active_confirmed
        .clone()
        .or(lifecycle.latest_event);
    let status = binding
        .as_ref()
        .map(|value| value.status.clone())
        .unwrap_or_else(|| {
            if legacy_basis.is_some() {
                "legacy-unbound".to_string()
            } else {
                "unavailable".to_string()
            }
        });
    let mut limitations = vec![
        "Last-known-good is explicit engineering evidence; Flopeek does not infer it from tests, commits, graph similarity, or candidate ranking.".to_string(),
    ];
    if !has_active && has_latest {
        limitations.push(
            "No active confirmed last-known-good binding exists; the returned binding is lifecycle history only."
                .to_string(),
        );
    }
    if has_pending {
        limitations.push(
            "A last-known-good proposal is pending and has not changed the active confirmation."
                .to_string(),
        );
    }
    if legacy_basis.is_some() && binding.is_none() {
        limitations.push("legacy last-known-good basis is readable but unbound and is not used for new diagnosis.".to_string());
    }
    Ok(LastKnownGoodResolution {
        schema_version: LAST_KNOWN_GOOD_SCHEMA.to_string(),
        context_id: context_id.to_string(),
        status,
        binding,
        legacy_basis,
        limitations,
    })
}

pub fn list_last_known_good_history(
    root: &Path,
    context_id: &str,
) -> Result<Vec<LastKnownGoodBinding>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    ensure_context(&connection, context_id, &project_id)?;
    let lifecycle = match reduce_last_known_good_lifecycle(load_binding_history(
        &connection,
        context_id,
        &project_id,
    )?) {
        Ok(lifecycle) => lifecycle,
        Err(reason) => {
            return Err(format!(
                "Last-known-good lifecycle is unavailable: {reason}"
            ));
        }
    };
    Ok(lifecycle.history)
}

pub fn validate_last_known_good(
    root: &Path,
    context_id: &str,
    binding_id: &str,
) -> Result<LastKnownGoodBinding, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let mut binding = load_binding(&connection, context_id, binding_id, &project_id)?;
    let identity = crate::identity::resolve(root)?;
    let repository_id = identity.repository_id.ok_or_else(|| {
        "repository-identity-unavailable: cannot validate portable binding.".to_string()
    })?;
    binding.validation = validate_binding(&connection, root, &binding, &repository_id)?;
    Ok(binding)
}

#[allow(dead_code)]
pub(crate) fn confirmed_last_known_good(
    root: &Path,
    context_id: &str,
) -> Result<Option<LastKnownGoodBinding>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let lifecycle = match reduce_last_known_good_lifecycle(load_binding_history(
        &connection,
        context_id,
        &project_id,
    )?) {
        Ok(lifecycle) => lifecycle,
        Err(_) => return Ok(None),
    };
    let Some(mut binding) = lifecycle.active_confirmed else {
        return Ok(None);
    };
    let identity = crate::identity::resolve(root)?;
    let Some(repository_id) = identity.repository_id else {
        return Ok(None);
    };
    let validation = validate_binding(&connection, root, &binding, &repository_id)?;
    if validation.status != "valid" {
        return Ok(None);
    }
    binding.validation = validation;
    Ok(Some(binding))
}

fn ensure_context(
    connection: &Connection,
    context_id: &str,
    project_id: &str,
) -> Result<(), String> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM diagnostic_contexts WHERE id = ?1 AND project_id = ?2",
            params![context_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to inspect Diagnostic Context: {error}"))?;
    if exists.is_none() {
        return Err(format!("Diagnostic Context {context_id} is unavailable."));
    }
    Ok(())
}

fn load_binding_history(
    connection: &Connection,
    context_id: &str,
    project_id: &str,
) -> Result<Vec<LastKnownGoodBinding>, String> {
    let mut statement = connection
        .prepare(
            "SELECT payload_json FROM last_known_good_bindings
             WHERE context_id = ?1 AND project_id = ?2",
        )
        .map_err(|error| format!("Unable to prepare last-known-good history: {error}"))?;
    statement
        .query_map(params![context_id, project_id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| format!("Unable to query last-known-good history: {error}"))?
        .map(|row| {
            let payload =
                row.map_err(|error| format!("Unable to read last-known-good history: {error}"))?;
            serde_json::from_str(&payload)
                .map_err(|error| format!("Last-known-good binding is corrupted: {error}"))
        })
        .collect()
}

fn load_binding(
    connection: &Connection,
    context_id: &str,
    binding_id: &str,
    project_id: &str,
) -> Result<LastKnownGoodBinding, String> {
    let payload = connection
        .query_row(
            "SELECT payload_json FROM last_known_good_bindings
             WHERE binding_id = ?1 AND context_id = ?2 AND project_id = ?3",
            params![binding_id, context_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read last-known-good binding: {error}"))?
        .ok_or_else(|| "Last-known-good binding is unavailable or wrong-project.".to_string())?;
    serde_json::from_str(&payload)
        .map_err(|error| format!("Last-known-good binding is corrupted: {error}"))
}

fn binding_target_is_valid(
    transaction: &Transaction<'_>,
    binding_id: &str,
) -> Result<bool, String> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM last_known_good_bindings WHERE binding_id = ?1",
            params![binding_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read last-known-good target: {error}"))?;
    let Some(payload) = payload else {
        return Ok(false);
    };
    let binding = serde_json::from_str::<LastKnownGoodBinding>(&payload)
        .map_err(|error| format!("Last-known-good target is corrupted: {error}"))?;
    Ok(binding.validation.status == "valid" && binding.validation.basis_provenance_consistent)
}
