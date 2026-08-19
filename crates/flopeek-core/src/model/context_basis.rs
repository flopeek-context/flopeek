//! Canonical Diagnostic Context definition identity.

use super::DiagnosticContext;
use sha2::{Digest, Sha256};

pub const DIAGNOSTIC_CONTEXT_BASIS_SCHEMA: &str = "flopeek-diagnostic-context-basis/v1";

fn field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update((name.len() as u64).to_be_bytes());
    hasher.update(name.as_bytes());
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn list(hasher: &mut Sha256, name: &str, values: &[String], set_semantics: bool) {
    let mut canonical = values.to_vec();
    if set_semantics {
        canonical.sort();
        canonical.dedup();
    }
    field(hasher, name, &canonical.len().to_string());
    for value in canonical {
        field(hasher, name, &value);
    }
}

pub fn diagnostic_context_basis_fingerprint(context: &DiagnosticContext) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DIAGNOSTIC_CONTEXT_BASIS_SCHEMA.as_bytes());
    field(&mut hasher, "intent", &context.intent);
    field(&mut hasher, "symptom", &context.symptom);
    field(&mut hasher, "expectedBehavior", &context.expected_behavior);
    list(
        &mut hasher,
        "focusContextRefs",
        &context.focus_context_refs,
        true,
    );
    list(
        &mut hasher,
        "focusFlowRefs",
        &context.focus_flow_refs,
        true,
    );
    list(&mut hasher, "constraints", &context.constraints, false);
    list(
        &mut hasher,
        "acceptanceCriteria",
        &context.acceptance_criteria,
        false,
    );
    list(
        &mut hasher,
        "unresolvedQuestions",
        &context.unresolved_questions,
        false,
    );
    format!("sha256:{:x}", hasher.finalize())
}

