//! Validation of persisted graph rows before structural graph reuse.

#[allow(unused_imports)]
use super::*;

pub(super) fn graph_rows_match(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<bool, String> {
    let stored_metadata = transaction
        .query_row(
            "SELECT graph_id, project_id, truncated, omissions_json
             FROM graph_versions WHERE graph_version = ?1",
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
        .map_err(|error| format!("Unable to inspect reusable graph metadata: {error}"))?;
    let expected_omissions = serde_json::to_string(&snapshot.omissions)
        .map_err(|error| format!("Unable to encode graph omissions: {error}"))?;
    if stored_metadata
        != Some((
            snapshot.graph_id.clone(),
            snapshot.project_id.clone(),
            i64::from(snapshot.truncated),
            expected_omissions,
        ))
    {
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
    if stored_source_rows != expected_source_rows {
        return Ok(false);
    }

    let mut expected_nodes = snapshot
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
    expected_nodes.sort_by(|left, right| left.0.cmp(&right.0));
    let stored_nodes = transaction
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
    if stored_nodes != expected_nodes {
        return Ok(false);
    }

    let mut expected_edges = snapshot
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
    expected_edges.sort();
    let stored_edges = transaction
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
    Ok(stored_edges == expected_edges)
}
