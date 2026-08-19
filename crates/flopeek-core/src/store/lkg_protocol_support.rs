// SQLite adapter for Last-Known-Good Protocol 1.0.
//
// The reducer itself lives in `model::lkg_protocol`; this module owns
// transaction boundaries, observation lookup, materialized state, and the
// untrusted/trusted API split.

use crate::model::{
    EvidenceContract, LastKnownGoodApplicability, LastKnownGoodCandidate,
    LastKnownGoodEvent, LastKnownGoodIntegrity, LastKnownGoodProposalRequest,
    LastKnownGoodReviewPacket, LastKnownGoodState, LastKnownGoodTransitionRequest,
    LKG_CANDIDATE_SCHEMA, LKG_EVENT_SCHEMA, LKG_INTEGRITY_COMPLETE,
    LKG_INTEGRITY_PARTIAL, LKG_REVIEW_PACKET_SCHEMA,
    reduce_last_known_good,
};
use rusqlite::{OptionalExtension, Transaction, params};
use std::path::Path;

const DEFAULT_MAX_PATHS: usize = 4_096;
const HARD_MAX_PATHS: usize = 10_000;
const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const HARD_MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;

fn bound(value: Option<usize>, default: usize, hard: usize) -> usize {
    value.unwrap_or(default).min(hard)
}

fn validate_text(value: &str, name: &str, max: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{name} is empty, invalid, or unbounded."));
    }
    Ok(())
}

fn encode<T: serde::Serialize>(value: &T, label: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("Unable to encode {label}: {error}"))
}

fn decode<T: serde::de::DeserializeOwned>(payload: &str, label: &str) -> Result<T, String> {
    serde_json::from_str(payload).map_err(|error| format!("Unable to decode {label}: {error}"))
}

fn context_snapshot(
    transaction: &Transaction<'_>,
    context_id: &str,
) -> Result<crate::model::DiagnosticContext, String> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1",
            params![context_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read Diagnostic Context: {error}"))?
        .ok_or_else(|| "Diagnostic Context is unavailable.".to_string())?;
    decode(&payload, "Diagnostic Context")
}

fn event_payload(event: &LastKnownGoodEvent) -> Result<String, String> {
    encode(event, "LastKnownGoodEvent")
}

fn candidate_payload(candidate: &LastKnownGoodCandidate) -> Result<String, String> {
    encode(candidate, "LastKnownGoodCandidate")
}

