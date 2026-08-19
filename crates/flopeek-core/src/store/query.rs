//! Graph and flow read queries.

#[allow(unused_imports)]
use super::*;

pub fn status(root: &Path) -> Result<StoreStatus, String> {
    let connection = open(root)?;
    let project_id = connection
        .query_row(
            "SELECT value FROM product_metadata WHERE key = 'project_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read SQLite project identity: {error}"))?
        .unwrap_or_else(|| crate::graph::project_id(root));
    let current = connection
        .query_row(
            "SELECT graph.graph_id, graph.graph_version, observation.observation_id
             FROM project_state state
             JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            )),
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph: {error}"))?;
    let graph_count = connection
        .query_row(
            "SELECT COUNT(*) FROM graph_versions WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Unable to count graphs: {error}"))?;
    let (node_count, edge_count) = if let Some((_, version, _)) = current {
        (
            connection
                .query_row(
                    "SELECT COUNT(*) FROM graph_nodes WHERE graph_version = ?1",
                    params![version],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("Unable to count nodes: {error}"))?,
            connection
                .query_row(
                    "SELECT COUNT(*) FROM graph_edges WHERE graph_version = ?1",
                    params![version],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("Unable to count edges: {error}"))?,
        )
    } else {
        (0, 0)
    };
    Ok(StoreStatus {
        schema_version: STORE_SCHEMA.to_string(),
        path: database_path(root).to_string_lossy().into_owned(),
        project_id,
        current_graph_id: current.as_ref().map(|value| value.0.clone()),
        current_graph_version: current.as_ref().map(|value| value.1 as u64),
        graph_count: graph_count as u64,
        node_count: node_count as u64,
        edge_count: edge_count as u64,
        current_observation_id: current.map(|value| value.2),
    })
}

pub fn resolve_context(root: &Path, uri: &str) -> Result<ContextRef, String> {
    let connection = open(root)?;
    context::resolve(&connection, uri, &crate::graph::project_id(root))
}

pub fn resolve_flow(root: &Path, uri: &str) -> Result<crate::model::FlowRef, String> {
    let connection = open(root)?;
    flow_ref::resolve(&connection, uri, &crate::graph::project_id(root))
}

pub fn list_flows(root: &Path) -> Result<Vec<crate::model::ContextFlow>, String> {
    let graph = current_graph(root)?.ok_or_else(|| "No graph has been scanned yet.".to_string())?;
    Ok(graph.flows)
}

pub fn get_flow(root: &Path, flow_id: &str) -> Result<crate::model::ContextFlow, String> {
    let graph = current_graph(root)?.ok_or_else(|| "No graph has been scanned yet.".to_string())?;
    graph
        .flows
        .into_iter()
        .find(|flow| flow.flow_id == flow_id)
        .ok_or_else(|| "Flow is unavailable in the current graph.".to_string())
}

