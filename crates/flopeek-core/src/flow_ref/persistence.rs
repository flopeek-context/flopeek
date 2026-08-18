//! Flow Ref persistence.

#[allow(unused_imports)]
use super::*;

pub fn for_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &GraphSnapshot,
) -> Result<Vec<FlowRef>, String> {
    let mut refs = Vec::new();
    let origin_basis = GraphBasis {
        project_id: snapshot.project_id.clone(),
        graph_id: snapshot.graph_id.clone(),
        graph_version: snapshot.graph_version,
        source_revision: snapshot.source_revision.clone(),
        observation_id: snapshot.observation_id.clone(),
    };
    for flow in &snapshot.flows {
        let reference_uri = uri(&snapshot.project_id, &snapshot.graph_id, &flow.flow_id);
        transaction
            .execute(
                "INSERT OR IGNORE INTO flow_refs(
                uri, project_id, graph_id, graph_version, flow_id,
                origin_observation_id, origin_source_revision, origin_fingerprint,
                fingerprint_scope, freshness_reason, created_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    reference_uri,
                    snapshot.project_id,
                    snapshot.graph_id,
                    snapshot.graph_version as i64,
                    flow.flow_id,
                    snapshot.observation_id,
                    snapshot.source_revision,
                    flow.fingerprint,
                    "flow-entry-steps-topology-related-tests",
                    "origin-observation-current",
                    crate::store::now_seconds_for_sql(),
                ],
            )
            .map_err(|error| format!("Unable to persist Flow Ref: {error}"))?;
        let canonical = resolve_transaction(transaction, &reference_uri, &snapshot.project_id)?;
        validate_canonical(&canonical, flow, snapshot, &origin_basis)?;
        refs.push(canonical);
    }
    refs.sort_by(|a, b| a.uri.cmp(&b.uri));
    Ok(refs)
}
