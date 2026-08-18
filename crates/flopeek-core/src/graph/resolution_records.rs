//! Canonical resolution records and bounded evidence.

use super::*;

pub fn coalesce_resolutions(records: &mut Vec<SymbolResolution>) {
    records.sort_by(resolution_ordering);
    let mut coalesced = Vec::with_capacity(records.len());
    for record in records.drain(..) {
        if let Some(existing) = coalesced.last_mut()
            && same_resolution_key(existing, &record)
        {
            existing.occurrence_count = existing
                .occurrence_count
                .saturating_add(record.occurrence_count);
        } else {
            coalesced.push(record);
        }
    }
    *records = coalesced;
}

fn same_resolution_key(left: &SymbolResolution, right: &SymbolResolution) -> bool {
    left.path == right.path
        && left.caller_node_id == right.caller_node_id
        && left.reference == right.reference
        && left.form == right.form
        && left.status == right.status
        && left.reason == right.reason
        && left.candidate_node_ids == right.candidate_node_ids
}

fn resolution_ordering(left: &SymbolResolution, right: &SymbolResolution) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.caller_node_id.cmp(&right.caller_node_id))
        .then_with(|| left.reference.cmp(&right.reference))
        .then_with(|| left.form.cmp(&right.form))
        .then_with(|| left.status.cmp(&right.status))
        .then_with(|| left.reason.cmp(&right.reason))
        .then_with(|| left.candidate_node_ids.cmp(&right.candidate_node_ids))
}

pub fn resolution_evidence(facts: &[TypeScriptFacts], legacy: bool) -> ResolutionEvidence {
    if legacy
        || facts.iter().any(|fact| {
            fact.schema_version != crate::model::TYPESCRIPT_FACTS_SCHEMA
                || fact.parser != crate::typescript::PARSER_IDENTITY
        })
    {
        return ResolutionEvidence {
            schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
            status: "unavailable".to_string(),
            records: Vec::new(),
            truncated: false,
            omissions: vec!["legacy-facts-without-resolution-evidence".to_string()],
        };
    }
    let mut records = Vec::new();
    for fact in facts {
        records.extend(fact.resolution_records.iter().cloned());
    }
    coalesce_resolutions(&mut records);
    let mut truncated = false;
    let mut omissions = Vec::new();
    if records.len() > MAX_RESOLUTION_RECORDS {
        records.truncate(MAX_RESOLUTION_RECORDS);
        truncated = true;
        omissions.push(format!(
            "resolution records capped at {MAX_RESOLUTION_RECORDS}"
        ));
    }
    ResolutionEvidence {
        schema_version: TYPESCRIPT_RESOLUTION_SCHEMA.to_string(),
        status: if truncated { "truncated" } else { "complete" }.to_string(),
        records,
        truncated,
        omissions,
    }
}
