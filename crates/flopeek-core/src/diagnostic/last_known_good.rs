//! Git-only validation for last-known-good bindings.

use super::*;

pub(crate) fn validate_revision_range(root: &Path, revision: &str) -> Result<String, String> {
    let resolved = resolve_revision(root, revision)?;
    let current = current_head(root)?;
    let range = format!("{resolved}..{current}");
    let _ = git_output(
        root,
        &["rev-list", "--first-parent", "--max-count=1", &range],
    )?;
    Ok(resolved)
}