pub fn related_tests(
    root: &Path,
    node_id: Option<&str>,
    flow_id: Option<&str>,
) -> Result<crate::model::RelatedTestEvidence, String> {
    if node_id.is_some() == flow_id.is_some() {
        return Err("getRelatedTests requires exactly one of nodeId or flowId.".to_string());
    }
    let graph = current_graph(root)?.ok_or_else(|| "No graph has been scanned yet.".to_string())?;
    let ids = if let Some(node_id) = node_id {
        std::iter::once(node_id.to_string()).collect::<std::collections::BTreeSet<_>>()
    } else {
        let flow = graph
            .flows
            .iter()
            .find(|flow| Some(flow.flow_id.as_str()) == flow_id)
            .ok_or_else(|| "Flow is unavailable in the current graph.".to_string())?;
        flow.steps
            .iter()
            .map(|step| step.node_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let mut evidence = graph.related_test_evidence;
    evidence
        .records
        .retain(|record| ids.contains(&record.target_node_id));
    Ok(evidence)
}

pub fn current_graph(root: &Path) -> Result<Option<GraphSnapshot>, String> {
    let connection = open(root)?;
    let project_id = crate::graph::project_id(root);
    let Some((
        graph_id,
        graph_version,
        source_revision,
        source_fingerprint,
        source_manifest_json,
        observation_id,
        module_resolution_status,
        module_resolution_fingerprint,
        module_resolution_effective_fingerprint,
        module_resolution_manifest_json,
        entry_manifest_status,
        entry_manifest_fingerprint,
        entry_effective_fingerprint,
        entry_manifest_json,
        truncated,
        omissions_json,
    )) = connection
        .query_row(
            "SELECT graph.graph_id, graph.graph_version,
                    CASE WHEN observation.dirty = 1 THEN observation.git_revision || '+dirty'
                         ELSE observation.git_revision END,
                     observation.source_fingerprint, observation.source_manifest_json,
                     observation.observation_id,
                     observation.module_resolution_status,
                     observation.module_resolution_fingerprint,
                     observation.module_resolution_effective_fingerprint,
                     observation.module_resolution_manifest_json,
                     observation.entry_manifest_status,
                     observation.entry_manifest_fingerprint,
                     observation.entry_effective_fingerprint,
                     observation.entry_manifest_json,
                     graph.truncated, graph.omissions_json
             FROM project_state state
             JOIN graph_observations observation ON observation.observation_id = state.current_observation_id
             JOIN graph_versions graph ON graph.graph_version = observation.graph_version
             WHERE state.project_id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
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
                    row.get::<_, i64>(14)?,
                    row.get::<_, String>(15)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph: {error}"))?
    else {
        return Ok(None);
    };
    let mut facts = Vec::new();
    let mut legacy_facts = false;
    let fact_rows = connection
        .prepare(
            "SELECT path, facts_json FROM source_files
             WHERE graph_version = ?1 ORDER BY path",
        )
        .map_err(|error| format!("Unable to prepare source query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("Unable to query source evidence: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode source evidence: {error}"))?;
    for (path, facts_json) in fact_rows {
        let fact = serde_json::from_str::<TypeScriptFacts>(&facts_json)
            .map_err(|error| format!("Unable to decode TypeScript facts: {error}"))?;
        if fact.path != path {
            return Err("Persisted TypeScript facts path conflicts with source row.".to_string());
        }
        legacy_facts |= fact.schema_version != crate::model::TYPESCRIPT_FACTS_SCHEMA
            || fact.parser != crate::typescript::PARSER_IDENTITY;
        facts.push(fact);
    }
    let mut files = crate::store::observation::decode_source_manifest(&source_manifest_json)?;
    let mut nodes = connection
        .prepare(
            "SELECT node_id, kind, path, name, language, evidence_fingerprint FROM graph_nodes
             WHERE graph_version = ?1 ORDER BY node_id",
        )
        .map_err(|error| format!("Unable to prepare node query: {error}"))?
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
        .map_err(|error| format!("Unable to query graph nodes: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode graph nodes: {error}"))?;
    let mut edges = connection
        .prepare(
            "SELECT from_id, to_id, kind, evidence FROM graph_edges
             WHERE graph_version = ?1 ORDER BY from_id, to_id, kind, evidence",
        )
        .map_err(|error| format!("Unable to prepare edge query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok(GraphEdge {
                from: row.get(0)?,
                to: row.get(1)?,
                kind: row.get(2)?,
                evidence: row.get(3)?,
            })
        })
        .map_err(|error| format!("Unable to query graph edges: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode graph edges: {error}"))?;
    files.shrink_to_fit();
    nodes.shrink_to_fit();
    edges.shrink_to_fit();
    let omissions = serde_json::from_str::<Vec<String>>(&omissions_json)
        .map_err(|error| format!("Unable to decode graph omissions: {error}"))?;
    let mut resolution_evidence = crate::graph::resolution_evidence(&facts, legacy_facts);
    if module_resolution_status == "truncated" {
        resolution_evidence.status = "truncated".to_string();
        resolution_evidence.truncated = true;
        resolution_evidence
            .omissions
            .push("module-resolution-basis-truncated".to_string());
    } else if module_resolution_status == "unavailable"
        || module_resolution_status.starts_with("legacy-")
    {
        resolution_evidence.status = "unavailable".to_string();
        resolution_evidence
            .omissions
            .push("module-resolution-basis-unavailable".to_string());
    }
    let module_resolution_manifest: Vec<crate::model::ModuleResolutionConfigFile> =
        serde_json::from_str(&module_resolution_manifest_json)
            .map_err(|error| format!("Unable to decode module resolution manifest: {error}"))?;
    let root_config = if module_resolution_manifest.is_empty() {
        None
    } else {
        Some("tsconfig.json".to_string())
    };
    let limitations = if module_resolution_status == "legacy-config-basis-unavailable" {
        vec!["legacy-config-basis-unavailable".to_string()]
    } else if module_resolution_status == "complete" {
        vec![
            "nested-tsconfig-project-selection-unsupported".to_string(),
            "package-and-project-reference-resolution-unsupported".to_string(),
        ]
    } else {
        omissions
            .iter()
            .filter(|omission| {
                omission.starts_with("tsconfig-") || omission.starts_with("module-resolution-")
            })
            .cloned()
            .collect()
    };
    let module_resolution_omissions =
        if module_resolution_status == "legacy-config-basis-unavailable" {
            vec!["legacy-config-basis-unavailable".to_string()]
        } else {
            omissions
                .iter()
                .filter(|omission| {
                    omission.starts_with("tsconfig-") || omission.starts_with("module-resolution-")
                })
                .cloned()
                .collect()
        };
    let module_resolution = crate::model::ModuleResolutionBasis {
        schema_version: crate::module_resolution::MODULE_RESOLUTION_SCHEMA.to_string(),
        status: module_resolution_status,
        root_config,
        config_files: module_resolution_manifest,
        exact_fingerprint: module_resolution_fingerprint,
        effective_fingerprint: module_resolution_effective_fingerprint,
        limitations,
        omissions: module_resolution_omissions,
    };
    let flow_evidence = connection
        .query_row(
            "SELECT entry_json, related_tests_json FROM graph_flow_evidence WHERE graph_version = ?1",
            params![graph_version],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("Unable to read flow evidence: {error}"))?;
    let (entry_evidence, related_test_evidence) = if let Some((entry_json, related_json)) =
        flow_evidence
    {
        (
            serde_json::from_str(&entry_json)
                .map_err(|error| format!("Unable to decode entry evidence: {error}"))?,
            serde_json::from_str(&related_json)
                .map_err(|error| format!("Unable to decode related-test evidence: {error}"))?,
        )
    } else {
        let manifest = if entry_manifest_json.is_empty() {
            None
        } else {
            serde_json::from_str(&entry_manifest_json).ok()
        };
        (
            crate::model::EntryEvidence {
                schema_version: crate::model::ENTRY_EVIDENCE_SCHEMA.to_string(),
                status: if entry_manifest_status.is_empty()
                    || entry_manifest_status.starts_with("legacy-")
                {
                    "unavailable"
                } else {
                    &entry_manifest_status
                }
                .to_string(),
                manifest,
                exact_fingerprint: entry_manifest_fingerprint,
                effective_fingerprint: entry_effective_fingerprint,
                records: Vec::new(),
                truncated: false,
                omissions: vec!["legacy-entry-basis-unavailable".to_string()],
                limitations: vec!["legacy-v5-graph-requires-rescan-for-entry-evidence".to_string()],
            },
            crate::model::RelatedTestEvidence::default(),
        )
    };
    let flows = connection
        .prepare("SELECT flow_id, fingerprint, payload_json FROM graph_flows WHERE graph_version = ?1 ORDER BY flow_id")
        .map_err(|error| format!("Unable to prepare flow query: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| format!("Unable to query flows: {error}"))?
        .map(|row| {
            let (flow_id, fingerprint, payload) =
                row.map_err(|error| format!("Unable to decode flow row: {error}"))?;
            let flow: crate::model::ContextFlow = serde_json::from_str(&payload)
                .map_err(|error| format!("Unable to decode flow payload: {error}"))?;
            if flow.flow_id != flow_id || flow.fingerprint != fingerprint {
                return Err("Persisted flow metadata conflicts with its payload.".to_string());
            }
            Ok(flow)
        })
        .collect::<Result<Vec<crate::model::ContextFlow>, String>>()?;
    Ok(Some(GraphSnapshot {
        schema_version: crate::model::GRAPH_SCHEMA.to_string(),
        product: PRODUCT_IDENTITY.to_string(),
        project_id,
        graph_id,
        graph_version: graph_version as u64,
        source_revision,
        source_fingerprint,
        observation_id,
        files,
        nodes,
        edges,
        resolution_evidence,
        module_resolution,
        entry_evidence,
        related_test_evidence,
        flows,
        truncated: truncated != 0,
        omissions,
    }))
}

pub fn node_details(root: &Path, node_id: &str) -> Result<serde_json::Value, String> {
    let graph = current_graph(root)?.ok_or_else(|| "No graph has been scanned yet.".to_string())?;
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| "Node is unavailable in the current graph.".to_string())?;
    let outgoing = graph
        .edges
        .iter()
        .filter(|edge| edge.from == node_id)
        .cloned()
        .collect::<Vec<_>>();
    let incoming = graph
        .edges
        .iter()
        .filter(|edge| edge.to == node_id)
        .cloned()
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "schemaVersion": crate::model::GRAPH_SCHEMA,
        "projectId": graph.project_id,
        "graphId": graph.graph_id,
        "graphVersion": graph.graph_version,
        "node": node,
        "outgoing": outgoing,
        "incoming": incoming,
        "evidenceClass": "static",
        "limitations": ["Node details describe source structure only; runtime behavior and causality are unavailable."],
    }))
}
