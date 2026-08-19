//! Historical diagnosis orchestration.

#[allow(unused_imports)]
use super::*;

pub fn diagnose_history(
    root: &Path,
    context_id: &str,
    limits: DiagnosticLimits,
) -> Result<HistoricalDiagnosis, String> {
    let context = store::get_diagnostic_context(root, context_id)?;
    let graph = store::current_graph(root)?.ok_or_else(|| {
        "A current graph is required before historical diagnosis can run.".to_string()
    })?;
    let current_basis = GraphBasis {
        project_id: graph.project_id.clone(),
        graph_id: graph.graph_id.clone(),
        graph_version: graph.graph_version,
        source_revision: graph.source_revision.clone(),
        observation_id: graph.observation_id.clone(),
    };
    let mut limitations = vec![
        "Historical candidates are deterministic path/topology relevance signals, not runtime causes or root-cause findings.".to_string(),
        "Runtime execution, dynamic dispatch, reflection, generated code and business intent remain unavailable.".to_string(),
    ];
    let protocol_candidate = store::confirmed_protocol_candidate(root, context_id)?;
    let protocol_state = Some(store::get_last_known_good_protocol(root, context_id)?);
    let protocol_applicability =
        protocol_candidate
            .as_ref()
            .map(|_| crate::model::LastKnownGoodApplicability {
                status: "applicable".to_string(),
                limitations: Vec::new(),
            });
    let confirmed_binding = if let Some(candidate) = protocol_candidate.clone() {
        Some(crate::model::LastKnownGoodBinding {
            schema_version: crate::model::LAST_KNOWN_GOOD_SCHEMA.to_string(),
            binding_id: candidate.candidate_id.clone(),
            repository_id: candidate.repository_id.clone(),
            project_id: candidate.project_id.clone(),
            context_id: candidate.context_id.clone(),
            git_revision: candidate.git_revision.clone(),
            observation_id: candidate.observation_id.clone(),
            event_id: None,
            graph_basis: candidate.graph_basis.clone(),
            actor: candidate.proposed_by.clone(),
            actor_kind: "human".to_string(),
            evidence: candidate.evidence.clone(),
            status: "confirmed".to_string(),
            predecessor_binding_id: None,
            target_binding_id: None,
            supersedes_binding_id: None,
            created_at: candidate.proposed_at,
            validation: Default::default(),
        })
    } else {
        None
    };
    let Some(confirmed_binding) = confirmed_binding.clone() else {
        limitations.push(if context.last_known_good_basis.is_some() {
            "legacy last-known-good basis is legacy-unbound; no historical range was inspected."
                .to_string()
        } else {
            "last-known-good basis is unavailable; no historical range was inspected.".to_string()
        });
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: None,
            last_known_good_binding: None,
            last_known_good_candidate: protocol_candidate.clone(),
            last_known_good_state: protocol_state.clone(),
            last_known_good_applicability: protocol_applicability.clone(),
            last_known_good_status: if context.last_known_good_basis.is_some() {
                "legacy-unbound".to_string()
            } else {
                "unavailable".to_string()
            },
            range: None,
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
            limitations,
        });
    };

    let last_known_good = GitBasis {
        revision: confirmed_binding.git_revision.clone(),
    };
    let last_revision = resolve_revision(root, &last_known_good.revision)?;
    let current_revision = current_head(root)?;
    let range = format!("{last_revision}..{current_revision}");
    if graph.source_revision != current_revision || git_is_dirty(root) {
        limitations.push(
            "historical diagnosis is unavailable because the persisted graph does not match a clean current Git source state.".to_string(),
        );
        if graph.source_revision != current_revision {
            limitations.push(format!(
                "source revision mismatch: graph={} current={current_revision}",
                graph.source_revision
            ));
        }
        if git_is_dirty(root) {
            limitations.push(
                "Git working tree is dirty; historical candidates were not computed.".to_string(),
            );
        }
        let mut omissions = vec![
            "historical candidates unavailable for dirty or mismatched source state".to_string(),
        ];
        if limits.max_commits == 0 {
            omissions.push("history commits omitted because max_commits is zero".to_string());
        }
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            last_known_good_binding: Some(confirmed_binding.clone()),
            last_known_good_candidate: protocol_candidate.clone(),
            last_known_good_state: protocol_state.clone(),
            last_known_good_applicability: protocol_applicability.clone(),
            last_known_good_status: "confirmed".to_string(),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: limits.max_commits == 0,
            omissions,
            limitations,
        });
    }
    if last_revision == current_revision {
        limitations.push(
            "last-known-good and current revisions are identical; the inspected range is empty."
                .to_string(),
        );
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            last_known_good_binding: Some(confirmed_binding.clone()),
            last_known_good_candidate: protocol_candidate.clone(),
            last_known_good_state: protocol_state.clone(),
            last_known_good_applicability: protocol_applicability.clone(),
            last_known_good_status: "confirmed".to_string(),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: false,
            omissions: Vec::new(),
            limitations,
        });
    }

    if limits.max_commits == 0 {
        limitations
            .push("history limit max_commits is zero; no commits were inspected.".to_string());
        return Ok(HistoricalDiagnosis {
            schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
            context_id: context.id,
            current_graph_basis: current_basis,
            last_known_good_basis: Some(GitBasis {
                revision: last_revision,
            }),
            last_known_good_binding: Some(confirmed_binding.clone()),
            last_known_good_candidate: protocol_candidate.clone(),
            last_known_good_state: protocol_state.clone(),
            last_known_good_applicability: protocol_applicability.clone(),
            last_known_good_status: "confirmed".to_string(),
            range: Some(range),
            commits_inspected: 0,
            candidates: Vec::new(),
            truncated: true,
            omissions: vec!["history commits omitted because max_commits is zero".to_string()],
            limitations,
        });
    }

    let (focus_paths, cone_paths, focus_flow_ids, mut focus_limitations) =
        focus_paths(root, &context, &graph, &limits)?;
    let focus_limit_truncated = context.focus_context_refs.len() > limits.max_context_refs;
    limitations.append(&mut focus_limitations);
    let log_limit = limits.max_commits.saturating_add(1);
    let commits = git_log(root, &last_revision, &current_revision, log_limit)?;
    let truncated_commits = commits.len() > limits.max_commits;
    let inspected = commits
        .into_iter()
        .take(limits.max_commits)
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    let mut omissions = Vec::new();
    let mut snapshot_cache = BTreeMap::new();
    let path_bound_zero = limits.max_paths == 0;
    let snapshot_bound_zero = limits.max_snapshot_bytes == 0;
    if path_bound_zero {
        omissions.push("historical candidate paths capped at zero".to_string());
    }
    if snapshot_bound_zero {
        omissions.push("historical snapshot bytes capped at zero".to_string());
    }

    for commit in &inspected {
        let mut changed_paths = git_changed_paths(
            root,
            &commit.sha,
            commit.parents.first().map(String::as_str),
        )?;
        changed_paths.sort();
        changed_paths.dedup();
        let original_path_count = changed_paths.len();
        changed_paths.retain(|path| {
            is_typescript_path(path)
                || (*path == "package.json" && !context.focus_flow_refs.is_empty())
        });
        if changed_paths.is_empty() {
            continue;
        }
        if path_bound_zero {
            continue;
        }
        let mut reasons = Vec::new();
        let mut score = 10_u32;
        if changed_paths.iter().any(|path| focus_paths.contains(path)) {
            reasons.push("changed-path-in-focus-context".to_string());
            score += 100;
        }
        if changed_paths.iter().any(|path| cone_paths.contains(path)) {
            reasons.push("changed-path-in-dependency-cone".to_string());
            score += 60;
        }
        if limits.max_snapshot_bytes > 0 && !commit.parents.is_empty() {
            match historical_delta_reasons(
                root,
                commit,
                &focus_paths,
                &cone_paths,
                &focus_flow_ids,
                &limits,
                &mut snapshot_cache,
            ) {
                Ok((delta_reasons, delta_score, snapshot_notes)) => {
                    for reason in delta_reasons {
                        if !reasons.contains(&reason) {
                            reasons.push(reason);
                        }
                    }
                    score += delta_score;
                    for note in snapshot_notes {
                        limitations.push(format!("commit {}: {note}", commit.sha));
                    }
                }
                Err(error) => limitations.push(format!(
                    "historical graph snapshot unavailable for {}: {error}",
                    commit.sha
                )),
            }
        }
        if reasons.is_empty() {
            continue;
        }
        reasons.push("introduced-after-last-known-good".to_string());
        let changed_paths_truncated = changed_paths.len() > limits.max_paths;
        if changed_paths_truncated {
            changed_paths.truncate(limits.max_paths);
            omissions.push(format!(
                "commit {} paths capped at {}",
                commit.sha, limits.max_paths
            ));
        }
        if original_path_count > changed_paths.len() && !changed_paths_truncated {
            omissions.push(format!(
                "commit {} contained non-TypeScript paths omitted from candidate evidence",
                commit.sha
            ));
        }
        let current_files = graph
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.hash.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut retained_path = false;
        let mut changed_path = false;
        let mut removed_path = false;
        for path in &changed_paths {
            if path == "package.json" {
                let current_hash = graph
                    .entry_evidence
                    .manifest
                    .as_ref()
                    .map(|manifest| manifest.hash.as_str());
                match (current_hash, git_show_bytes(root, &commit.sha, path)) {
                    (Some(current_hash), Ok(bytes))
                        if blake3::hash(&bytes).to_hex().to_string() == current_hash =>
                    {
                        retained_path = true;
                    }
                    (Some(_), Ok(_)) => {
                        retained_path = true;
                        changed_path = true;
                    }
                    _ => removed_path = true,
                }
                continue;
            }
            let Some(current_hash) = current_files.get(path.as_str()) else {
                removed_path = true;
                continue;
            };
            retained_path = true;
            match git_show_bytes(root, &commit.sha, path) {
                Ok(bytes) if blake3::hash(&bytes).to_hex().to_string() == *current_hash => {}
                Ok(_) => changed_path = true,
                Err(_) => changed_path = true,
            }
        }
        let retention_status = if !retained_path {
            "removed"
        } else if changed_path || removed_path {
            "changed"
        } else {
            "retained"
        };
        let id_input = format!(
            "flopeek-historical-candidate-v1\0{}\0{}\0{}",
            context.id, current_basis.graph_id, commit.sha
        );
        candidates.push(HistoricalCandidate {
            schema_version: HISTORICAL_CANDIDATE_SCHEMA.to_string(),
            id: format!("candidate_{}", blake3::hash(id_input.as_bytes()).to_hex()),
            project_id: graph.project_id.clone(),
            context_id: context.id.clone(),
            current_graph_basis: current_basis.clone(),
            last_known_good_revision: last_revision.clone(),
            commit: commit.sha.clone(),
            parents: commit.parents.clone(),
            summary: commit.summary.clone(),
            changed_paths,
            changed_paths_truncated,
            relevance_reasons: reasons,
            score,
            retention_status: retention_status.to_string(),
        });
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.commit.cmp(&right.commit))
    });
    let mut truncated =
        truncated_commits || path_bound_zero || snapshot_bound_zero || focus_limit_truncated;
    if truncated_commits {
        omissions.push(format!("history commits capped at {}", limits.max_commits));
    }
    if candidates.len() > limits.max_candidates {
        candidates.truncate(limits.max_candidates);
        truncated = true;
        omissions.push(format!(
            "historical candidates capped at {}",
            limits.max_candidates
        ));
    }
    let omission_limit = limits.max_paths.max(1);
    if omissions.len() > omission_limit {
        omissions.truncate(omission_limit);
        truncated = true;
    }
    let diagnosis = HistoricalDiagnosis {
        schema_version: HISTORICAL_DIAGNOSIS_SCHEMA.to_string(),
        context_id: context.id,
        current_graph_basis: current_basis,
        last_known_good_basis: Some(GitBasis {
            revision: last_revision,
        }),
        last_known_good_binding: Some(confirmed_binding),
        last_known_good_candidate: protocol_candidate,
        last_known_good_state: protocol_state,
        last_known_good_applicability: protocol_applicability,
        last_known_good_status: "confirmed".to_string(),
        range: Some(range),
        commits_inspected: inspected.len(),
        candidates,
        truncated,
        omissions,
        limitations,
    };
    store::persist_historical_candidates(root, &diagnosis)?;
    Ok(diagnosis)
}
