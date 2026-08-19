//! Diagnostic memory persistence.

#[allow(unused_imports)]
use super::*;

pub fn create_diagnostic_context(
    root: &Path,
    mut context: crate::model::DiagnosticContext,
) -> Result<crate::model::DiagnosticContext, String> {
    context.context_definition_revision = 1;
    context.memory_revision = 0;
    context.context_basis_fingerprint =
        crate::model::diagnostic_context_basis_fingerprint(&context);
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
    for uri in &context.focus_flow_refs {
        let resolved = resolve_flow(root, uri)?;
        if resolved.project_id != project_id || resolved.status != "current" {
            return Err(format!(
                "Diagnostic Context focus Flow Ref {uri} is not current."
            ));
        }
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
    if context.created_at == 0 {
        context.created_at = now_seconds() as u64;
    }
    let payload = serde_json::to_string(&context)
        .map_err(|error| format!("Unable to encode Diagnostic Context: {error}"))?;
    transaction
        .execute(
            "INSERT INTO diagnostic_contexts(
                 id, project_id, revision, context_definition_revision,
                 context_basis_fingerprint, memory_revision, payload_json, created_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                context.id,
                context.project_id,
                context.context_definition_revision,
                context.context_definition_revision,
                context.context_basis_fingerprint,
                context.memory_revision,
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
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM diagnostic_assertions WHERE context_id = ?1",
            params![assertion.context_id],
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
    updated_context.memory_revision = assertion.revision;
    let updated_payload = serde_json::to_string(&updated_context)
        .map_err(|error| format!("Unable to encode updated Diagnostic Context: {error}"))?;
    transaction
        .execute(
            "UPDATE diagnostic_contexts SET memory_revision = ?1, payload_json = ?2 WHERE id = ?3",
            params![
                updated_context.memory_revision,
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

pub(super) fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

pub(crate) fn now_seconds_for_sql() -> i64 {
    now_seconds()
}
