//! Versioned diagnostic memory and bounded historical candidates.
//!
//! This module deliberately keeps static graph facts, human/agent assertions and
//! historical candidates in separate records.  Git path history is evidence of a
//! change, never proof that the change caused a runtime symptom.

use crate::model::{
    ContextFlow, ContextRef, DIAGNOSTIC_ASSERTION_SCHEMA, DIAGNOSTIC_CONTEXT_SCHEMA,
    DIAGNOSTIC_PACKET_SCHEMA, DiagnosticAssertion, DiagnosticContext, DiagnosticLimits,
    DiagnosticPacket, EvidenceReference, GitBasis, GraphBasis, GraphNode,
    HISTORICAL_CANDIDATE_SCHEMA, HISTORICAL_DIAGNOSIS_SCHEMA, HISTORICAL_SNAPSHOT_SCHEMA,
    HistoricalCandidate, HistoricalDiagnosis, HistoricalSnapshot, RelatedTestEvidence,
};
use crate::module_resolution::{MAX_CONFIG_BYTES, MAX_CONFIG_FILES, config_extends};
use crate::store;
use crate::typescript::PARSER_IDENTITY;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 8 * 1024;
const MAX_LIST_ITEMS: usize = 256;
const HISTORY_DERIVATION_ID: &str = "typescript-historical-delta-v9";

const ALLOWED_INTENTS: &[&str] = &["diagnose", "audit", "verify-fix"];
const ALLOWED_CONTEXT_STATUSES: &[&str] = &["open", "reconciled", "resolved", "superseded"];
const ALLOWED_ASSERTION_KINDS: &[&str] = &[
    "observation",
    "hypothesis",
    "finding",
    "remediation",
    "verification",
];
const ALLOWED_ASSERTION_STATUSES: &[&str] = &[
    "proposed",
    "confirmed",
    "rejected",
    "superseded",
    "implemented",
    "verified",
];
const ALLOWED_EVIDENCE_CLASSES: &[&str] = &[
    "static",
    "observation",
    "hypothesis",
    "finding",
    "remediation",
    "verification",
];

mod continuity;
mod continuity_evidence;
mod diagnosis;
mod focus;
mod git;
mod last_known_good;
mod packet;
mod ranking;
mod snapshot;
#[cfg(test)]
mod tests;
mod validation;

use focus::{focus_paths, validate_basis};
use git::{
    current_head, first_parent, git_changed_paths, git_is_dirty, git_log, git_output,
    git_show_bytes, git_tree_paths, historical_config_paths, resolve_revision, safe_relative_path,
};
use ranking::{CommitRecord, FocusPathSets, historical_delta_reasons};
use snapshot::load_or_build_historical_snapshot;
use validation::{
    is_typescript_path, validate_choice, validate_id, validate_list, validate_revision,
    validate_string_list, validate_text,
};

pub use continuity::{HistoricalContinuityLimits, get_historical_context_continuity};
pub use diagnosis::diagnose_history;
pub(crate) use last_known_good::{resolve_last_known_good_revision, validate_first_parent_range};
pub use packet::build_packet;

