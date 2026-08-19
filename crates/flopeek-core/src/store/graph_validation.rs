//! Validation of persisted graph rows before graph reuse.
//!
//! Exact source materialization and structural graph compatibility are
//! intentionally separate. An observation owns exact bytes and hashes;
//! graph rows own reusable structural evidence.

#[allow(unused_imports)]
use super::*;

/// Validate the current scan materialization, including exact source rows.
/// This remains the repair predicate for the checkout's canonical graph.
pub(super) fn exact_materialization_matches(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<bool, String> {
    if !structural_graph_compatible(transaction, graph_version, snapshot)? {
        return Ok(false);
    }

    let mut expected_source_rows = snapshot
        .files
        .iter()
        .map(|file| {
            let fact = facts
                .iter()
                .find(|fact| fact.path == file.path)
                .ok_or_else(|| format!("Missing facts for {}", file.path))?;
            Ok((
                file.path.clone(),
                file.language.clone(),
                file.bytes as i64,
                file.hash.clone(),
                serde_json::to_string(fact).map_err(|error| {
                    format!("Unable to encode facts for {}: {error}", file.path)
                })?,
            ))
        })
        .collect::<Result<Vec<(String, String, i64, String, String)>, String>>()?;
    expected_source_rows.sort_by(|left, right| left.0.cmp(&right.0));
    let stored_source_rows = transaction
        .prepare(
            "SELECT path, language, bytes, hash, facts_json FROM source_files
             WHERE graph_version = ?1 ORDER BY path",
        )
        .map_err(|error| format!("Unable to inspect reusable source rows: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| format!("Unable to query reusable source rows: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode reusable source rows: {error}"))?;
    Ok(stored_source_rows == expected_source_rows)
}

/// Validate only the structural evidence that defines a graph identity.
/// Exact bytes, raw hashes, source positions, and serialized facts are not
/// part of detached historical graph reuse.
pub(super) fn structural_graph_compatible(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
) -> Result<bool, String> {
    let stored_metadata = transaction
        .query_row(
            "SELECT graph_id, project_id, truncated, omissions_json,
                    graph_schema_version, graph_derivation_id,
                    node_fingerprint_contract
             FROM graph_versions WHERE graph_version = ?1",
            params![graph_version],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("Unable to inspect reusable graph metadata: {error}"))?;
    let expected_omissions = serde_json::to_string(&snapshot.omissions)
        .map_err(|error| format!("Unable to encode graph omissions: {error}"))?;
    if stored_metadata
        != Some((
            snapshot.graph_id.clone(),
            snapshot.project_id.clone(),
            i64::from(snapshot.truncated),
            expected_omissions,
            snapshot.schema_version.clone(),
            crate::graph::GRAPH_DERIVATION_ID.to_string(),
            crate::temporal::NODE_FINGERPRINT_CONTRACT.to_string(),
        ))
    {
        return Ok(false);
    }

    let mut expected_files = snapshot
        .files
        .iter()
        .map(|file| (file.path.clone(), file.language.clone()))
        .collect::<Vec<_>>();
    expected_files.sort();
    let stored_files = transaction
        .prepare(
            "SELECT path, language FROM source_files
             WHERE graph_version = ?1 ORDER BY path",
        )
        .map_err(|error| format!("Unable to inspect reusable file identities: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("Unable to query reusable file identities: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode reusable file identities: {error}"))?;
    if stored_files != expected_files {
        return Ok(false);
    }

    if !nodes_match(transaction, graph_version, snapshot)?
        || !edges_match(transaction, graph_version, snapshot)?
        || !flow_evidence_matches(transaction, graph_version, snapshot)?
    {
        return Ok(false);
    }

    let stored_facts = transaction
        .prepare(
            "SELECT facts_json FROM source_files
             WHERE graph_version = ?1 ORDER BY path",
        )
        .map_err(|error| format!("Unable to inspect reusable structural facts: {error}"))?
        .query_map(params![graph_version], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to query reusable structural facts: {error}"))?
        .map(|row| {
            let payload =
                row.map_err(|error| format!("Unable to read structural facts: {error}"))?;
            serde_json::from_str::<TypeScriptFacts>(&payload)
                .map_err(|error| format!("Unable to decode structural facts: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut persisted_resolution = crate::graph::resolution_evidence(&stored_facts, false);
    if snapshot.module_resolution.status == "truncated" {
        persisted_resolution.status = "truncated".to_string();
        persisted_resolution.truncated = true;
        persisted_resolution
            .omissions
            .push("module-resolution-basis-truncated".to_string());
    } else if snapshot.module_resolution.status == "unavailable"
        || snapshot.module_resolution.status.starts_with("legacy-")
    {
        persisted_resolution.status = "unavailable".to_string();
        persisted_resolution
            .omissions
            .push("module-resolution-basis-unavailable".to_string());
    }
    persisted_resolution.omissions.sort();
    persisted_resolution.omissions.dedup();
    Ok(persisted_resolution == snapshot.resolution_evidence)
}

fn nodes_match(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
) -> Result<bool, String> {
    let mut expected = snapshot
        .nodes
        .iter()
        .map(|node| {
            (
                node.id.clone(),
                node.kind.clone(),
                node.path.clone(),
                node.name.clone(),
                node.language.clone(),
                node.evidence_fingerprint.clone(),
            )
        })
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    let stored = transaction
        .prepare(
            "SELECT node_id, kind, path, name, language, evidence_fingerprint
             FROM graph_nodes WHERE graph_version = ?1 ORDER BY node_id",
        )
        .map_err(|error| format!("Unable to inspect reusable node rows: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| format!("Unable to query reusable node rows: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode reusable node rows: {error}"))?;
    Ok(stored == expected)
}

fn edges_match(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
) -> Result<bool, String> {
    let mut expected = snapshot
        .edges
        .iter()
        .map(|edge| {
            (
                edge.from.clone(),
                edge.to.clone(),
                edge.kind.clone(),
                edge.evidence.clone(),
            )
        })
        .collect::<Vec<_>>();
    expected.sort();
    let stored = transaction
        .prepare(
            "SELECT from_id, to_id, kind, evidence FROM graph_edges
             WHERE graph_version = ?1 ORDER BY from_id, to_id, kind, evidence",
        )
        .map_err(|error| format!("Unable to inspect reusable edge rows: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| format!("Unable to query reusable edge rows: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode reusable edge rows: {error}"))?;
    Ok(stored == expected)
}

fn flow_evidence_matches(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
) -> Result<bool, String> {
    let expected_entry = serde_json::to_string(&snapshot.entry_evidence)
        .map_err(|error| format!("Unable to encode entry evidence: {error}"))?;
    let expected_related = serde_json::to_string(&snapshot.related_test_evidence)
        .map_err(|error| format!("Unable to encode related-test evidence: {error}"))?;
    let expected_truncated = i64::from(
        snapshot.entry_evidence.truncated
            || snapshot.related_test_evidence.truncated
            || snapshot.flows.iter().any(|flow| flow.truncated),
    );
    let expected_omissions = serde_json::to_string(&snapshot.omissions)
        .map_err(|error| format!("Unable to encode flow omissions: {error}"))?;
    let stored = transaction
        .query_row(
            "SELECT entry_json, related_tests_json, truncated, omissions_json
             FROM graph_flow_evidence WHERE graph_version = ?1",
            params![graph_version],
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
        .map_err(|error| format!("Unable to inspect reusable flow evidence: {error}"))?;
    if stored
        != Some((
            expected_entry,
            expected_related,
            expected_truncated,
            expected_omissions,
        ))
    {
        return Ok(false);
    }

    let mut expected_flows = snapshot
        .flows
        .iter()
        .map(|flow| {
            Ok::<_, String>((
                flow.flow_id.clone(),
                flow.fingerprint.clone(),
                serde_json::to_string(flow)
                    .map_err(|error| format!("Unable to encode flow {}: {error}", flow.flow_id))?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    expected_flows.sort_by(|left, right| left.0.cmp(&right.0));
    let stored_flows = transaction
        .prepare(
            "SELECT flow_id, fingerprint, payload_json FROM graph_flows
             WHERE graph_version = ?1 ORDER BY flow_id",
        )
        .map_err(|error| format!("Unable to inspect reusable flow rows: {error}"))?
        .query_map(params![graph_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| format!("Unable to query reusable flow rows: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode reusable flow rows: {error}"))?;
    Ok(stored_flows == expected_flows)
}
