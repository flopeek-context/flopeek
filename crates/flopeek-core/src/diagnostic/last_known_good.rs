//! Git-only validation for last-known-good bindings.

use super::*;

const MAX_FIRST_PARENT_VALIDATION_COMMITS: usize = 100_000;

pub(crate) fn resolve_last_known_good_revision(
    root: &Path,
    revision: &str,
) -> Result<String, String> {
    resolve_revision(root, revision)
}

pub(crate) fn validate_first_parent_range(root: &Path, revision: &str) -> Result<String, String> {
    let resolved = resolve_revision(root, revision)?;
    let current = current_head(root)?;
    let max_count = MAX_FIRST_PARENT_VALIDATION_COMMITS
        .saturating_add(1)
        .to_string();
    let lineage = git_output(
        root,
        &[
            "rev-list",
            "--first-parent",
            "--max-count",
            &max_count,
            &current,
        ],
    )?;
    for (index, candidate) in lineage.lines().enumerate() {
        if candidate == resolved && index < MAX_FIRST_PARENT_VALIDATION_COMMITS {
            return Ok(resolved);
        }
        if index >= MAX_FIRST_PARENT_VALIDATION_COMMITS {
            return Err(format!(
                "git-first-parent-lineage-validation-capped-at-{MAX_FIRST_PARENT_VALIDATION_COMMITS}"
            ));
        }
    }
    Err("git-revision-not-on-current-first-parent-lineage".to_string())
}
