//! Normative Last-Known-Good Protocol 1.0 domain records.
//!
//! These records deliberately keep the immutable candidate, append-only event,
//! and reduced lifecycle state separate.  The reducer in this module is pure;
//! SQLite/Git/JSONL adapters live outside the model boundary.

use super::{EvidenceContract, EvidenceReference, GraphBasis};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LKG_CANDIDATE_SCHEMA: &str = "flopeek-last-known-good-candidate/v1";
pub const LKG_EVENT_SCHEMA: &str = "flopeek-last-known-good-event/v1";
pub const LKG_STATE_SCHEMA: &str = "flopeek-last-known-good-state/v1";
pub const LKG_REVIEW_PACKET_SCHEMA: &str = "flopeek-last-known-good-review-packet/v1";
pub const LKG_PROTOCOL_SCHEMA: &str = "flopeek-lkg-protocol/v1";

pub fn expected_behavior_fingerprint(expected_behavior: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"flopeek-lkg-expected-behavior/v1\0");
    hasher.update(expected_behavior.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub const LKG_INTEGRITY_COMPLETE: &str = "complete";
pub const LKG_INTEGRITY_PARTIAL: &str = "partial";
pub const LKG_INTEGRITY_INVALID: &str = "invalid";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodIntegrity {
    pub status: String,
    #[serde(default)]
    pub revision_available: bool,
    #[serde(default)]
    pub observation_available: bool,
    #[serde(default)]
    pub graph_basis_available: bool,
    #[serde(default)]
    pub evidence_contract_compatible: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodCandidate {
    pub schema_version: String,
    pub candidate_id: String,
    pub repository_id: String,
    pub project_id: String,
    pub context_id: String,
    pub context_revision: u64,
    pub expected_behavior_fingerprint: String,
    pub git_revision: String,
    #[serde(default)]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub graph_basis: Option<GraphBasis>,
    #[serde(default)]
    pub evidence_contract: Option<EvidenceContract>,
    pub proposed_by: String,
    pub proposed_at: u64,
    pub evidence: Vec<EvidenceReference>,
    pub reason: String,
    pub integrity: LastKnownGoodIntegrity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodEvent {
    pub schema_version: String,
    pub event_id: String,
    pub repository_id: String,
    pub project_id: String,
    pub context_id: String,
    pub event_type: String,
    pub candidate_id: String,
    #[serde(default)]
    pub replaces_candidate_id: Option<String>,
    #[serde(default)]
    pub predecessor_event_id: Option<String>,
    pub actor: String,
    pub actor_kind: String,
    pub actor_trust: String,
    pub reason: String,
    pub evidence: Vec<EvidenceReference>,
    pub created_at: u64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodState {
    pub schema_version: String,
    pub context_id: String,
    #[serde(default)]
    pub tip_event_id: Option<String>,
    #[serde(default)]
    pub active_candidate_id: Option<String>,
    #[serde(default)]
    pub pending_candidate_id: Option<String>,
    pub lifecycle_status: String,
    pub applicability_status: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodApplicability {
    pub status: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodReviewPacket {
    pub schema_version: String,
    pub context_id: String,
    pub context: super::DiagnosticContext,
    pub candidate: LastKnownGoodCandidate,
    pub state: LastKnownGoodState,
    pub applicability: LastKnownGoodApplicability,
    #[serde(default)]
    pub structural_delta: Option<serde_json::Value>,
    pub confirmable: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodProposalRequest {
    pub context_id: String,
    pub git_revision: String,
    pub actor: String,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceReference>,
    pub expected_tip_event_id: Option<String>,
    pub idempotency_key: String,
    #[serde(default)]
    pub max_paths: Option<usize>,
    #[serde(default)]
    pub max_snapshot_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodTransitionRequest {
    pub context_id: String,
    pub actor: String,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceReference>,
    pub expected_tip_event_id: Option<String>,
    pub idempotency_key: String,
    #[serde(default)]
    pub candidate_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducedLkg {
    pub events: Vec<LastKnownGoodEvent>,
    pub state: LastKnownGoodState,
}

/// Reduce a complete append-only event stream.  No persistence or repository
/// concerns are allowed here; malformed streams fail closed.
pub fn reduce_last_known_good(
    context_id: &str,
    candidates: &[LastKnownGoodCandidate],
    events: &[LastKnownGoodEvent],
) -> Result<ReducedLkg, String> {
    let mut candidate_ids = std::collections::BTreeSet::new();
    let mut candidates_by_id = std::collections::BTreeMap::new();
    let mut repository_id: Option<&str> = None;
    let mut project_id: Option<&str> = None;
    for candidate in candidates {
        if candidate.context_id != context_id
            || !candidate_ids.insert(candidate.candidate_id.as_str())
        {
            return Err("lkg-candidate-context-or-identity-corrupt".to_string());
        }
        if candidate.schema_version != LKG_CANDIDATE_SCHEMA
            || candidate.repository_id.is_empty()
            || candidate.project_id.is_empty()
            || candidate.git_revision.is_empty()
        {
            return Err("lkg-candidate-contract-invalid".to_string());
        }
        if repository_id.is_some_and(|value| value != candidate.repository_id.as_str())
            || project_id.is_some_and(|value| value != candidate.project_id.as_str())
        {
            return Err("lkg-candidate-provenance-inconsistent".to_string());
        }
        repository_id.get_or_insert(candidate.repository_id.as_str());
        project_id.get_or_insert(candidate.project_id.as_str());
        candidates_by_id.insert(candidate.candidate_id.as_str(), candidate);
    }
    let mut by_id = std::collections::BTreeMap::new();
    for event in events {
        if event.context_id != context_id
            || event.schema_version != LKG_EVENT_SCHEMA
            || event.event_id.is_empty()
            || event.repository_id.is_empty()
            || event.project_id.is_empty()
            || !by_id
                .insert(event.event_id.clone(), event.clone())
                .is_none()
            || !candidate_ids.contains(event.candidate_id.as_str())
        {
            return Err("lkg-event-context-or-identity-corrupt".to_string());
        }
        let candidate = candidates_by_id
            .get(event.candidate_id.as_str())
            .ok_or_else(|| "lkg-event-candidate-unavailable".to_string())?;
        if event.repository_id != candidate.repository_id
            || event.project_id != candidate.project_id
        {
            return Err("lkg-event-candidate-provenance-mismatch".to_string());
        }
        if !matches!(
            event.event_type.as_str(),
            "PROPOSE" | "CONFIRM" | "REJECT" | "REVOKE"
        ) {
            return Err("lkg-event-type-unsupported".to_string());
        }
        if event.event_type != "CONFIRM" && event.replaces_candidate_id.is_some() {
            return Err("lkg-event-replacement-on-non-confirm-invalid".to_string());
        }
        if event.event_type == "PROPOSE" && event.replaces_candidate_id.is_some() {
            return Err("lkg-proposal-replacement-invalid".to_string());
        }
    }
    let mut successors = std::collections::BTreeMap::<String, String>::new();
    let roots = events
        .iter()
        .filter(|event| event.predecessor_event_id.is_none())
        .map(|event| event.event_id.clone())
        .collect::<Vec<_>>();
    if events.is_empty() {
        return Ok(ReducedLkg {
            events: Vec::new(),
            state: LastKnownGoodState {
                schema_version: LKG_STATE_SCHEMA.to_string(),
                context_id: context_id.to_string(),
                lifecycle_status: "none".to_string(),
                applicability_status: "unavailable".to_string(),
                ..Default::default()
            },
        });
    }
    if roots.len() != 1 {
        return Err("lkg-event-chain-root-invalid".to_string());
    }
    for event in events {
        if let Some(predecessor) = event.predecessor_event_id.as_deref()
            && (predecessor == event.event_id
                || !by_id.contains_key(predecessor)
                || successors
                    .insert(predecessor.to_string(), event.event_id.clone())
                    .is_some())
        {
            return Err("lkg-event-chain-predecessor-invalid".to_string());
        }
    }
    let mut ordered = Vec::with_capacity(events.len());
    let mut current = roots[0].clone();
    while let Some(event) = by_id.get(&current) {
        ordered.push(event.clone());
        let Some(next) = successors.get(&current) else {
            break;
        };
        current = next.clone();
        if ordered.len() > events.len() {
            return Err("lkg-event-chain-cycle".to_string());
        }
    }
    if ordered.len() != events.len() {
        return Err("lkg-event-chain-disconnected".to_string());
    }

    let mut active = None;
    let mut pending = None;
    let mut seen_candidates = std::collections::BTreeSet::<String>::new();
    for event in &ordered {
        match event.event_type.as_str() {
            "PROPOSE" => {
                if seen_candidates.contains(&event.candidate_id) {
                    return Err("lkg-candidate-proposed-more-than-once".to_string());
                }
                if pending.replace(event.candidate_id.clone()).is_some() {
                    return Err("lkg-multiple-pending-candidates".to_string());
                }
                seen_candidates.insert(event.candidate_id.clone());
            }
            "CONFIRM" => {
                if pending.as_deref() != Some(event.candidate_id.as_str()) {
                    return Err("lkg-confirm-target-not-pending".to_string());
                }
                if let Some(active_id) = active.as_deref() {
                    if event.replaces_candidate_id.as_deref() != Some(active_id) {
                        return Err("lkg-confirm-replacement-target-invalid".to_string());
                    }
                } else if event.replaces_candidate_id.is_some() {
                    return Err("lkg-confirm-replacement-without-active".to_string());
                }
                active = Some(event.candidate_id.clone());
                pending = None;
                seen_candidates.insert(event.candidate_id.clone());
            }
            "REJECT" => {
                if pending.as_deref() != Some(event.candidate_id.as_str()) {
                    return Err("lkg-reject-target-not-pending".to_string());
                }
                pending = None;
                seen_candidates.insert(event.candidate_id.clone());
            }
            "REVOKE" => {
                if active.as_deref() != Some(event.candidate_id.as_str()) {
                    return Err("lkg-revoke-target-not-active".to_string());
                }
                active = None;
                seen_candidates.insert(event.candidate_id.clone());
            }
            _ => unreachable!(),
        }
    }
    let lifecycle_status = if active.is_some() {
        "active"
    } else if pending.is_some() {
        "pending"
    } else {
        "inactive"
    };
    Ok(ReducedLkg {
        events: ordered.clone(),
        state: LastKnownGoodState {
            schema_version: LKG_STATE_SCHEMA.to_string(),
            context_id: context_id.to_string(),
            tip_event_id: ordered.last().map(|event| event.event_id.clone()),
            active_candidate_id: active,
            pending_candidate_id: pending,
            lifecycle_status: lifecycle_status.to_string(),
            applicability_status: "unavailable".to_string(),
            limitations: Vec::new(),
        },
    })
}
