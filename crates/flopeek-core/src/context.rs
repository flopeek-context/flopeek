//! Context Ref identity and freshness resolution.

use crate::model::{CONTEXT_REF_SCHEMA, ContextRef, GraphSnapshot};
use rusqlite::{Connection, OptionalExtension, params};

pub const MAX_CONTEXT_REFS: usize = 256;

pub fn uri(project_id: &str, graph_id: &str, node_id: &str) -> String {
    format!("fp://local/{project_id}/{graph_id}/{node_id}")
}

pub fn for_snapshot(
    connection: &Connection,
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
        };
        connection
            .execute(
                "INSERT OR IGNORE INTO context_refs(uri, project_id, graph_id, graph_version, node_id, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, strftime('%s','now'))",
                params![
                    reference.uri,
                    reference.project_id,
                    reference.graph_id,
                    reference.graph_version as i64,
                    reference.node_id,
                ],
            )
            .map_err(|error| format!("Unable to persist Context Ref: {error}"))?;
        refs.push(reference);
    }
    refs.sort_by(|left, right| left.uri.cmp(&right.uri));
    Ok(refs)
}

pub fn resolve(
    connection: &Connection,
    requested_uri: &str,
    project_id: &str,
) -> Result<ContextRef, String> {
    let Some((stored_project, graph_id, graph_version, node_id)) = connection
        .query_row(
            "SELECT project_id, graph_id, graph_version, node_id FROM context_refs WHERE uri = ?1",
            params![requested_uri],
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
        });
    }
    let current = connection
        .query_row(
            "SELECT graph_id, graph_version FROM graph_versions WHERE project_id = ?1 ORDER BY graph_version DESC LIMIT 1",
            params![project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| format!("Unable to read current graph for Context Ref: {error}"))?;
    let status = match current {
        Some((current_graph, current_version)) if current_graph == graph_id => {
            if current_version as u64 == graph_version as u64 {
                "current"
            } else {
                "stale"
            }
        }
        Some(_) => "stale",
        None => "unavailable",
    };
    Ok(ContextRef {
        schema_version: CONTEXT_REF_SCHEMA.to_string(),
        uri: requested_uri.to_string(),
        project_id: stored_project,
        graph_id,
        graph_version: graph_version as u64,
        node_id,
        status: status.to_string(),
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
            files: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
        };
    }
}
