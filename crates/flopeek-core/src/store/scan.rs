//! Transactional graph scan persistence.

#[allow(unused_imports)]
use super::*;

pub fn persist_scan(
    root: &Path,
    mut snapshot: GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<ScanResult, String> {
    let identity = crate::identity::resolve(root)?;
    let mut connection = open(root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin SQLite transaction: {error}"))?;
    transaction
        .execute(
            "INSERT INTO product_metadata(key, value) VALUES('product', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![PRODUCT_IDENTITY],
        )
        .map_err(|error| format!("Unable to record product identity: {error}"))?;
    let previous_project = transaction
        .query_row(
            "SELECT value FROM product_metadata WHERE key = 'project_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read previous project identity: {error}"))?;
    let legacy_project = previous_project
        .as_ref()
        .filter(|previous| previous.as_str() != snapshot.project_id.as_str())
        .filter(|_| identity.repository_id.is_some())
        .cloned();
    if let Some(previous_project) = legacy_project.as_deref() {
        transaction
            .execute(
                "INSERT OR IGNORE INTO project_identity_aliases(
                    legacy_project_id, repository_project_id, checkout_id_hash, alias_kind, created_at
                 ) VALUES(?1, ?2, ?3, 'legacy-checkout-local', ?4)",
                params![
                    previous_project,
                    snapshot.project_id,
                    identity.checkout_id,
                    now_seconds()
                ],
            )
            .map_err(|error| format!("Unable to persist legacy project identity alias: {error}"))?;
    }
    if let (Some(repository_id), Some(manifest_path), Some(manifest_bytes), Some(manifest_hash)) = (
        identity.repository_id.as_deref(),
        identity.basis.manifest_path.as_deref(),
        identity.basis.manifest_bytes,
        identity.basis.manifest_hash.as_deref(),
    ) {
        transaction
            .execute(
                "INSERT OR IGNORE INTO repository_identities(
                    repository_id, manifest_path, manifest_bytes, manifest_hash, created_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    repository_id,
                    manifest_path,
                    manifest_bytes as i64,
                    manifest_hash,
                    now_seconds()
                ],
            )
            .map_err(|error| {
                format!("Unable to persist repository identity provenance: {error}")
            })?;
    }
    transaction
        .execute(
            "INSERT INTO product_metadata(key, value) VALUES('project_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![snapshot.project_id],
        )
        .map_err(|error| format!("Unable to record project identity: {error}"))?;

    let existing = transaction
        .query_row(
            "SELECT graph_version FROM graph_versions WHERE graph_id = ?1",
            params![snapshot.graph_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read graph identity: {error}"))?;
    let graph_version = if let Some(version) = existing {
        if !graph_validation::graph_rows_match(&transaction, version, &snapshot, facts)? {
            transaction
                .execute(
                    "DELETE FROM graph_flows WHERE graph_version = ?1",
                    rusqlite::params![version],
                )
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM graph_flow_evidence WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM graph_edges WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM graph_nodes WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .and_then(|_| {
                    transaction.execute(
                        "DELETE FROM source_files WHERE graph_version = ?1",
                        rusqlite::params![version],
                    )
                })
                .map_err(|error| format!("Unable to recover corrupted graph rows: {error}"))?;
            persist_graph_rows(&transaction, version, &snapshot, facts)?;
        }
        transaction
            .execute(
                "UPDATE graph_versions
                 SET graph_id = ?1,
                     project_id = ?2,
                     truncated = ?3,
                     omissions_json = ?4,
                     graph_schema_version = ?5,
                     graph_derivation_id = ?6,
                     node_fingerprint_contract = ?7
                 WHERE graph_version = ?8",
                params![
                    snapshot.graph_id,
                    snapshot.project_id,
                    i64::from(snapshot.truncated),
                    serde_json::to_string(&snapshot.omissions)
                        .map_err(|error| format!("Unable to encode graph omissions: {error}"))?,
                    crate::model::GRAPH_SCHEMA,
                    crate::graph::GRAPH_DERIVATION_ID,
                    crate::temporal::NODE_FINGERPRINT_CONTRACT,
                    version
                ],
            )
            .map_err(|error| {
                format!("Unable to record reusable graph evidence contract: {error}")
            })?;
        version as u64
    } else {
        let version = transaction
            .query_row(
                "SELECT COALESCE(MAX(graph_version), 0) + 1 FROM graph_versions",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("Unable to allocate graph version: {error}"))?;
        transaction
            .execute(
                "INSERT INTO graph_versions(
                    graph_version, graph_id, project_id, source_revision, created_at,
                    truncated, omissions_json, graph_schema_version, graph_derivation_id,
                    node_fingerprint_contract
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    version,
                    snapshot.graph_id,
                    snapshot.project_id,
                    snapshot.source_revision,
                    now_seconds(),
                    i64::from(snapshot.truncated),
                    serde_json::to_string(&snapshot.omissions)
                        .map_err(|error| format!("Unable to encode graph omissions: {error}"))?,
                    crate::model::GRAPH_SCHEMA,
                    crate::graph::GRAPH_DERIVATION_ID,
                    crate::temporal::NODE_FINGERPRINT_CONTRACT,
                ],
            )
            .map_err(|error| format!("Unable to persist graph version: {error}"))?;
        persist_graph_rows(&transaction, version, &snapshot, facts)?;
        version as u64
    };
    snapshot.graph_version = graph_version;
    transaction
        .execute(
            "DELETE FROM graph_flows WHERE graph_version = ?1",
            params![graph_version as i64],
        )
        .and_then(|_| {
            transaction.execute(
                "DELETE FROM graph_flow_evidence WHERE graph_version = ?1",
                params![graph_version as i64],
            )
        })
        .map_err(|error| format!("Unable to refresh flow evidence rows: {error}"))?;
    persist_flow_rows(&transaction, graph_version as i64, &snapshot)?;
    let dirty = snapshot.source_revision.ends_with("+dirty");
    let git_revision = snapshot
        .source_revision
        .strip_suffix("+dirty")
        .unwrap_or(&snapshot.source_revision)
        .to_string();
    let observation = observation_id(
        &snapshot.project_id,
        &snapshot.source_revision,
        &snapshot.source_fingerprint,
        &snapshot.module_resolution.exact_fingerprint,
        &snapshot.entry_evidence.exact_fingerprint,
        &snapshot.graph_id,
    );
    let source_manifest_json = serde_json::to_string(&snapshot.files)
        .map_err(|error| format!("Unable to encode graph observation manifest: {error}"))?;
    let module_resolution_manifest_json =
        serde_json::to_string(&snapshot.module_resolution.config_files)
            .map_err(|error| format!("Unable to encode module resolution manifest: {error}"))?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO graph_observations(
                observation_id, project_id, graph_version, git_revision,
                source_fingerprint, source_manifest_json, dirty,
                module_resolution_status, module_resolution_fingerprint,
                module_resolution_effective_fingerprint, module_resolution_manifest_json,
                entry_manifest_status, entry_manifest_fingerprint,
                entry_effective_fingerprint, entry_manifest_json, observed_at,
                repository_identity_status, repository_identity_id,
                repository_manifest_path, repository_manifest_bytes,
                repository_manifest_hash, checkout_id_hash
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                      ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                observation,
                snapshot.project_id,
                graph_version as i64,
                git_revision,
                snapshot.source_fingerprint,
                source_manifest_json,
                i64::from(dirty),
                snapshot.module_resolution.status,
                snapshot.module_resolution.exact_fingerprint,
                snapshot.module_resolution.effective_fingerprint,
                module_resolution_manifest_json,
                snapshot.entry_evidence.status,
                snapshot.entry_evidence.exact_fingerprint,
                snapshot.entry_evidence.effective_fingerprint,
                serde_json::to_string(&snapshot.entry_evidence.manifest)
                    .map_err(|error| format!("Unable to encode entry manifest: {error}"))?,
                now_seconds(),
                snapshot.identity_basis.status,
                snapshot.identity_basis.repository_id,
                snapshot.identity_basis.manifest_path,
                snapshot
                    .identity_basis
                    .manifest_bytes
                    .map(|value| value as i64),
                snapshot.identity_basis.manifest_hash,
                identity.checkout_id
            ],
        )
        .map_err(|error| format!("Unable to persist graph observation: {error}"))?;
    snapshot.observation_id = observation;
    let previous_state = transaction
        .query_row(
            "SELECT current_observation_id, current_event_id
             FROM project_state WHERE project_id = ?1",
            params![snapshot.project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|error| format!("Unable to read previous observation event: {error}"))?;
    let event_id = if previous_state
        .as_ref()
        .is_some_and(|(previous_observation, event)| {
            previous_observation == &snapshot.observation_id && event.is_some()
        }) {
        previous_state
            .as_ref()
            .and_then(|(_, event)| event.clone())
            .expect("same observation has an event")
    } else {
        let predecessor = previous_state
            .as_ref()
            .and_then(|(_, event)| event.as_deref());
        let event_id = crate::temporal::observation_event_id(
            &snapshot.project_id,
            predecessor,
            &snapshot.observation_id,
        );
        transaction
            .execute(
                "INSERT OR IGNORE INTO observation_events(
                    event_id, project_id, observation_id, predecessor_event_id, observed_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    snapshot.project_id,
                    snapshot.observation_id,
                    predecessor,
                    now_seconds()
                ],
            )
            .map_err(|error| format!("Unable to persist observation continuity event: {error}"))?;
        event_id
    };
    transaction
        .execute(
            "INSERT INTO project_state(project_id, current_observation_id, current_event_id)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET
                 current_observation_id = excluded.current_observation_id,
                 current_event_id = excluded.current_event_id",
            params![snapshot.project_id, snapshot.observation_id, event_id],
        )
        .map_err(|error| format!("Unable to update current project observation: {error}"))?;
    if let Some(legacy_project) = legacy_project {
        transaction
            .execute(
                "UPDATE project_state
                 SET current_observation_id = ?1, current_event_id = ?2
                 WHERE project_id = ?3",
                params![snapshot.observation_id, event_id, legacy_project],
            )
            .map_err(|error| {
                format!("Unable to bridge legacy checkout-local project state: {error}")
            })?;
    }
    let refs = context::for_snapshot(&transaction, &snapshot)?;
    let mut flow_refs = flow_ref::for_snapshot(&transaction, &snapshot)?;
    let flow_refs_truncated = flow_refs.len() > crate::flow::MAX_FLOW_REFS;
    if flow_refs_truncated {
        flow_refs.truncate(crate::flow::MAX_FLOW_REFS);
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit SQLite graph/context transaction: {error}"))?;
    Ok(ScanResult {
        schema_version: STORE_SCHEMA.to_string(),
        product: PRODUCT_IDENTITY.to_string(),
        project_id: snapshot.project_id.clone(),
        graph: snapshot,
        identity_basis: identity.basis,
        context_refs: refs,
        flow_refs,
        limitations: {
            let mut limitations = vec![
            "Evidence is static TypeScript/TSX syntax and Git identity; runtime behavior is unavailable.".to_string(),
            "Dynamic dispatch, reflection, generated files and package-manager execution are unsupported.".to_string(),
            "Historical change ranking is not inferred from this scan and must remain an explicit candidate.".to_string(),
            ];
            if flow_refs_truncated {
                limitations.push(format!(
                    "Flow Refs capped at {}.",
                    crate::flow::MAX_FLOW_REFS
                ));
            }
            limitations
        },
    })
}