pub fn validate_context(context: &DiagnosticContext) -> Result<(), String> {
    if context.schema_version != DIAGNOSTIC_CONTEXT_SCHEMA {
        return Err(format!(
            "Diagnostic Context schema must be {DIAGNOSTIC_CONTEXT_SCHEMA}."
        ));
    }
    validate_id("Diagnostic Context id", &context.id)?;
    validate_id("Diagnostic Context project id", &context.project_id)?;
    validate_choice("intent", &context.intent, ALLOWED_INTENTS)?;
    validate_choice("status", &context.status, ALLOWED_CONTEXT_STATUSES)?;
    validate_text("symptom", &context.symptom)?;
    validate_text("expectedBehavior", &context.expected_behavior)?;
    validate_text("actor", &context.actor)?;
    validate_list(
        "focusContextRefs",
        &context.focus_context_refs,
        MAX_LIST_ITEMS,
    )?;
    for reference in &context.focus_context_refs {
        if !reference.starts_with("fp://local/") || reference.len() > 512 {
            return Err(
                "focusContextRefs must contain bounded fp://local Context Refs.".to_string(),
            );
        }
    }
    validate_list("focusFlowRefs", &context.focus_flow_refs, MAX_LIST_ITEMS)?;
    for reference in &context.focus_flow_refs {
        if !reference.starts_with("fp://local/")
            || !reference.contains("/flow/")
            || reference.len() > 768
        {
            return Err("focusFlowRefs must contain bounded fp://local Flow Refs.".to_string());
        }
    }
    validate_basis(&context.current_graph_basis)?;
    if let Some(last_known_good) = &context.last_known_good_basis {
        validate_revision(&last_known_good.revision)?;
    }
    validate_string_list("constraints", &context.constraints)?;
    validate_string_list("acceptanceCriteria", &context.acceptance_criteria)?;
    validate_string_list("unresolvedQuestions", &context.unresolved_questions)?;
    if let Some(supersedes) = &context.supersedes {
        validate_id("supersedes", supersedes)?;
        if supersedes == &context.id {
            return Err("A Diagnostic Context cannot supersede itself.".to_string());
        }
    }
    Ok(())
}

pub fn validate_assertion(assertion: &DiagnosticAssertion) -> Result<(), String> {
    if assertion.schema_version != DIAGNOSTIC_ASSERTION_SCHEMA {
        return Err(format!(
            "Diagnostic Assertion schema must be {DIAGNOSTIC_ASSERTION_SCHEMA}."
        ));
    }
    validate_id("Diagnostic Assertion id", &assertion.id)?;
    validate_id("contextId", &assertion.context_id)?;
    validate_choice("kind", &assertion.kind, ALLOWED_ASSERTION_KINDS)?;
    validate_choice("status", &assertion.status, ALLOWED_ASSERTION_STATUSES)?;
    validate_text("actor", &assertion.actor)?;
    validate_text("statement", &assertion.statement)?;
    validate_list("evidence", &assertion.evidence, MAX_LIST_ITEMS)?;
    for evidence in &assertion.evidence {
        validate_evidence(evidence)?;
    }
    if assertion.status == "superseded" && assertion.supersedes.is_none() {
        return Err("A superseded assertion must declare supersedes.".to_string());
    }
    if let Some(supersedes) = &assertion.supersedes {
        validate_id("supersedes", supersedes)?;
        if supersedes == &assertion.id {
            return Err("A Diagnostic Assertion cannot supersede itself.".to_string());
        }
    }
    Ok(())
}

pub fn validate_evidence(evidence: &EvidenceReference) -> Result<(), String> {
    validate_choice(
        "evidenceClass",
        &evidence.evidence_class,
        ALLOWED_EVIDENCE_CLASSES,
    )?;
    validate_text("evidence kind", &evidence.kind)?;
    if evidence.reference.is_empty()
        || evidence.reference.len() > 1024
        || evidence.reference.contains(['\r', '\n', '\0'])
    {
        return Err("evidence reference must be bounded and single-line.".to_string());
    }
    let lower = evidence.reference.to_ascii_lowercase();
    if [
        "password=",
        "token=",
        "secret=",
        "private_key",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err("evidence reference cannot contain credential material.".to_string());
    }
    Ok(())
}

pub fn graph_basis(graph_snapshot: &crate::model::GraphSnapshot) -> GraphBasis {
    GraphBasis {
        project_id: graph_snapshot.project_id.clone(),
        graph_id: graph_snapshot.graph_id.clone(),
        graph_version: graph_snapshot.graph_version,
        source_revision: graph_snapshot.source_revision.clone(),
        observation_id: graph_snapshot.observation_id.clone(),
    }
}

pub fn context_ref_json(reference: &ContextRef) -> Result<Value, String> {
    serde_json::to_value(reference).map_err(|error| error.to_string())
}
