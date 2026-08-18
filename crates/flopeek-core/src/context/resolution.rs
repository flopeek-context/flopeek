//! Context Ref resolution.

#[allow(unused_imports)]
use super::*;

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
        fingerprint_contract,
    )) = connection
        .query_row(
            "SELECT project_id, graph_id, graph_version, node_id,
                    origin_observation_id, origin_source_revision,
                    origin_fingerprint, fingerprint_scope, fingerprint_contract
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
                    row.get::<_, String>(8)?,
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
            fingerprint_contract: "unavailable".to_string(),
            freshness_reason: "project-id-does-not-match-request".to_string(),
            origin_basis: None,
            current_basis: None,
            current_event_id: String::new(),
            successor_uri: None,
        });
    }

    let origin_basis = connection
        .query_row(
            "SELECT observation.git_revision, observation.dirty,
                    observation.graph_version, graph.graph_id
             FROM graph_observations observation
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE observation.observation_id = ?1 AND observation.project_id = ?2",
            params![origin_observation_id, stored_project],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read Context Ref origin observation: {error}"))?
        .map(
            |(revision, dirty, origin_graph_version, origin_graph_id)| GraphBasis {
                project_id: stored_project.clone(),
                graph_id: origin_graph_id,
                graph_version: origin_graph_version as u64,
                source_revision: if dirty != 0 {
                    format!("{revision}+dirty")
                } else {
                    revision
                },
                observation_id: origin_observation_id.clone(),
            },
        );
    let origin_evidence = if let Some(basis) = origin_basis.as_ref() {
        connection
            .query_row(
                "SELECT CASE WHEN ?1 = 'legacy-file-v1' THEN source.hash
                             ELSE node.evidence_fingerprint END
                 FROM graph_nodes node
                 LEFT JOIN source_files source ON source.graph_version = node.graph_version
                     AND source.path = node.path
                 WHERE node.graph_version = ?2 AND node.node_id = ?3",
                params![fingerprint_scope, basis.graph_version as i64, node_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to read Context Ref origin evidence: {error}"))?
            .flatten()
    } else {
        None
    };
    let origin_scope_valid = matches!(
        fingerprint_scope.as_str(),
        "ast-and-direct-edges" | "legacy-file-v1"
    );
    let origin_contract_valid = match fingerprint_scope.as_str() {
        "ast-and-direct-edges" => {
            fingerprint_contract == crate::temporal::NODE_FINGERPRINT_CONTRACT
        }
        "legacy-file-v1" => {
            fingerprint_contract == crate::temporal::LEGACY_FILE_FINGERPRINT_CONTRACT
        }
        _ => false,
    };
    let origin_metadata_valid = origin_scope_valid
        && origin_contract_valid
        && origin_basis.is_some()
        && origin_evidence
            .as_deref()
            .is_some_and(|fingerprint| fingerprint == origin_fingerprint);
    let origin_kind = origin_basis.as_ref().and_then(|basis| {
        connection
            .query_row(
                "SELECT kind FROM graph_nodes WHERE graph_version = ?1 AND node_id = ?2",
                params![basis.graph_version as i64, node_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
    });
    let current = connection
        .query_row(
            "SELECT observation.observation_id, observation.git_revision,
                    observation.dirty, observation.graph_version, graph.graph_id,
                    state.current_event_id
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
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph observation: {error}"))?;
    let evaluated_event_id = current
        .as_ref()
        .and_then(|value| value.5.clone())
        .unwrap_or_default();
    let (status, freshness_reason, current_basis) = match current {
        Some((
            current_observation_id,
            current_revision,
            current_dirty,
            current_version,
            current_graph_id,
            _current_event_id,
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
            } else if origin_basis.is_none() {
                ("unavailable", "origin graph observation is unavailable")
            } else if !origin_metadata_valid {
                ("unavailable", "origin evidence metadata is inconsistent")
            } else if current_evidence.is_none() {
                ("stale", "node-identity-missing")
            } else if fingerprint_contract != crate::temporal::NODE_FINGERPRINT_CONTRACT {
                (
                    "unresolved",
                    "legacy Context Ref lacks a compatible fingerprint contract",
                )
            } else if fingerprint_scope == "legacy-file-v1" {
                (
                    "unresolved",
                    "legacy file evidence lacks node fingerprint contract",
                )
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
    let mut status = status.to_string();
    let mut freshness_reason = freshness_reason.to_string();
    let mut successor_uri = None;
    if status == "stale"
        && fingerprint_scope == "ast-and-direct-edges"
        && fingerprint_contract == crate::temporal::NODE_FINGERPRINT_CONTRACT
        && origin_kind.is_some()
        && current_basis.is_some()
    {
        let basis = current_basis.as_ref().expect("checked above");
        let mut statement = connection
            .prepare(
                "SELECT node.node_id, refs.uri, refs.fingerprint_scope,
                        refs.fingerprint_contract, refs.origin_fingerprint
                 FROM graph_nodes node
                 LEFT JOIN context_refs refs
                   ON refs.project_id = ?1
                  AND refs.graph_id = ?2
                  AND refs.graph_version = ?3
                  AND refs.node_id = node.node_id
                 WHERE node.graph_version = ?3
                   AND node.kind = ?4
                   AND node.evidence_fingerprint = ?5
                 ORDER BY node.node_id, refs.uri",
            )
            .map_err(|error| format!("Unable to inspect exact Context Ref successors: {error}"))?;
        let rows = statement
            .query_map(
                params![
                    project_id,
                    basis.graph_id,
                    basis.graph_version as i64,
                    origin_kind.as_ref().expect("checked above"),
                    origin_fingerprint
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(|error| {
                format!("Unable to enumerate exact Context Ref successors: {error}")
            })?;
        let successors = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode exact Context Ref successors: {error}"))?;
        if successors.len() > 1 {
            status = "stale".to_string();
            freshness_reason = "exact-successor-ambiguous".to_string();
        } else if let Some((_, uri, scope, contract, fingerprint)) = successors.into_iter().next() {
            if let Some(uri) = uri {
                if scope.as_deref() == Some("ast-and-direct-edges")
                    && contract.as_deref() == Some(crate::temporal::NODE_FINGERPRINT_CONTRACT)
                    && fingerprint.as_deref() == Some(origin_fingerprint.as_str())
                {
                    status = "superseded".to_string();
                    freshness_reason = "unique-exact-compatible-fingerprint".to_string();
                    successor_uri = Some(uri);
                } else {
                    freshness_reason = "successor-ref-unavailable".to_string();
                }
            } else {
                freshness_reason = "successor-ref-unavailable".to_string();
            }
        }
    }
    Ok(ContextRef {
        schema_version: CONTEXT_REF_SCHEMA.to_string(),
        uri: requested_uri.to_string(),
        project_id: stored_project,
        graph_id,
        graph_version: graph_version as u64,
        node_id,
        status,
        origin_observation_id,
        origin_source_revision,
        origin_fingerprint,
        fingerprint_scope,
        fingerprint_contract,
        freshness_reason,
        origin_basis,
        current_basis,
        current_event_id: evaluated_event_id,
        successor_uri,
    })
}
