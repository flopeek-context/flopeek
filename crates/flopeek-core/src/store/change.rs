//! SQLite adapter for bounded adjacent observation deltas.

use super::*;
use crate::model::{
    EvidenceContract, GraphBasis, GraphEdge, GraphNode, ObservationBasisRelations,
    ObservationDelta, ObservationDeltaCounts,
};
use crate::temporal::{self, DeltaLimits, FlowFingerprint, ObservationEvidence};

#[derive(Debug, Clone)]
struct EventRecord {
    event_id: String,
    project_id: String,
    observation_id: String,
    predecessor_event_id: Option<String>,
}

pub fn get_observation_delta(
    root: &Path,
    event_id: Option<&str>,
    limits: DeltaLimits,
) -> Result<ObservationDelta, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let target = if let Some(event_id) = event_id {
        connection
            .query_row(
                "SELECT event_id, project_id, observation_id, predecessor_event_id
                 FROM observation_events WHERE event_id = ?1",
                params![event_id],
                |row| {
                    Ok(EventRecord {
                        event_id: row.get(0)?,
                        project_id: row.get(1)?,
                        observation_id: row.get(2)?,
                        predecessor_event_id: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("Unable to read observation delta event: {error}"))?
    } else {
        let current_event = connection
            .query_row(
                "SELECT current_event_id FROM project_state WHERE project_id = ?1",
                params![project_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("Unable to read current observation delta event: {error}"))?
            .flatten();
        current_event
            .as_deref()
            .map(|id| {
                connection
                    .query_row(
                        "SELECT event_id, project_id, observation_id, predecessor_event_id
                         FROM observation_events WHERE event_id = ?1",
                        params![id],
                        |row| {
                            Ok(EventRecord {
                                event_id: row.get(0)?,
                                project_id: row.get(1)?,
                                observation_id: row.get(2)?,
                                predecessor_event_id: row.get(3)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(|error| {
                        format!("Unable to read current observation delta event: {error}")
                    })
            })
            .transpose()?
            .flatten()
    };

    let Some(target) = target else {
        return Ok(unavailable_delta(
            &project_id,
            "event-unavailable",
            None,
            None,
            None,
            false,
        ));
    };
    if target.project_id != project_id {
        return Ok(unavailable_delta(
            &project_id,
            "wrong-project-event",
            Some(target.event_id),
            None,
            None,
            true,
        ));
    }
    let to = match load_observation(&connection, &target.observation_id) {
        Ok(to) => to,
        Err(error) if error.starts_with("observation-source-manifest-invalid") => {
            return Ok(unavailable_delta(
                &project_id,
                "observation-source-manifest-invalid",
                Some(target.event_id),
                None,
                None,
                false,
            ));
        }
        Err(error) => return Err(error),
    };
    let Some(to) = to else {
        return Ok(unavailable_delta(
            &project_id,
            "target-observation-unavailable",
            Some(target.event_id),
            None,
            None,
            false,
        ));
    };
    if target.predecessor_event_id.is_none() {
        return Ok(unavailable_delta(
            &project_id,
            "predecessor-event-unavailable",
            Some(target.event_id),
            Some(to.basis),
            Some(to.contract),
            false,
        ));
    }
    let predecessor_id = target
        .predecessor_event_id
        .as_deref()
        .expect("checked predecessor");
    let predecessor = connection
        .query_row(
            "SELECT event_id, project_id, observation_id, predecessor_event_id
             FROM observation_events WHERE event_id = ?1",
            params![predecessor_id],
            |row| {
                Ok(EventRecord {
                    event_id: row.get(0)?,
                    project_id: row.get(1)?,
                    observation_id: row.get(2)?,
                    predecessor_event_id: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read predecessor observation delta event: {error}"))?;
    let Some(predecessor) = predecessor else {
        return Ok(unavailable_delta(
            &project_id,
            "predecessor-event-unavailable",
            Some(target.event_id),
            Some(to.basis),
            Some(to.contract),
            false,
        ));
    };
    if predecessor.project_id != project_id {
        return Ok(unavailable_delta(
            &project_id,
            "predecessor-event-wrong-project",
            Some(target.event_id),
            Some(to.basis),
            Some(to.contract),
            true,
        ));
    }
    let from = match load_observation(&connection, &predecessor.observation_id) {
        Ok(from) => from,
        Err(error) if error.starts_with("observation-source-manifest-invalid") => {
            return Ok(unavailable_delta(
                &project_id,
                "observation-source-manifest-invalid",
                Some(target.event_id),
                Some(to.basis),
                Some(to.contract),
                false,
            ));
        }
        Err(error) => return Err(error),
    };
    let Some(from) = from else {
        return Ok(unavailable_delta(
            &project_id,
            "predecessor-observation-unavailable",
            Some(target.event_id),
            Some(to.basis),
            Some(to.contract),
            false,
        ));
    };
    Ok(temporal::compare_observations(
        &project_id,
        &predecessor.event_id,
        &target.event_id,
        &from,
        &to,
        &limits,
    ))
}

fn load_observation(
    connection: &Connection,
    observation_id: &str,
) -> Result<Option<ObservationEvidence>, String> {
    let metadata = connection
        .query_row(
            "SELECT observation.project_id, observation.graph_version,
                    observation.git_revision, observation.dirty,
                    observation.source_fingerprint,
                    observation.source_manifest_json,
                    observation.module_resolution_fingerprint,
                    observation.module_resolution_effective_fingerprint,
                    observation.entry_manifest_fingerprint,
                    observation.entry_effective_fingerprint,
                    graph.graph_id, graph.graph_schema_version,
                    graph.graph_derivation_id, graph.node_fingerprint_contract
             FROM graph_observations observation
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE observation.observation_id = ?1",
            params![observation_id],
            |row| {
                let project_id = row.get::<_, String>(0)?;
                let graph_version = row.get::<_, i64>(1)? as u64;
                let git_revision = row.get::<_, String>(2)?;
                let dirty = row.get::<_, i64>(3)? != 0;
                let source_revision = if dirty {
                    format!("{git_revision}+dirty")
                } else {
                    git_revision
                };
                Ok((
                    project_id,
                    graph_version,
                    source_revision,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read observation delta metadata: {error}"))?;
    let Some((
        project_id,
        graph_version,
        source_revision,
        source_fingerprint,
        source_manifest_json,
        module_resolution_exact_fingerprint,
        module_resolution_effective_fingerprint,
        entry_manifest_fingerprint,
        entry_effective_fingerprint,
        graph_id,
        graph_schema_version,
        graph_derivation_id,
        node_fingerprint_contract,
    )) = metadata
    else {
        return Ok(None);
    };

    let source_files = observation::decode_source_manifest(&source_manifest_json)?;
    let nodes = connection
        .prepare(
            "SELECT node_id, kind, path, name, language, evidence_fingerprint
             FROM graph_nodes WHERE graph_version = ?1 ORDER BY node_id",
        )
        .map_err(|error| format!("Unable to prepare observation delta node query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(GraphNode {
                id: row.get(0)?,
                kind: row.get(1)?,
                path: row.get(2)?,
                name: row.get(3)?,
                language: row.get(4)?,
                evidence_fingerprint: row.get(5)?,
            })
        })
        .map_err(|error| format!("Unable to query observation delta nodes: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode observation delta nodes: {error}"))?;
    let edges = connection
        .prepare(
            "SELECT from_id, to_id, kind, evidence FROM graph_edges
             WHERE graph_version = ?1 ORDER BY from_id, to_id, kind, evidence",
        )
        .map_err(|error| format!("Unable to prepare observation delta edge query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(GraphEdge {
                from: row.get(0)?,
                to: row.get(1)?,
                kind: row.get(2)?,
                evidence: row.get(3)?,
            })
        })
        .map_err(|error| format!("Unable to query observation delta edges: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode observation delta edges: {error}"))?;
    let flows = connection
        .prepare(
            "SELECT flow_id, fingerprint FROM graph_flows
             WHERE graph_version = ?1 ORDER BY flow_id",
        )
        .map_err(|error| format!("Unable to prepare observation delta flow query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(FlowFingerprint {
                flow_id: row.get(0)?,
                fingerprint: row.get(1)?,
            })
        })
        .map_err(|error| format!("Unable to query observation delta flows: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode observation delta flows: {error}"))?;

    Ok(Some(ObservationEvidence {
        basis: GraphBasis {
            project_id,
            graph_id,
            graph_version,
            source_revision,
            observation_id: observation_id.to_string(),
        },
        contract: EvidenceContract {
            graph_schema_version,
            graph_derivation_id,
            node_fingerprint_contract,
        },
        source_fingerprint,
        source_files,
        module_resolution_exact_fingerprint,
        module_resolution_effective_fingerprint,
        entry_manifest_fingerprint,
        entry_effective_fingerprint,
        nodes,
        edges,
        flows,
    }))
}

/// Compare two persisted observations without treating the pair as a Git
/// parent/child relation.  LKG review uses this bounded structural comparison
/// to show candidate-to-current evidence; the normal public delta endpoint
/// continues to require an adjacent observation event.
pub(crate) fn compare_observation_ids(
    connection: &Connection,
    project_id: &str,
    from_observation_id: &str,
    to_observation_id: &str,
    limits: DeltaLimits,
) -> Result<ObservationDelta, String> {
    let from = match load_observation(connection, from_observation_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Ok(unavailable_delta(
                project_id,
                "candidate-observation-unavailable",
                None,
                None,
                None,
                false,
            ));
        }
        Err(error) if error.starts_with("observation-source-manifest-invalid") => {
            return Ok(unavailable_delta(
                project_id,
                "observation-source-manifest-invalid",
                None,
                None,
                None,
                false,
            ));
        }
        Err(error) => return Err(error),
    };
    let to = match load_observation(connection, to_observation_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Ok(unavailable_delta(
                project_id,
                "current-observation-unavailable",
                None,
                Some(from.basis),
                Some(from.contract),
                false,
            ));
        }
        Err(error) if error.starts_with("observation-source-manifest-invalid") => {
            return Ok(unavailable_delta(
                project_id,
                "observation-source-manifest-invalid",
                None,
                Some(from.basis),
                Some(from.contract),
                false,
            ));
        }
        Err(error) => return Err(error),
    };
    if from.basis.project_id != project_id || to.basis.project_id != project_id {
        return Ok(unavailable_delta(
            project_id,
            "wrong-project-observation",
            None,
            Some(to.basis),
            Some(to.contract),
            true,
        ));
    }
    let mut delta = temporal::compare_observations(
        project_id,
        from_observation_id,
        to_observation_id,
        &from,
        &to,
        &limits,
    );
    delta.from_event_id = None;
    delta.to_event_id = None;
    delta.limitations.push(
        "LKG candidate-to-current comparison is observation evidence, not Git ancestry or runtime order."
            .to_string(),
    );
    delta.limitations.sort();
    delta.limitations.dedup();
    Ok(delta)
}

fn unavailable_delta(
    project_id: &str,
    reason: &str,
    to_event_id: Option<String>,
    to_basis: Option<GraphBasis>,
    to_contract: Option<EvidenceContract>,
    wrong_project: bool,
) -> ObservationDelta {
    ObservationDelta {
        schema_version: crate::model::OBSERVATION_DELTA_SCHEMA.to_string(),
        project_id: project_id.to_string(),
        status: if wrong_project {
            "wrong-project".to_string()
        } else {
            "unavailable".to_string()
        },
        reason: reason.to_string(),
        from_event_id: None,
        to_event_id,
        relation: "observed-after".to_string(),
        from_basis: None,
        to_basis,
        from_contract: None,
        to_contract,
        contract_compatible: false,
        graph_relation: "unavailable".to_string(),
        basis_relations: ObservationBasisRelations {
            typescript_source: "unavailable".to_string(),
            module_resolution_exact: "unavailable".to_string(),
            module_resolution_effective: "unavailable".to_string(),
            entry_manifest_exact: "unavailable".to_string(),
            entry_manifest_effective: "unavailable".to_string(),
        },
        counts: ObservationDeltaCounts::default(),
        source_changes: Vec::new(),
        node_changes: Vec::new(),
        edge_changes: Vec::new(),
        flow_changes: Vec::new(),
        truncated: false,
        omissions: vec![reason.to_string()],
        limitations: vec![
            "Observation deltas require two adjacent observations with compatible persisted evidence contracts.".to_string(),
            "Unavailable evidence is not evidence that no structural change occurred.".to_string(),
        ],
    }
}
