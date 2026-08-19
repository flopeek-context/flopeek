//! SQLite adapter for immutable observation continuity and Context reconciliation.

#[allow(unused_imports)]
use super::*;

type CurrentObservation = (String, String, u64, String, String, Option<String>);

fn current_observation(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<CurrentObservation>, String> {
    connection
        .query_row(
            "SELECT observation.observation_id, observation.git_revision,
                    observation.graph_version, graph.graph_id,
                    CASE WHEN observation.dirty = 1
                         THEN observation.git_revision || '+dirty'
                         ELSE observation.git_revision END,
                    state.current_event_id
             FROM project_state state
             JOIN graph_observations observation
               ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read current observation continuity: {error}"))
}

fn current_basis(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<(GraphBasis, Option<String>)>, String> {
    Ok(current_observation(connection, project_id)?.map(
        |(observation_id, _revision, graph_version, graph_id, source_revision, event_id)| {
            (
                GraphBasis {
                    project_id: project_id.to_string(),
                    graph_id,
                    graph_version,
                    source_revision,
                    observation_id,
                },
                event_id,
            )
        },
    ))
}

pub fn get_observation_continuity(
    root: &Path,
    max_events: usize,
) -> Result<ObservationContinuity, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let Some((basis, current_event_id)) = current_basis(&connection, &project_id)? else {
        return Ok(ObservationContinuity {
            schema_version: OBSERVATION_CONTINUITY_SCHEMA.to_string(),
            project_id,
            current_observation_id: None,
            current_event_id: None,
            current_basis: None,
            events: Vec::new(),
            graph_relation: "unavailable".to_string(),
            truncated: false,
            omissions: vec!["observation continuity is unavailable before the first scan".to_string()],
            limitations: vec![
                "Observation events describe observed-after local scans, not Git ancestry or runtime order."
                    .to_string(),
            ],
        });
    };
    let event_id = current_event_id.ok_or_else(|| {
        "Current observation continuity event is unavailable for the project.".to_string()
    })?;
    let mut omissions = Vec::new();
    let bounded = max_events.min(256);
    if max_events > 256 {
        omissions.push("observation events capped at 256".to_string());
    }
    let mut reversed = Vec::new();
    let mut cursor = Some(event_id.clone());
    while let Some(id) = cursor {
        if reversed.len() >= bounded {
            omissions.push("older observation events omitted by maxEvents".to_string());
            break;
        }
        let event = connection
            .query_row(
                "SELECT event_id, project_id, observation_id, predecessor_event_id, observed_at
                 FROM observation_events WHERE event_id = ?1",
                params![id],
                |row| {
                    Ok(ObservationContinuityEvent {
                        event_id: row.get(0)?,
                        project_id: row.get(1)?,
                        observation_id: row.get(2)?,
                        predecessor_event_id: row.get(3)?,
                        relation: "observed-after".to_string(),
                        observed_at: row.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("Unable to read observation continuity event: {error}"))?;
        let Some(event) = event else {
            omissions.push("corrupt observation continuity predecessor is unavailable".to_string());
            break;
        };
        cursor = event.predecessor_event_id.clone();
        reversed.push(event);
    }
    let truncated = !omissions.is_empty();
    reversed.reverse();
    let graph_ids = reversed
        .iter()
        .filter_map(|event| {
            connection
                .query_row(
                    "SELECT graph.graph_id
                     FROM graph_observations observation
                     JOIN graph_versions graph ON graph.graph_version = observation.graph_version
                     WHERE observation.observation_id = ?1",
                    params![event.observation_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    Ok(ObservationContinuity {
        schema_version: OBSERVATION_CONTINUITY_SCHEMA.to_string(),
        project_id,
        current_observation_id: Some(basis.observation_id.clone()),
        current_event_id: Some(event_id),
        current_basis: Some(basis),
        events: reversed,
        graph_relation: if graph_ids.len() <= 1 {
            "same-structural-graph".to_string()
        } else {
            "structural-graph-changed".to_string()
        },
        truncated,
        omissions,
        limitations: vec![
            "Observation events mean observed-after in this local store; they are not Git ancestry or runtime execution order.".to_string(),
            "Continuity is bounded and reports omitted predecessor events explicitly.".to_string(),
        ],
    })
}

fn exact_candidate_uris(
    connection: &Connection,
    reference: &ContextRef,
    current_basis: &GraphBasis,
) -> Result<Vec<String>, String> {
    if reference.fingerprint_scope != "ast-and-direct-edges"
        || reference.fingerprint_contract != crate::temporal::NODE_FINGERPRINT_CONTRACT
        || reference.origin_fingerprint.is_empty()
    {
        return Ok(Vec::new());
    }
    let origin_kind = connection
        .query_row(
            "SELECT kind FROM graph_nodes WHERE graph_version = ?1 AND node_id = ?2",
            params![reference.graph_version as i64, reference.node_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read Context Ref origin kind: {error}"))?;
    let Some(origin_kind) = origin_kind else {
        return Ok(Vec::new());
    };
    let mut statement = connection
        .prepare(
            "SELECT refs.uri
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
        .map_err(|error| format!("Unable to inspect exact Context Ref candidates: {error}"))?;
    let rows = statement
        .query_map(
            params![
                reference.project_id,
                current_basis.graph_id,
                current_basis.graph_version as i64,
                origin_kind,
                reference.origin_fingerprint
            ],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|error| format!("Unable to enumerate exact Context Ref candidates: {error}"))?;
    let mut candidates = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode exact Context Ref candidates: {error}"))?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

pub fn reconcile_context(root: &Path, uri: &str) -> Result<ContextReconciliation, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let reference = match context::resolve(&connection, uri, &project_id) {
        Ok(reference) => reference,
        Err(_) => ContextRef {
            schema_version: CONTEXT_REF_SCHEMA.to_string(),
            uri: uri.to_string(),
            project_id,
            graph_id: String::new(),
            graph_version: 0,
            node_id: String::new(),
            status: "unavailable".to_string(),
            origin_observation_id: String::new(),
            origin_source_revision: String::new(),
            origin_fingerprint: String::new(),
            fingerprint_scope: "unavailable".to_string(),
            fingerprint_contract: "unavailable".to_string(),
            freshness_reason: "origin-or-ref-unavailable".to_string(),
            origin_basis: None,
            current_basis: None,
            current_event_id: String::new(),
            successor_uri: None,
        },
    };
    let (candidates, truncated) = if let Some(current_basis) = reference.current_basis.as_ref() {
        let mut candidates = exact_candidate_uris(&connection, &reference, current_basis)?;
        let truncated = candidates.len() > 32;
        candidates.truncate(32);
        (candidates, truncated)
    } else {
        (Vec::new(), false)
    };
    let mut omissions = Vec::new();
    if truncated {
        omissions.push("exact Context Ref candidates capped at 32".to_string());
    }
    Ok(ContextReconciliation {
        schema_version: CONTEXT_RECONCILIATION_SCHEMA.to_string(),
        evaluation_event_id: (!reference.current_event_id.is_empty())
            .then_some(reference.current_event_id.clone()),
        status: reference.status.clone(),
        reason: reference.freshness_reason.clone(),
        successor: reference.successor_uri.clone(),
        candidates,
        truncated,
        omissions,
        limitations: vec![
            "Exact-compatible fingerprint matches are stale reconciliation candidates; they do not claim semantic rename, runtime equivalence, or business intent.".to_string(),
            "Candidates are derived from immutable graph observations and canonical refs; no guessed mapping is stored.".to_string(),
        ],
        reference,
    })
}
