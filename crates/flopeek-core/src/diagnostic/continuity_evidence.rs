//! Pure evidence comparison helpers for adjacent historical snapshots.

use crate::model::{HistoricalPathChange, HistoricalSnapshot, ObservationBasisRelations};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn snapshot_basis_relations(
    before: &HistoricalSnapshot,
    after: &HistoricalSnapshot,
) -> ObservationBasisRelations {
    let source_before = crate::graph::exact_source_fingerprint(&before.files).unwrap_or_default();
    let source_after = crate::graph::exact_source_fingerprint(&after.files).unwrap_or_default();
    ObservationBasisRelations {
        typescript_source: fingerprint_relation(&source_before, &source_after),
        module_resolution_exact: fingerprint_relation(
            &before.module_resolution.exact_fingerprint,
            &after.module_resolution.exact_fingerprint,
        ),
        module_resolution_effective: fingerprint_relation(
            &before.module_resolution.effective_fingerprint,
            &after.module_resolution.effective_fingerprint,
        ),
        entry_manifest_exact: fingerprint_relation(
            &before.entry_evidence.exact_fingerprint,
            &after.entry_evidence.exact_fingerprint,
        ),
        entry_manifest_effective: fingerprint_relation(
            &before.entry_evidence.effective_fingerprint,
            &after.entry_evidence.effective_fingerprint,
        ),
    }
}

fn fingerprint_relation(before: &str, after: &str) -> String {
    if before.is_empty() || after.is_empty() {
        "unavailable".to_string()
    } else if before == after {
        "same".to_string()
    } else {
        "changed".to_string()
    }
}

pub(super) fn unavailable_basis_relations() -> ObservationBasisRelations {
    ObservationBasisRelations {
        typescript_source: "unavailable".to_string(),
        module_resolution_exact: "unavailable".to_string(),
        module_resolution_effective: "unavailable".to_string(),
        entry_manifest_exact: "unavailable".to_string(),
        entry_manifest_effective: "unavailable".to_string(),
    }
}

pub(super) fn path_changes(
    before: &HistoricalSnapshot,
    after: &HistoricalSnapshot,
) -> Vec<HistoricalPathChange> {
    let before = before
        .files
        .iter()
        .map(|file| (file.path.clone(), file.hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .files
        .iter()
        .map(|file| (file.path.clone(), file.hash.clone()))
        .collect::<BTreeMap<_, _>>();
    before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| {
            let before_hash = before.get(&path).cloned();
            let after_hash = after.get(&path).cloned();
            let status = match (&before_hash, &after_hash) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(left), Some(right)) if left != right => "changed",
                _ => return None,
            };
            Some(HistoricalPathChange {
                path,
                status: status.to_string(),
                before_hash,
                after_hash,
            })
        })
        .collect()
}

pub(super) fn path_lineage_candidates(changes: &[HistoricalPathChange]) -> Vec<String> {
    let mut removed = BTreeMap::<&str, Vec<&str>>::new();
    let mut added = BTreeMap::<&str, Vec<&str>>::new();
    for change in changes {
        match change.status.as_str() {
            "removed" => {
                if let Some(hash) = change
                    .before_hash
                    .as_deref()
                    .filter(|hash| !hash.is_empty())
                {
                    removed.entry(hash).or_default().push(&change.path);
                }
            }
            "added" => {
                if let Some(hash) = change.after_hash.as_deref().filter(|hash| !hash.is_empty()) {
                    added.entry(hash).or_default().push(&change.path);
                }
            }
            _ => {}
        }
    }
    let mut candidates = Vec::new();
    for (hash, old_paths) in removed {
        if let Some(new_paths) = added.get(hash) {
            for old_path in old_paths {
                for new_path in new_paths {
                    candidates.push(format!(
                        "rename-or-copy-candidate:{old_path}->{new_path}:fingerprint={hash}"
                    ));
                }
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

pub(super) fn bound_path_lineage(
    mut values: Vec<String>,
    limit: usize,
    truncated: &mut bool,
    omissions: &mut Vec<String>,
) -> Vec<String> {
    if values.len() > limit {
        values.truncate(limit);
        *truncated = true;
        omissions.push(format!(
            "historical path lineage candidates capped at {limit}"
        ));
    }
    values
}

pub(super) fn bound_paths(
    mut values: Vec<HistoricalPathChange>,
    limit: usize,
    truncated: &mut bool,
    omissions: &mut Vec<String>,
) -> Vec<HistoricalPathChange> {
    if values.len() > limit {
        values.truncate(limit);
        *truncated = true;
        omissions.push(format!("historical path changes capped at {limit}"));
    }
    values
}
