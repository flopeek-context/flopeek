//! Flow Ref resolution.

#[allow(unused_imports)]
use super::*;

pub fn resolve(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
) -> Result<FlowRef, String> {
    resolve_connection(connection, requested_uri, project_id)
}

fn resolve_connection(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
) -> Result<FlowRef, String> {
    let (uri_project, graph_id, flow_id) = parse_uri(requested_uri)?;
    if uri_project != project_id {
        return Ok(FlowRef {
            schema_version: FLOW_REF_SCHEMA.to_string(),
            uri: requested_uri.to_string(),
            project_id: uri_project,
            graph_id,
            graph_version: 0,
            flow_id,
            status: "wrong-project".to_string(),
            origin_observation_id: String::new(),
            origin_source_revision: String::new(),
            origin_fingerprint: String::new(),
            fingerprint_scope: "unavailable".to_string(),
            freshness_reason: "project-id-does-not-match-request".to_string(),
            origin_basis: None,
            current_basis: None,
        });
    }
    match resolve_query(connection, requested_uri, project_id) {
        Ok(reference) => Ok(reference),
        Err(error) if error == "Flow Ref is unavailable." => Ok(FlowRef {
            schema_version: FLOW_REF_SCHEMA.to_string(),
            uri: requested_uri.to_string(),
            project_id: project_id.to_string(),
            graph_id,
            graph_version: 0,
            flow_id,
            status: "unavailable".to_string(),
            origin_observation_id: String::new(),
            origin_source_revision: String::new(),
            origin_fingerprint: String::new(),
            fingerprint_scope: "unavailable".to_string(),
            freshness_reason: "origin-flow-ref-unavailable".to_string(),
            origin_basis: None,
            current_basis: None,
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn resolve_transaction(
    transaction: &Transaction<'_>,
    requested_uri: &str,
    project_id: &str,
) -> Result<FlowRef, String> {
    let (uri_project, _graph_id, _flow_id) = parse_uri(requested_uri)?;
    if uri_project != project_id {
        return Err("Flow Ref belongs to a different project.".to_string());
    }
    let row = transaction
        .query_row(
            "SELECT project_id, graph_id, graph_version, flow_id, origin_observation_id,
                origin_source_revision, origin_fingerprint, fingerprint_scope, freshness_reason
         FROM flow_refs WHERE uri = ?1",
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
        .map_err(|error| format!("Unable to read Flow Ref: {error}"))?
        .ok_or_else(|| "Flow Ref is unavailable.".to_string())?;
    resolve_from_row(transaction, requested_uri, project_id, row)
}

fn resolve_query(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
) -> Result<FlowRef, String> {
    let (uri_project, _graph_id, _flow_id) = parse_uri(requested_uri)?;
    if uri_project != project_id {
        return Err("Flow Ref belongs to a different project.".to_string());
    }
    let row = connection
        .query_row(
            "SELECT project_id, graph_id, graph_version, flow_id, origin_observation_id,
                origin_source_revision, origin_fingerprint, fingerprint_scope, freshness_reason
         FROM flow_refs WHERE uri = ?1",
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
        .map_err(|error| format!("Unable to read Flow Ref: {error}"))?
        .ok_or_else(|| "Flow Ref is unavailable.".to_string())?;
    resolve_from_row(connection, requested_uri, project_id, row)
}

fn resolve_from_row(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
    row: (
        String,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
    ),
) -> Result<FlowRef, String> {
    let (
        stored_project,
        graph_id,
        graph_version,
        flow_id,
        origin_observation_id,
        origin_source_revision,
        origin_fingerprint,
        fingerprint_scope,
        _stored_reason,
    ) = row;
    let (uri_project, uri_graph, uri_flow) = parse_uri(requested_uri)?;
    if stored_project != project_id || uri_project != project_id {
        return Err("Flow Ref belongs to a different project.".to_string());
    }
    if uri_graph != graph_id || uri_flow != flow_id {
        return Err("Flow Ref metadata conflicts with its URI.".to_string());
    }
    let origin_lookup = query_basis(
        connection,
        &origin_observation_id,
        graph_id.clone(),
        graph_version as u64,
    );
    let origin_basis = origin_lookup
        .as_ref()
        .ok()
        .filter(|basis| {
            basis.project_id == stored_project
                && basis.graph_id == graph_id
                && basis.graph_version == graph_version as u64
                && basis.source_revision == origin_source_revision
        })
        .cloned();
    let current = connection.query_row(
        "SELECT graph.graph_id, graph.graph_version, observation.observation_id,
                CASE WHEN observation.dirty = 1 THEN observation.git_revision || '+dirty' ELSE observation.git_revision END,
                graph_flows.fingerprint
         FROM project_state state
         JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
         JOIN graph_versions graph ON graph.graph_version = observation.graph_version
         LEFT JOIN graph_flows ON graph_flows.graph_version = graph.graph_version AND graph_flows.flow_id = ?2
         WHERE state.project_id = ?1",
        params![project_id, flow_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?)),
    ).optional().map_err(|error| format!("Unable to read current Flow basis: {error}"))?;
    let (status, reason, current_basis) = if origin_basis.is_none() || origin_fingerprint.is_empty()
    {
        (
            "unavailable".to_string(),
            if origin_fingerprint.is_empty() {
                "origin-flow-evidence-unavailable".to_string()
            } else if origin_lookup.is_err() {
                "origin-observation-unavailable".to_string()
            } else {
                "origin-flow-basis-inconsistent".to_string()
            },
            current.map(
                |(current_graph_id, current_version, current_observation, current_revision, _)| {
                    GraphBasis {
                        project_id: project_id.to_string(),
                        graph_id: current_graph_id,
                        graph_version: current_version as u64,
                        source_revision: current_revision,
                        observation_id: current_observation,
                    }
                },
            ),
        )
    } else {
        match current {
            None => (
                "unavailable".to_string(),
                "origin-graph-unavailable".to_string(),
                None,
            ),
            Some((
                current_graph_id,
                current_version,
                current_observation,
                current_revision,
                fingerprint,
            )) => {
                let basis = GraphBasis {
                    project_id: project_id.to_string(),
                    graph_id: current_graph_id.clone(),
                    graph_version: current_version as u64,
                    source_revision: current_revision,
                    observation_id: current_observation.clone(),
                };
                match fingerprint {
                    None => (
                        "stale".to_string(),
                        "flow-identity-missing".to_string(),
                        Some(basis),
                    ),
                    Some(value)
                        if value == origin_fingerprint
                            && current_observation == origin_observation_id =>
                    {
                        (
                            "current".to_string(),
                            "origin-observation-current".to_string(),
                            Some(basis),
                        )
                    }
                    Some(value) if value == origin_fingerprint => (
                        "current".to_string(),
                        "flow-fingerprint-match".to_string(),
                        Some(basis),
                    ),
                    Some(_) => (
                        "stale".to_string(),
                        "flow-fingerprint-changed".to_string(),
                        Some(basis),
                    ),
                }
            }
        }
    };
    Ok(FlowRef {
        schema_version: FLOW_REF_SCHEMA.to_string(),
        uri: requested_uri.to_string(),
        project_id: stored_project,
        graph_id,
        graph_version: graph_version as u64,
        flow_id,
        status,
        origin_observation_id,
        origin_source_revision,
        origin_fingerprint,
        fingerprint_scope,
        freshness_reason: reason,
        origin_basis,
        current_basis,
    })
}

fn query_basis(
    connection: &Connection,
    observation_id: &str,
    graph_id: String,
    graph_version: u64,
) -> Result<GraphBasis, String> {
    let (project_id, source_revision, actual_graph_version, actual_graph_id) = connection
        .query_row(
            "SELECT observation.project_id,
                    CASE WHEN observation.dirty = 1 THEN observation.git_revision || '+dirty' ELSE observation.git_revision END,
                    observation.graph_version, graph.graph_id
             FROM graph_observations observation
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE observation.observation_id = ?1",
            params![observation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read Flow origin observation: {error}"))?
        .ok_or_else(|| "Flow origin observation is unavailable.".to_string())?;
    if actual_graph_version != graph_version as i64 || actual_graph_id != graph_id {
        return Err("Flow origin graph metadata is inconsistent.".to_string());
    }
    Ok(GraphBasis {
        project_id,
        graph_id,
        graph_version,
        source_revision,
        observation_id: observation_id.to_string(),
    })
}

fn parse_uri(value: &str) -> Result<(String, String, String), String> {
    let mut parts = value
        .strip_prefix("fp://local/")
        .ok_or_else(|| "Flow Ref URI must use fp://local/.".to_string())?
        .split('/');
    let project = parts.next().unwrap_or_default().to_string();
    let graph = parts.next().unwrap_or_default().to_string();
    if parts.next() != Some("flow") {
        return Err("Flow Ref URI must contain /flow/.".to_string());
    }
    let flow = parts.next().unwrap_or_default().to_string();
    if project.is_empty() || graph.is_empty() || flow.is_empty() || parts.next().is_some() {
        return Err("Flow Ref URI is malformed.".to_string());
    }
    Ok((project, graph, flow))
}

pub(super) fn validate_canonical(
    reference: &FlowRef,
    flow: &ContextFlow,
    snapshot: &GraphSnapshot,
    _basis: &GraphBasis,
) -> Result<(), String> {
    let current_matches = reference.current_basis.as_ref().is_some_and(|basis| {
        basis.project_id == snapshot.project_id
            && basis.graph_id == snapshot.graph_id
            && basis.graph_version == snapshot.graph_version
            && basis.source_revision == snapshot.source_revision
            && basis.observation_id == snapshot.observation_id
    });
    let origin_matches = reference.origin_basis.as_ref().is_some_and(|basis| {
        basis.project_id == reference.project_id
            && basis.graph_id == reference.graph_id
            && basis.graph_version == reference.graph_version
            && basis.source_revision == reference.origin_source_revision
            && basis.observation_id == reference.origin_observation_id
    });
    if reference.project_id != snapshot.project_id
        || reference.graph_id != snapshot.graph_id
        || reference.flow_id != flow.flow_id
        || reference.origin_fingerprint != flow.fingerprint
        || reference.fingerprint_scope != "flow-entry-steps-topology-related-tests"
        || !origin_matches
        || !current_matches
    {
        return Err("Canonical Flow Ref metadata conflicts with the scanned flow.".to_string());
    }
    Ok(())
}
