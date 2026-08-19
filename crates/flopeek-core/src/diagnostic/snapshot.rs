//! Historical graph snapshots.

#[allow(unused_imports)]
use super::*;

pub(super) fn load_or_build_historical_snapshot(
    root: &Path,
    revision: &str,
    limits: &DiagnosticLimits,
    cache: &mut BTreeMap<String, HistoricalSnapshot>,
) -> Result<HistoricalSnapshot, String> {
    let cache_key = format!(
        "{revision}\0{HISTORY_DERIVATION_ID}\0{PARSER_IDENTITY}\0{}\0{}",
        limits.max_paths, limits.max_snapshot_bytes
    );
    if let Some(snapshot) = cache.get(&cache_key) {
        return Ok(snapshot.clone());
    }
    if let Some(snapshot) = crate::history_store::load_with_key(
        root,
        revision,
        HISTORY_DERIVATION_ID,
        PARSER_IDENTITY,
        limits.max_paths,
        limits.max_snapshot_bytes,
    )? {
        cache.insert(cache_key, snapshot.clone());
        return Ok(snapshot);
    }
    let snapshot = build_historical_snapshot(root, revision, limits)?;
    crate::history_store::save_with_key(
        root,
        &snapshot,
        HISTORY_DERIVATION_ID,
        PARSER_IDENTITY,
        limits.max_paths,
        limits.max_snapshot_bytes,
    )?;
    cache.insert(cache_key, snapshot.clone());
    Ok(snapshot)
}

pub(super) fn build_historical_snapshot(
    root: &Path,
    revision: &str,
    limits: &DiagnosticLimits,
) -> Result<HistoricalSnapshot, String> {
    let paths = git_tree_paths(root, revision)?;
    let temporary = std::env::temp_dir().join(format!(
        "flopeek-history-{}-{}",
        &revision[..revision.len().min(16)],
        std::process::id()
    ));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)
            .map_err(|error| format!("Unable to replace historical snapshot workspace: {error}"))?;
    }
    fs::create_dir_all(&temporary)
        .map_err(|error| format!("Unable to create historical snapshot workspace: {error}"))?;
    let mut total_bytes = 0_usize;
    let mut truncated = false;
    let mut omissions = Vec::new();
    let mut included = 0_usize;
    for path in paths.iter().filter(|path| {
        is_typescript_path(path)
            || *path == "package.json"
            || *path == crate::identity::MANIFEST_PATH
    }) {
        if included >= limits.max_paths {
            truncated = true;
            omissions.push(format!(
                "historical snapshot paths capped at {}",
                limits.max_paths
            ));
            break;
        }
        if !safe_relative_path(path) {
            truncated = true;
            omissions.push(format!("unsafe historical path omitted: {path}"));
            continue;
        }
        let bytes = git_show_bytes(root, revision, path)?;
        if total_bytes.saturating_add(bytes.len()) > limits.max_snapshot_bytes {
            truncated = true;
            omissions.push(format!(
                "historical snapshot bytes capped at {}",
                limits.max_snapshot_bytes
            ));
            break;
        }
        let destination = temporary.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Unable to create historical source directory: {error}")
            })?;
        }
        fs::write(destination, &bytes).map_err(|error| {
            format!("Unable to materialize historical source evidence: {error}")
        })?;
        total_bytes += bytes.len();
        included += 1;
    }
    let config_paths = match historical_config_paths(root, revision, &paths) {
        Ok(paths) => paths,
        Err(error) => {
            if error.contains("capped") {
                truncated = true;
            }
            omissions.push(format!(
                "historical module configuration unavailable: {error}"
            ));
            if paths.iter().any(|path| path == "tsconfig.json") {
                vec!["tsconfig.json".to_string()]
            } else {
                Vec::new()
            }
        }
    };
    let mut config_bytes = 0_usize;
    for path in config_paths {
        if !safe_relative_path(&path) {
            truncated = true;
            omissions.push(format!("unsafe historical config path omitted: {path}"));
            continue;
        }
        let bytes = git_show_bytes(root, revision, &path)?;
        if config_bytes.saturating_add(bytes.len()) > MAX_CONFIG_BYTES {
            truncated = true;
            omissions.push(format!(
                "historical module configuration bytes capped at {MAX_CONFIG_BYTES}"
            ));
            break;
        }
        if total_bytes.saturating_add(bytes.len()) > limits.max_snapshot_bytes {
            truncated = true;
            omissions.push(format!(
                "historical snapshot bytes capped at {}",
                limits.max_snapshot_bytes
            ));
            break;
        }
        let destination = temporary.join(&path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Unable to create historical config directory: {error}")
            })?;
        }
        fs::write(destination, &bytes).map_err(|error| {
            format!("Unable to materialize historical config evidence: {error}")
        })?;
        total_bytes += bytes.len();
        config_bytes += bytes.len();
    }
    let historical_identity = crate::identity::resolve(&temporary)
        .map_err(|error| format!("historical-repository-identity-invalid: {error}"));
    let built = crate::graph::build(&temporary);
    let _ = fs::remove_dir_all(&temporary);
    let (mut graph_snapshot, _) = built?;
    let historical_identity = historical_identity?;
    graph_snapshot.project_id = crate::graph::project_id(root);
    for flow in &mut graph_snapshot.flows {
        flow.flow_id = crate::flow::flow_id(
            &graph_snapshot.project_id,
            &flow.entry_kind,
            &flow.entry_key,
        );
    }
    graph_snapshot.source_revision = revision.to_string();
    graph_snapshot.observation_id.clear();
    graph_snapshot.graph_version = 0;
    graph_snapshot.truncated |= truncated;
    graph_snapshot.omissions.extend(omissions);
    Ok(HistoricalSnapshot {
        schema_version: HISTORICAL_SNAPSHOT_SCHEMA.to_string(),
        project_id: graph_snapshot.project_id,
        source_revision: revision.to_string(),
        repository_identity_id: historical_identity.repository_id,
        evidence_contract: Some(crate::model::EvidenceContract {
            graph_schema_version: crate::model::GRAPH_SCHEMA.to_string(),
            graph_derivation_id: crate::graph::GRAPH_DERIVATION_ID.to_string(),
            node_fingerprint_contract: crate::temporal::NODE_FINGERPRINT_CONTRACT.to_string(),
        }),
        files: graph_snapshot.files,
        nodes: graph_snapshot.nodes,
        edges: graph_snapshot.edges,
        resolution_evidence: graph_snapshot.resolution_evidence,
        module_resolution: graph_snapshot.module_resolution,
        entry_evidence: graph_snapshot.entry_evidence,
        related_test_evidence: graph_snapshot.related_test_evidence,
        flows: graph_snapshot.flows,
        truncated: graph_snapshot.truncated,
        omissions: graph_snapshot.omissions,
    })
}
