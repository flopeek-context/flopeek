//! Context Ref persistence.

#[allow(unused_imports)]
use super::*;

pub fn for_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &GraphSnapshot,
) -> Result<Vec<ContextRef>, String> {
    let mut refs = Vec::new();
    for node in snapshot
        .nodes
        .iter()
        .filter(|node| node.path.is_some())
        .take(MAX_CONTEXT_REFS)
    {
        let reference = ContextRef {
            schema_version: CONTEXT_REF_SCHEMA.to_string(),
            uri: uri(&snapshot.project_id, &snapshot.graph_id, &node.id),
            project_id: snapshot.project_id.clone(),
            graph_id: snapshot.graph_id.clone(),
            graph_version: snapshot.graph_version,
            node_id: node.id.clone(),
            status: "current".to_string(),
            origin_observation_id: snapshot.observation_id.clone(),
            origin_source_revision: snapshot.source_revision.clone(),
            origin_fingerprint: node.evidence_fingerprint.clone(),
            fingerprint_scope: "ast-and-direct-edges".to_string(),
            freshness_reason: "origin-observation-current".to_string(),
            origin_basis: Some(GraphBasis {
                project_id: snapshot.project_id.clone(),
                graph_id: snapshot.graph_id.clone(),
                graph_version: snapshot.graph_version,
                source_revision: snapshot.source_revision.clone(),
                observation_id: snapshot.observation_id.clone(),
            }),
            current_basis: Some(GraphBasis {
                project_id: snapshot.project_id.clone(),
                graph_id: snapshot.graph_id.clone(),
                graph_version: snapshot.graph_version,
                source_revision: snapshot.source_revision.clone(),
                observation_id: snapshot.observation_id.clone(),
            }),
        };
        transaction
            .execute(
                "INSERT OR IGNORE INTO context_refs(
                    uri, project_id, graph_id, graph_version, node_id, created_at,
                    origin_observation_id, origin_source_revision, origin_fingerprint, fingerprint_scope
                 ) VALUES(?1, ?2, ?3, ?4, ?5, strftime('%s','now'), ?6, ?7, ?8, ?9)",
                params![
                    reference.uri,
                    reference.project_id,
                    reference.graph_id,
                    reference.graph_version as i64,
                    reference.node_id,
                    reference.origin_observation_id,
                    reference.origin_source_revision,
                    reference.origin_fingerprint,
                    reference.fingerprint_scope,
                ],
            )
            .map_err(|error| format!("Unable to persist Context Ref: {error}"))?;
        let canonical = resolve(transaction, &reference.uri, &snapshot.project_id)?;
        let current_basis_matches = canonical.current_basis.as_ref().is_some_and(|basis| {
            basis.project_id == snapshot.project_id
                && basis.graph_id == snapshot.graph_id
                && basis.graph_version == snapshot.graph_version
                && basis.source_revision == snapshot.source_revision
                && basis.observation_id == snapshot.observation_id
        });
        let origin_basis_matches = canonical.origin_basis.as_ref().is_some_and(|basis| {
            basis.project_id == canonical.project_id
                && basis.graph_id == canonical.graph_id
                && basis.graph_version == canonical.graph_version
                && basis.source_revision == canonical.origin_source_revision
                && basis.observation_id == canonical.origin_observation_id
        });
        let origin_fingerprint_matches = transaction
            .query_row(
                "SELECT CASE WHEN ?1 = 'legacy-file-v1' THEN source.hash
                             ELSE node.evidence_fingerprint END
                 FROM graph_nodes node
                 LEFT JOIN source_files source ON source.graph_version = node.graph_version
                     AND source.path = node.path
                 WHERE node.graph_version = ?2 AND node.node_id = ?3",
                params![
                    canonical.fingerprint_scope,
                    canonical.graph_version as i64,
                    canonical.node_id,
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to validate Context Ref origin evidence: {error}"))?
            .flatten()
            .is_some_and(|fingerprint| fingerprint == canonical.origin_fingerprint);
        let origin_scope_matches = matches!(
            canonical.fingerprint_scope.as_str(),
            "ast-and-direct-edges" | "legacy-file-v1"
        );
        let canonical_status_is_valid = matches!(
            canonical.status.as_str(),
            "current" | "stale" | "unresolved"
        );
        let canonical_identity_matches = canonical.project_id == reference.project_id
            && canonical.graph_id == reference.graph_id
            && canonical.graph_version == reference.graph_version
            && canonical.node_id == reference.node_id
            && canonical_status_is_valid
            && current_basis_matches
            && (canonical.status == "unresolved"
                || (origin_basis_matches && origin_scope_matches && origin_fingerprint_matches));
        if !canonical_identity_matches {
            return Err(format!(
                "Persisted Context Ref {} does not match the current graph observation.",
                reference.uri
            ));
        }
        refs.push(canonical);
    }
    refs.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(refs)
}
