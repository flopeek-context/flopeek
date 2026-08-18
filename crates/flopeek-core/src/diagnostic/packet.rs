//! Diagnostic packet composition.

#[allow(unused_imports)]
use super::*;

pub fn build_packet(
    root: &Path,
    context_id: &str,
    limits: DiagnosticLimits,
) -> Result<DiagnosticPacket, String> {
    let context = store::get_diagnostic_context(root, context_id)?;
    let graph = store::current_graph(root)?.ok_or_else(|| {
        "A current graph is required before a diagnostic packet can be built.".to_string()
    })?;
    let current_basis = GraphBasis {
        project_id: graph.project_id.clone(),
        graph_id: graph.graph_id.clone(),
        graph_version: graph.graph_version,
        source_revision: graph.source_revision.clone(),
        observation_id: graph.observation_id.clone(),
    };
    let mut focus_context_refs = Vec::new();
    let mut focus_nodes = Vec::new();
    let mut omissions = Vec::new();
    for (index, uri) in context.focus_context_refs.iter().enumerate() {
        if index >= limits.max_context_refs {
            omissions.push(format!(
                "focus Context Refs capped at {}",
                limits.max_context_refs
            ));
            break;
        }
        let resolved = store::resolve_context(root, uri)?;
        if let Some(node) = graph.nodes.iter().find(|node| node.id == resolved.node_id) {
            if !focus_nodes
                .iter()
                .any(|existing: &GraphNode| existing.id == node.id)
            {
                focus_nodes.push(node.clone());
            }
        } else {
            omissions.push(format!("focus node unavailable for {uri}"));
        }
        focus_context_refs.push(resolved);
    }
    let mut focus_flow_refs = Vec::new();
    let mut focus_flows = Vec::new();
    let mut related_tests = RelatedTestEvidence {
        schema_version: crate::model::RELATED_TEST_EVIDENCE_SCHEMA.to_string(),
        status: "complete".to_string(),
        records: Vec::new(),
        truncated: false,
        omissions: Vec::new(),
    };
    for (index, uri) in context.focus_flow_refs.iter().enumerate() {
        if index >= limits.max_context_refs {
            omissions.push(format!(
                "focus Flow Refs capped at {}",
                limits.max_context_refs
            ));
            break;
        }
        let resolved = store::resolve_flow(root, uri)?;
        if let Some(flow) = graph
            .flows
            .iter()
            .find(|flow| flow.flow_id == resolved.flow_id)
        {
            if !focus_flows
                .iter()
                .any(|existing: &ContextFlow| existing.flow_id == flow.flow_id)
            {
                focus_flows.push(flow.clone());
            }
            related_tests.records.extend(flow.related_tests.clone());
        } else {
            omissions.push(format!("focus flow unavailable for {uri}"));
        }
        focus_flow_refs.push(resolved);
    }
    let focus_node_ids = focus_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut context_reconciliation = Vec::new();
    for uri in context
        .focus_context_refs
        .iter()
        .take(limits.max_context_refs)
    {
        context_reconciliation.push(store::reconcile_context(root, uri)?);
    }
    related_tests.records.extend(
        graph
            .related_test_evidence
            .records
            .iter()
            .filter(|record| focus_node_ids.contains(record.target_node_id.as_str()))
            .cloned(),
    );
    related_tests.records.sort_by(|a, b| {
        a.test_path
            .cmp(&b.test_path)
            .then_with(|| a.test_node_id.cmp(&b.test_node_id))
            .then_with(|| a.target_node_id.cmp(&b.target_node_id))
            .then_with(|| a.relation.cmp(&b.relation))
    });
    related_tests.records.dedup();
    if related_tests.records.len() > crate::flow::MAX_RELATED_TEST_RECORDS {
        related_tests
            .records
            .truncate(crate::flow::MAX_RELATED_TEST_RECORDS);
        related_tests.truncated = true;
        related_tests.status = "truncated".to_string();
        related_tests.omissions.push(format!(
            "related-test records capped at {}",
            crate::flow::MAX_RELATED_TEST_RECORDS
        ));
    }
    let mut assertions = store::list_diagnostic_assertions(root, context_id)?;
    let assertion_total = assertions.len();
    if assertion_total > limits.max_assertions {
        assertions.truncate(limits.max_assertions);
        omissions.push(format!("assertions capped at {}", limits.max_assertions));
    }
    let historical = diagnose_history(root, context_id, limits.clone())?;
    if historical.truncated {
        omissions
            .push("historical diagnosis was truncated by one or more declared bounds".to_string());
    }
    omissions.extend(
        historical
            .omissions
            .iter()
            .take(8)
            .map(|omission| format!("historical: {omission}")),
    );
    let mut limitations = historical.limitations.clone();
    limitations.push("Assertions retain their declared evidence class and attribution; they are not parser facts.".to_string());
    let packet_truncated = historical.truncated
        || focus_context_refs.len() < context.focus_context_refs.len()
        || focus_flow_refs.len() < context.focus_flow_refs.len()
        || assertions.len() < assertion_total
        || related_tests.truncated
        || context_reconciliation.iter().any(|item| item.truncated);
    let mut packet = DiagnosticPacket {
        schema_version: DIAGNOSTIC_PACKET_SCHEMA.to_string(),
        current_graph_basis: current_basis.clone(),
        last_known_good_basis: context.last_known_good_basis.clone(),
        focus_context_refs,
        focus_flow_refs,
        focus_nodes,
        focus_flows,
        related_tests,
        context_reconciliation,
        assertions,
        historical,
        context,
        limitations,
        omissions,
        truncated: packet_truncated,
    };
    trim_packet(&mut packet, limits.max_packet_bytes)?;
    Ok(packet)
}

pub(super) fn trim_packet(packet: &mut DiagnosticPacket, max_bytes: usize) -> Result<(), String> {
    if max_bytes == 0 {
        return Err("max_packet_bytes must be greater than zero.".to_string());
    }
    let serialized =
        |packet: &DiagnosticPacket| serde_json::to_vec(packet).map_err(|error| error.to_string());
    if serialized(packet)?.len() <= max_bytes {
        return Ok(());
    }
    packet.truncated = true;
    packet
        .omissions
        .push("diagnostic packet exceeded max_packet_bytes".to_string());
    while serialized(packet)?.len() > max_bytes {
        if packet.historical.candidates.pop().is_some() {
            packet
                .omissions
                .push("historical candidates omitted by packet bound".to_string());
        } else if packet.assertions.pop().is_some() {
            packet
                .omissions
                .push("assertions omitted by packet bound".to_string());
        } else if packet.focus_nodes.pop().is_some() {
            packet
                .omissions
                .push("focus node cards omitted by packet bound".to_string());
        } else if packet.focus_flows.pop().is_some() {
            packet
                .omissions
                .push("focus flows omitted by packet bound".to_string());
        } else if packet.related_tests.records.pop().is_some() {
            packet
                .omissions
                .push("related-test evidence omitted by packet bound".to_string());
        } else if packet.context_reconciliation.pop().is_some() {
            packet
                .omissions
                .push("Context Ref reconciliation omitted by packet bound".to_string());
        } else if packet.focus_flow_refs.pop().is_some() {
            packet
                .omissions
                .push("focus Flow Refs omitted by packet bound".to_string());
        } else if packet.focus_context_refs.pop().is_some() {
            packet
                .omissions
                .push("focus Context Refs omitted by packet bound".to_string());
        } else {
            return Err(
                "diagnostic packet envelope exceeds max_packet_bytes even after bounded omissions."
                    .to_string(),
            );
        }
    }
    Ok(())
}
