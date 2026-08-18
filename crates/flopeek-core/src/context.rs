//! Context Ref identity and freshness resolution.

use crate::model::{CONTEXT_REF_SCHEMA, ContextRef, GraphBasis, GraphSnapshot};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub const MAX_CONTEXT_REFS: usize = 256;

pub fn uri(project_id: &str, graph_id: &str, node_id: &str) -> String {
    format!("fp://local/{project_id}/{graph_id}/{node_id}")
}

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
                && basis.observation_id == snapshot.observation_id
        });
        let origin_basis_matches = canonical.origin_basis.as_ref().is_some_and(|basis| {
            basis.project_id == canonical.project_id
                && basis.graph_id == canonical.graph_id
                && basis.graph_version == canonical.graph_version
                && basis.observation_id == canonical.origin_observation_id
        });
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
            && (canonical.status == "unresolved" || origin_basis_matches);
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

pub fn resolve(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
) -> Result<ContextRef, String> {
    let Some((
        stored_project,
        graph_id,
        graph_version,
        node_id,
        origin_observation_id,
        origin_source_revision,
        origin_fingerprint,
        fingerprint_scope,
    )) = connection
        .query_row(
            "SELECT project_id, graph_id, graph_version, node_id,
                    origin_observation_id, origin_source_revision,
                    origin_fingerprint, fingerprint_scope
             FROM context_refs WHERE uri = ?1",
            params![requested_uri],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to resolve Context Ref: {error}"))?
    else {
        return Err("Context Ref is unavailable.".to_string());
    };
    if stored_project != project_id {
        return Ok(ContextRef {
            schema_version: CONTEXT_REF_SCHEMA.to_string(),
            uri: requested_uri.to_string(),
            project_id: stored_project,
            graph_id,
            graph_version: graph_version as u64,
            node_id,
            status: "wrong-project".to_string(),
            origin_observation_id,
            origin_source_revision,
            origin_fingerprint,
            fingerprint_scope: "unavailable".to_string(),
            freshness_reason: "project-id-does-not-match-request".to_string(),
            origin_basis: None,
            current_basis: None,
        });
    }

    let origin_basis = connection
        .query_row(
            "SELECT git_revision, dirty FROM graph_observations
             WHERE observation_id = ?1 AND project_id = ?2",
            params![origin_observation_id, stored_project],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| format!("Unable to read Context Ref origin observation: {error}"))?
        .map(|(revision, dirty)| GraphBasis {
            project_id: stored_project.clone(),
            graph_id: graph_id.clone(),
            graph_version: graph_version as u64,
            source_revision: if dirty != 0 {
                format!("{revision}+dirty")
            } else {
                revision
            },
            observation_id: origin_observation_id.clone(),
        });
    let current = connection
        .query_row(
            "SELECT observation.observation_id, observation.git_revision,
                    observation.dirty, observation.graph_version, graph.graph_id
             FROM project_state state
             JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph observation: {error}"))?;
    let (status, freshness_reason, current_basis) = match current {
        Some((
            current_observation_id,
            current_revision,
            current_dirty,
            current_version,
            current_graph_id,
        )) => {
            let same_observation = current_observation_id == origin_observation_id;
            let current_basis = Some(GraphBasis {
                project_id: project_id.to_string(),
                graph_id: current_graph_id,
                graph_version: current_version as u64,
                source_revision: if current_dirty != 0 {
                    format!("{current_revision}+dirty")
                } else {
                    current_revision
                },
                observation_id: current_observation_id,
            });
            let current_evidence = connection
                .query_row(
                    "SELECT node.evidence_fingerprint, source.hash
                     FROM graph_nodes node
                     LEFT JOIN source_files source ON source.graph_version = node.graph_version
                         AND source.path = node.path
                     WHERE node.graph_version = ?1 AND node.node_id = ?2",
                    params![current_version, node_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()
                .map_err(|error| format!("Unable to read current node evidence: {error}"))?;
            let current_fingerprint = current_evidence.as_ref().and_then(|(node, file)| {
                if fingerprint_scope == "legacy-file-v1" {
                    file.as_deref()
                } else {
                    Some(node.as_str())
                }
            });
            let result = if origin_observation_id.is_empty() || origin_fingerprint.is_empty() {
                (
                    "unresolved",
                    "legacy Context Ref has insufficient origin evidence",
                )
            } else if current_evidence.is_none() {
                ("stale", "origin node is missing from the current graph")
            } else if fingerprint_scope == "legacy-file-v1" {
                if current_fingerprint == Some(origin_fingerprint.as_str()) {
                    ("current", "legacy file evidence matches")
                } else {
                    ("stale", "legacy file evidence changed")
                }
            } else if current_fingerprint == Some(origin_fingerprint.as_str()) {
                if same_observation {
                    ("current", "origin-observation-current")
                } else {
                    ("current", "node AST and direct-edge fingerprint matches")
                }
            } else {
                ("stale", "node AST or direct-edge fingerprint changed")
            };
            (result.0, result.1, current_basis)
        }
        None => (
            "unavailable",
            "current graph observation is unavailable",
            None,
        ),
    };
    Ok(ContextRef {
        schema_version: CONTEXT_REF_SCHEMA.to_string(),
        uri: requested_uri.to_string(),
        project_id: stored_project,
        graph_id,
        graph_version: graph_version as u64,
        node_id,
        status: status.to_string(),
        origin_observation_id,
        origin_source_revision,
        origin_fingerprint,
        fingerprint_scope,
        freshness_reason: freshness_reason.to_string(),
        origin_basis,
        current_basis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GraphSnapshot;

    #[test]
    fn context_uri_is_deterministic_and_explicitly_branded() {
        assert_eq!(
            uri("project_a", "graph_b", "node_c"),
            "fp://local/project_a/graph_b/node_c"
        );
        let _ = GraphSnapshot {
            schema_version: "graph".to_string(),
            product: "product".to_string(),
            project_id: "project".to_string(),
            graph_id: "graph".to_string(),
            graph_version: 1,
            source_revision: "unavailable".to_string(),
            source_fingerprint: String::new(),
            observation_id: String::new(),
            files: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
        };
    }
}
