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
    list(&mut hasher, "focusFlowRefs", &context.focus_flow_refs, true);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> DiagnosticContext {
        DiagnosticContext {
            schema_version: crate::model::DIAGNOSTIC_CONTEXT_SCHEMA.to_string(),
            id: "context-basis".to_string(),
            project_id: "project-basis".to_string(),
            context_definition_revision: 1,
            context_basis_fingerprint: String::new(),
            memory_revision: 0,
            intent: "diagnose".to_string(),
            symptom: "timeout".to_string(),
            expected_behavior: "completes".to_string(),
            focus_context_refs: vec![
                "fp://local/project/node/b".to_string(),
                "fp://local/project/node/a".to_string(),
            ],
            focus_flow_refs: vec!["fp://local/project/flow/f".to_string()],
            current_graph_basis: crate::model::GraphBasis {
                project_id: "project-basis".to_string(),
                graph_id: "graph-a".to_string(),
                graph_version: 1,
                source_revision: "revision-a".to_string(),
                observation_id: "observation-a".to_string(),
            },
            last_known_good_basis: None,
            last_known_good_binding_id: None,
            last_known_good_candidate_id: None,
            constraints: vec!["bounded".to_string()],
            acceptance_criteria: vec!["verified".to_string()],
            unresolved_questions: vec!["runtime?".to_string()],
            actor: "alice".to_string(),
            created_at: 1,
            status: "open".to_string(),
            supersedes: None,
        }
    }

    #[test]
    fn definition_fingerprint_tracks_only_normative_definition_fields() {
        let original = context();
        let fingerprint = diagnostic_context_basis_fingerprint(&original);

        let mut reordered = original.clone();
        reordered.focus_context_refs.reverse();
        reordered
            .focus_context_refs
            .push(reordered.focus_context_refs[0].clone());
        reordered.memory_revision = 99;
        reordered.current_graph_basis.graph_id = "graph-b".to_string();
        reordered.last_known_good_candidate_id = Some("candidate".to_string());
        reordered.actor = "bob".to_string();
        reordered.created_at = 2;
        reordered.status = "closed".to_string();
        assert_eq!(
            diagnostic_context_basis_fingerprint(&reordered),
            fingerprint
        );

        let mutations: Vec<Box<dyn Fn(&mut DiagnosticContext)>> = vec![
            Box::new(|value| value.intent = "audit".to_string()),
            Box::new(|value| value.symptom = "different".to_string()),
            Box::new(|value| value.expected_behavior = "different".to_string()),
            Box::new(|value| {
                value
                    .focus_context_refs
                    .push("fp://local/project/node/c".to_string())
            }),
            Box::new(|value| value.focus_flow_refs.clear()),
            Box::new(|value| value.constraints.push("offline".to_string())),
            Box::new(|value| value.acceptance_criteria.push("reviewed".to_string())),
            Box::new(|value| value.unresolved_questions.clear()),
        ];
        for mutate in mutations {
            let mut changed = original.clone();
            mutate(&mut changed);
            assert_ne!(diagnostic_context_basis_fingerprint(&changed), fingerprint);
        }
    }
}
