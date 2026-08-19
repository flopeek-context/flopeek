//! Attributed, append-only last-known-good bindings.

use super::{EvidenceReference, GitBasis, GraphBasis, LAST_KNOWN_GOOD_SCHEMA};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodValidation {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub revision_available: bool,
    #[serde(default)]
    pub repository_match: bool,
    #[serde(default)]
    pub first_parent_range_available: bool,
    #[serde(default)]
    pub evidence_contract_compatible: bool,
    #[serde(default)]
    pub basis_provenance_consistent: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodBinding {
    pub schema_version: String,
    pub binding_id: String,
    pub repository_id: String,
    pub project_id: String,
    pub context_id: String,
    pub git_revision: String,
    #[serde(default)]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub graph_basis: Option<GraphBasis>,
    pub actor: String,
    pub actor_kind: String,
    pub evidence: Vec<EvidenceReference>,
    pub status: String,
    #[serde(default)]
    pub predecessor_binding_id: Option<String>,
    #[serde(default)]
    pub target_binding_id: Option<String>,
    #[serde(default)]
    pub supersedes_binding_id: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub validation: LastKnownGoodValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LastKnownGoodResolution {
    pub schema_version: String,
    pub context_id: String,
    pub status: String,
    #[serde(default)]
    pub binding: Option<LastKnownGoodBinding>,
    #[serde(default)]
    pub legacy_basis: Option<GitBasis>,
    pub limitations: Vec<String>,
}

impl LastKnownGoodBinding {
    pub fn new_schema_version() -> String {
        LAST_KNOWN_GOOD_SCHEMA.to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LastKnownGoodLifecycle {
    pub history: Vec<LastKnownGoodBinding>,
    pub latest_event: Option<LastKnownGoodBinding>,
    pub active_confirmed: Option<LastKnownGoodBinding>,
    pub pending_proposal: Option<LastKnownGoodBinding>,
}

pub(crate) fn reduce_last_known_good_lifecycle(
    bindings: Vec<LastKnownGoodBinding>,
) -> Result<LastKnownGoodLifecycle, String> {
    if bindings.is_empty() {
        return Ok(LastKnownGoodLifecycle::default());
    }
    let binding_count = bindings.len();
    let by_id = bindings
        .into_iter()
        .map(|binding| (binding.binding_id.clone(), binding))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != binding_count {
        return Err("Last-known-good lifecycle contains duplicate identities.".to_string());
    }
    let expected_context = by_id
        .values()
        .next()
        .map(|binding| (binding.project_id.as_str(), binding.context_id.as_str()))
        .expect("non-empty lifecycle");
    if by_id.values().any(|binding| {
        binding.project_id != expected_context.0 || binding.context_id != expected_context.1
    }) {
        return Err("Last-known-good lifecycle metadata is inconsistent.".to_string());
    }
    let mut successors = BTreeMap::<String, String>::new();
    let mut roots = BTreeSet::new();
    for binding in by_id.values() {
        match binding.predecessor_binding_id.as_deref() {
            Some(predecessor) => {
                if predecessor == binding.binding_id || !by_id.contains_key(predecessor) {
                    return Err("Last-known-good lifecycle predecessor is corrupted.".to_string());
                }
                if successors
                    .insert(predecessor.to_string(), binding.binding_id.clone())
                    .is_some()
                {
                    return Err("Last-known-good lifecycle is branched.".to_string());
                }
            }
            None => {
                roots.insert(binding.binding_id.clone());
            }
        }
    }
    if roots.len() != 1 {
        return Err("Last-known-good lifecycle must have exactly one root.".to_string());
    }
    let mut history = Vec::with_capacity(by_id.len());
    let mut current = roots.into_iter().next().expect("one lifecycle root");
    loop {
        let binding = by_id
            .get(&current)
            .cloned()
            .ok_or_else(|| "Last-known-good lifecycle is corrupted.".to_string())?;
        history.push(binding);
        let Some(next) = successors.get(&current) else {
            break;
        };
        current = next.clone();
        if history.len() > by_id.len() {
            return Err("Last-known-good lifecycle contains a cycle.".to_string());
        }
    }
    if history.len() != by_id.len() {
        return Err("Last-known-good lifecycle is disconnected.".to_string());
    }
    let mut active_confirmed: Option<LastKnownGoodBinding> = None;
    let mut pending_proposal: Option<LastKnownGoodBinding> = None;
    for binding in &history {
        if binding.status != "confirmed" && binding.supersedes_binding_id.is_some() {
            return Err(
                "Only a confirmed binding may supersede an active last-known-good.".to_string(),
            );
        }
        if binding
            .target_binding_id
            .as_deref()
            .is_some_and(|target| target == binding.binding_id)
            || binding
                .supersedes_binding_id
                .as_deref()
                .is_some_and(|target| target == binding.binding_id)
        {
            return Err("A last-known-good binding cannot target itself.".to_string());
        }
        match binding.status.as_str() {
            "proposed" => {
                if pending_proposal.is_some() {
                    return Err(
                        "Last-known-good lifecycle has multiple pending proposals.".to_string()
                    );
                }
                if binding.target_binding_id.is_some() || binding.supersedes_binding_id.is_some() {
                    return Err(
                        "A proposed last-known-good binding cannot target another binding."
                            .to_string(),
                    );
                }
                pending_proposal = Some(binding.clone());
            }
            "confirmed" => {
                if let Some(target) = binding.target_binding_id.as_deref() {
                    if pending_proposal
                        .as_ref()
                        .map(|value| value.binding_id.as_str())
                        != Some(target)
                    {
                        return Err(
                            "Last-known-good confirmation target is not the pending proposal."
                                .to_string(),
                        );
                    }
                } else if pending_proposal.is_some() {
                    return Err(
                        "A confirmation with a pending proposal requires targetBindingId."
                            .to_string(),
                    );
                } else if active_confirmed.is_some() {
                    return Err(
                        "A direct confirmation cannot replace an active last-known-good."
                            .to_string(),
                    );
                }
                match (
                    active_confirmed
                        .as_ref()
                        .map(|value| value.binding_id.as_str()),
                    binding.supersedes_binding_id.as_deref(),
                ) {
                    (Some(active), Some(target)) if active == target => {}
                    (Some(_), _) => {
                        return Err(
                            "A replacement confirmation must target the active last-known-good."
                                .to_string(),
                        );
                    }
                    (None, Some(_)) => {
                        return Err(
                            "A confirmation cannot supersede without an active last-known-good."
                                .to_string(),
                        );
                    }
                    (None, None) => {}
                }
                active_confirmed = Some(binding.clone());
                pending_proposal = None;
            }
            "rejected" => {
                if binding.target_binding_id.as_deref()
                    != pending_proposal
                        .as_ref()
                        .map(|value| value.binding_id.as_str())
                {
                    return Err(
                        "A rejected last-known-good binding must target the pending proposal."
                            .to_string(),
                    );
                }
                pending_proposal = None;
            }
            "revoked" | "superseded" => {
                if binding.target_binding_id.as_deref()
                    != active_confirmed
                        .as_ref()
                        .map(|value| value.binding_id.as_str())
                {
                    return Err(
                        "A terminal last-known-good event must target the active binding."
                            .to_string(),
                    );
                }
                active_confirmed = None;
            }
            _ => {
                return Err("Last-known-good lifecycle status is corrupted.".to_string());
            }
        }
    }
    Ok(LastKnownGoodLifecycle {
        latest_event: history.last().cloned(),
        history,
        active_confirmed,
        pending_proposal,
    })
}