pub(super) fn persist_flow_rows(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO graph_flow_evidence(graph_version, entry_json, related_tests_json, truncated, omissions_json)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                graph_version,
                serde_json::to_string(&snapshot.entry_evidence)
                    .map_err(|error| format!("Unable to encode entry evidence: {error}"))?,
                serde_json::to_string(&snapshot.related_test_evidence)
                    .map_err(|error| format!("Unable to encode related-test evidence: {error}"))?,
                i64::from(snapshot.entry_evidence.truncated || snapshot.related_test_evidence.truncated || snapshot.flows.iter().any(|flow| flow.truncated)),
                serde_json::to_string(&snapshot.omissions)
                    .map_err(|error| format!("Unable to encode flow omissions: {error}"))?,
            ],
        )
        .map_err(|error| format!("Unable to persist flow evidence: {error}"))?;
    for flow in &snapshot.flows {
        transaction
            .execute(
                "INSERT INTO graph_flows(graph_version, flow_id, fingerprint, payload_json)
                 VALUES(?1, ?2, ?3, ?4)",
                params![
                    graph_version,
                    flow.flow_id,
                    flow.fingerprint,
                    serde_json::to_string(flow).map_err(|error| format!(
                        "Unable to encode flow {}: {error}",
                        flow.flow_id
                    ))?,
                ],
            )
            .map_err(|error| format!("Unable to persist flow {}: {error}", flow.flow_id))?;
    }
    Ok(())
}