fn candidate_rows(
    transaction: &Transaction<'_>,
    context_id: &str,
) -> Result<Vec<LastKnownGoodCandidate>, String> {
    let mut statement = transaction
        .prepare(
            "SELECT payload_json FROM last_known_good_candidates
             WHERE context_id = ?1 ORDER BY proposed_at, candidate_id",
        )
        .map_err(|error| format!("Unable to prepare LKG candidate query: {error}"))?;
    statement
        .query_map(params![context_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to query LKG candidates: {error}"))?
        .map(|row| {
            row.map_err(|error| format!("Unable to read LKG candidate: {error}"))
                .and_then(|payload| decode(&payload, "LastKnownGoodCandidate"))
        })
        .collect()
}

fn event_rows(
    transaction: &Transaction<'_>,
    context_id: &str,
) -> Result<Vec<LastKnownGoodEvent>, String> {
    let mut statement = transaction
        .prepare(
            "SELECT payload_json FROM last_known_good_events
             WHERE context_id = ?1 ORDER BY created_at, event_id",
        )
        .map_err(|error| format!("Unable to prepare LKG event query: {error}"))?;
    statement
        .query_map(params![context_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to query LKG events: {error}"))?
        .map(|row| {
            row.map_err(|error| format!("Unable to read LKG event: {error}"))
                .and_then(|payload| decode(&payload, "LastKnownGoodEvent"))
        })
        .collect()
}

fn reduced(
    transaction: &Transaction<'_>,
    context_id: &str,
) -> Result<(Vec<LastKnownGoodCandidate>, crate::model::ReducedLkg), String> {
    let candidates = candidate_rows(transaction, context_id)?;
    let events = event_rows(transaction, context_id)?;
    let reduced = reduce_last_known_good(context_id, &candidates, &events)
        .map_err(|reason| format!("lkg-state-corrupt:{reason}"))?;
    Ok((candidates, reduced))
}

fn validate_evidence(evidence: &[crate::model::EvidenceReference]) -> Result<(), String> {
    if evidence.len() > 256 {
        return Err("LKG evidence is unbounded.".to_string());
    }
    for reference in evidence {
        crate::diagnostic::validate_evidence(reference)?;
    }
    Ok(())
}

fn observation_basis(
    transaction: &Transaction<'_>,
    project_id: &str,
    revision: &str,
) -> Result<Option<(String, crate::model::GraphBasis, EvidenceContract, bool)>, String> {
    let row = transaction
        .query_row(
            "SELECT observation.observation_id, observation.graph_version,
                    observation.git_revision, graph.graph_id,
                    graph.graph_schema_version, graph.graph_derivation_id,
                    graph.node_fingerprint_contract,
                    graph.truncated
             FROM graph_observations observation
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE observation.project_id = ?1 AND observation.git_revision = ?2
                   AND observation.dirty = 0
             ORDER BY observation.observed_at DESC, observation.observation_id DESC
             LIMIT 1",
            params![project_id, revision],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)? != 0,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to inspect LKG observation: {error}"))?;
    let Some((observation_id, graph_version, source_revision, graph_id, schema, derivation, contract, truncated)) = row else {
        return Ok(None);
    };
    Ok(Some((
        observation_id.clone(),
        crate::model::GraphBasis {
            project_id: project_id.to_string(),
            graph_id,
            graph_version,
            source_revision,
            observation_id,
        },
        EvidenceContract {
            graph_schema_version: schema,
            graph_derivation_id: derivation,
            node_fingerprint_contract: contract,
        },
        truncated,
    )))
}

fn candidate_for_request(
    transaction: &Transaction<'_>,
    root: &Path,
    request: &LastKnownGoodProposalRequest,
) -> Result<LastKnownGoodCandidate, String> {
    validate_text(&request.context_id, "contextId", 128)?;
    validate_text(&request.git_revision, "gitRevision", 128)?;
    validate_text(&request.actor, "actor", 256)?;
    validate_text(&request.reason, "reason", 4_096)?;
    validate_text(&request.idempotency_key, "idempotencyKey", 128)?;
    validate_evidence(&request.evidence)?;
    let context = context_snapshot(transaction, &request.context_id)?;
    let identity = crate::identity::resolve(root)?;
    if context.project_id != identity.project_id {
        return Err("Diagnostic Context is unavailable or wrong-project.".to_string());
    }
    let repository_id = identity.repository_id.ok_or_else(|| {
        "repository-identity-unavailable: LKG requires the root manifest.".to_string()
    })?;
    let resolved_revision = crate::diagnostic::resolve_last_known_good_revision(root, &request.git_revision)?;
    let expected = crate::model::expected_behavior_fingerprint(&context.expected_behavior);
    let max_paths = bound(request.max_paths, DEFAULT_MAX_PATHS, HARD_MAX_PATHS);
    let max_snapshot_bytes = bound(
        request.max_snapshot_bytes,
        DEFAULT_MAX_SNAPSHOT_BYTES,
        HARD_MAX_SNAPSHOT_BYTES,
    );
    let basis = observation_basis(transaction, &identity.project_id, &resolved_revision)?;
    let (observation_id, graph_basis, evidence_contract, integrity) = if let Some((observation, basis, contract, truncated)) = basis {
        let contract_compatible = contract.graph_schema_version == crate::model::GRAPH_SCHEMA
            && contract.graph_derivation_id == crate::graph::GRAPH_DERIVATION_ID
            && contract.node_fingerprint_contract == crate::temporal::NODE_FINGERPRINT_CONTRACT;
        let mut limitations = Vec::new();
        if truncated {
            limitations.push("historical-observation-truncated-by-materialization-bounds".to_string());
        }
        if !contract_compatible {
            limitations.push("candidate-evidence-contract-incompatible".to_string());
        }
        (
            Some(observation),
            Some(basis),
            Some(contract),
            LastKnownGoodIntegrity {
                status: if truncated {
                    LKG_INTEGRITY_PARTIAL.to_string()
                } else if !contract_compatible {
                    crate::model::LKG_INTEGRITY_INVALID.to_string()
                } else {
                    LKG_INTEGRITY_COMPLETE.to_string()
                },
                revision_available: true,
                observation_available: true,
                graph_basis_available: true,
                evidence_contract_compatible: contract_compatible,
                limitations,
            },
        )
    } else {
        (
            None,
            None,
            None,
            LastKnownGoodIntegrity {
                status: LKG_INTEGRITY_PARTIAL.to_string(),
                revision_available: true,
                observation_available: false,
                graph_basis_available: false,
                evidence_contract_compatible: false,
                limitations: vec![format!(
                    "historical-observation-not-materialized-within-{}-paths-{}-bytes",
                    max_paths, max_snapshot_bytes
                )],
            },
        )
    };
    let id_material = format!(
        "flopeek-lkg-candidate/v1\0{}\0{}\0{}\0{}\0{}\0{}",
        identity.project_id,
        request.context_id,
        context.revision,
        resolved_revision,
        expected,
        request.idempotency_key
    );
    Ok(LastKnownGoodCandidate {
        schema_version: LKG_CANDIDATE_SCHEMA.to_string(),
        candidate_id: format!("lkgc_{}", blake3::hash(id_material.as_bytes()).to_hex()),
        repository_id,
        project_id: identity.project_id,
        context_id: request.context_id.clone(),
        context_revision: context.revision,
        expected_behavior_fingerprint: expected,
        git_revision: resolved_revision,
        observation_id,
        graph_basis,
        evidence_contract,
        proposed_by: request.actor.clone(),
        proposed_at: now_seconds() as u64,
        evidence: request.evidence.clone(),
        reason: request.reason.clone(),
        integrity,
    })
}

fn proposal_retry_matches(
    transaction: &Transaction<'_>,
    root: &Path,
    request: &LastKnownGoodProposalRequest,
    event: &LastKnownGoodEvent,
    candidate: &LastKnownGoodCandidate,
) -> Result<bool, String> {
    if event.event_type != "PROPOSE"
        || event.actor != request.actor
        || event.reason != request.reason
        || event.evidence != request.evidence
        || event.predecessor_event_id != request.expected_tip_event_id
    {
        return Ok(false);
    }
    let context = context_snapshot(transaction, &request.context_id)?;
    let identity = crate::identity::resolve(root)?;
    let resolved_revision = match crate::diagnostic::resolve_last_known_good_revision(
        root,
        &request.git_revision,
    ) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(candidate.project_id == identity.project_id
        && candidate.context_revision == context.revision
        && candidate.expected_behavior_fingerprint
            == crate::model::expected_behavior_fingerprint(&context.expected_behavior)
        && candidate.git_revision == resolved_revision)
}

fn check_expected_tip(
    reduced: &crate::model::ReducedLkg,
    expected: Option<&str>,
) -> Result<(), String> {
    if reduced.state.tip_event_id.as_deref() != expected {
        return Err("stale-lifecycle-tip".to_string());
    }
    Ok(())
}

fn persist_state(
    transaction: &Transaction<'_>,
    state: &LastKnownGoodState,
) -> Result<(), String> {
    let payload = encode(state, "LastKnownGoodState")?;
    transaction
        .execute(
            "INSERT INTO last_known_good_state(
                 context_id, tip_event_id, active_candidate_id,
                 pending_candidate_id, payload_json
             ) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(context_id) DO UPDATE SET
                 tip_event_id=excluded.tip_event_id,
                 active_candidate_id=excluded.active_candidate_id,
                 pending_candidate_id=excluded.pending_candidate_id,
                 payload_json=excluded.payload_json",
            params![
                state.context_id,
                state.tip_event_id,
                state.active_candidate_id,
                state.pending_candidate_id,
                payload
            ],
        )
        .map_err(|error| format!("Unable to persist LKG state: {error}"))?;
    Ok(())
}

fn state_with_applicability(
    transaction: &Transaction<'_>,
    root: &Path,
    candidates: &[LastKnownGoodCandidate],
    mut state: LastKnownGoodState,
) -> Result<LastKnownGoodState, String> {
    let candidate_id = state
        .active_candidate_id
        .as_deref()
        .or(state.pending_candidate_id.as_deref());
    if let Some(candidate_id) = candidate_id {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.candidate_id == candidate_id)
            .ok_or_else(|| "lkg-state-candidate-unavailable".to_string())?;
        let context = context_snapshot(transaction, &state.context_id)?;
        let applicability = evaluate_applicability(transaction, root, candidate, &context)?;
        state.applicability_status = applicability.status;
        state.limitations.extend(applicability.limitations);
    } else {
        state.applicability_status = "unavailable".to_string();
    }
    state.limitations.sort();
    state.limitations.dedup();
    Ok(state)
}

fn update_context_projection(
    transaction: &Transaction<'_>,
    context_id: &str,
    candidate_id: Option<&str>,
) -> Result<(), String> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM diagnostic_contexts WHERE id = ?1",
            params![context_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Unable to read Context projection: {error}"))?;
    let mut context = decode::<crate::model::DiagnosticContext>(&payload, "Diagnostic Context")?;
    context.last_known_good_candidate_id = candidate_id.map(ToOwned::to_owned);
    let updated = encode(&context, "Diagnostic Context")?;
    transaction
        .execute(
            "UPDATE diagnostic_contexts SET payload_json = ?1 WHERE id = ?2",
            params![updated, context_id],
        )
        .map_err(|error| format!("Unable to update Context LKG projection: {error}"))?;
    Ok(())
}