pub(super) fn persist_graph_rows(
    transaction: &Transaction<'_>,
    graph_version: i64,
    snapshot: &GraphSnapshot,
    facts: &[TypeScriptFacts],
) -> Result<(), String> {
    for file in &snapshot.files {
        let fact = facts
            .iter()
            .find(|fact| fact.path == file.path)
            .ok_or_else(|| format!("Missing facts for {}", file.path))?;
        let facts_json = serde_json::to_string(fact)
            .map_err(|error| format!("Unable to encode facts for {}: {error}", file.path))?;
        transaction
            .execute(
                "INSERT INTO source_files(graph_version, path, language, bytes, hash, facts_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    graph_version,
                    file.path,
                    file.language,
                    file.bytes as i64,
                    file.hash,
                    facts_json
                ],
            )
            .map_err(|error| {
                format!(
                    "Unable to persist source evidence for {}: {error}",
                    file.path
                )
            })?;
    }
    for node in &snapshot.nodes {
        transaction
            .execute(
                "INSERT INTO graph_nodes(graph_version, node_id, kind, path, name, language, evidence_fingerprint)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    graph_version,
                    node.id,
                    node.kind,
                    node.path,
                    node.name,
                    node.language,
                    node.evidence_fingerprint
                ],
            )
            .map_err(|error| format!("Unable to persist graph node {}: {error}", node.id))?;
    }
    for edge in &snapshot.edges {
        transaction
            .execute(
                "INSERT INTO graph_edges(graph_version, from_id, to_id, kind, evidence)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![graph_version, edge.from, edge.to, edge.kind, edge.evidence],
            )
            .map_err(|error| format!("Unable to persist graph edge: {error}"))?;
    }
    Ok(())
}
