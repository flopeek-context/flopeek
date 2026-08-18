//! Bounded historical Git adapter.

#[allow(unused_imports)]
use super::*;

pub(super) fn git_tree_paths(root: &Path, revision: &str) -> Result<Vec<String>, String> {
    let output = git_output(root, &["ls-tree", "-r", "--name-only", revision, "--"])?;
    Ok(output
        .lines()
        .map(|path| path.replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect())
}

pub(super) fn historical_config_paths(
    root: &Path,
    revision: &str,
    paths: &[String],
) -> Result<Vec<String>, String> {
    if !paths.iter().any(|path| path == "tsconfig.json") {
        return Ok(Vec::new());
    }
    let path_set = paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    let mut current = "tsconfig.json".to_string();
    while result.len() < MAX_CONFIG_FILES {
        if result.iter().any(|path| path == &current) {
            return Ok(result);
        }
        if !path_set.contains(&current) {
            return Ok(result);
        }
        let bytes = git_show_bytes(root, revision, &current)?;
        result.push(current.clone());
        let Some(parent) = config_extends(&current, &bytes)? else {
            break;
        };
        current = parent;
    }
    if result.len() >= MAX_CONFIG_FILES {
        return Err(format!(
            "historical-tsconfig-files-capped-at-{MAX_CONFIG_FILES}"
        ));
    }
    Ok(result)
}

pub(super) fn git_show_bytes(root: &Path, revision: &str, path: &str) -> Result<Vec<u8>, String> {
    let object = format!("{revision}:{path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", "--format=", "--no-ext-diff", &object])
        .output()
        .map_err(|error| format!("Unable to execute historical source query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("Historical source query failed for {revision}:{path}.")
        } else {
            format!("Historical source query failed for {revision}:{path}: {detail}")
        });
    }
    Ok(output.stdout)
}

pub(super) fn safe_relative_path(path: &str) -> bool {
    let candidate = Path::new(path);
    !candidate.is_absolute()
        && candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

pub(super) fn current_head(root: &Path) -> Result<String, String> {
    git_output(root, &["rev-parse", "--verify", "HEAD"])
}

pub(super) fn git_is_dirty(root: &Path) -> bool {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let path = line.get(3..).unwrap_or_default().replace('\\', "/");
        !(path == ".flopeek" || path.starts_with(".flopeek/"))
    })
}

pub(super) fn resolve_revision(root: &Path, revision: &str) -> Result<String, String> {
    validate_revision(revision)?;
    let expression = format!("{revision}^{{commit}}");
    git_output(root, &["rev-parse", "--verify", &expression])
}

pub(super) fn git_log(
    root: &Path,
    last_known_good: &str,
    current: &str,
    max_count: usize,
) -> Result<Vec<CommitRecord>, String> {
    let range = format!("{last_known_good}..{current}");
    let max = max_count.to_string();
    let output = git_output(
        root,
        &[
            "log",
            "--first-parent",
            "--max-count",
            &max,
            "--format=%H%x00%P%x00%s",
            &range,
            "--",
        ],
    )?;
    let mut records = Vec::new();
    for line in output.lines() {
        let mut fields = line.splitn(3, '\0');
        let Some(sha) = fields.next() else { continue };
        let Some(parents) = fields.next() else {
            continue;
        };
        let Some(summary) = fields.next() else {
            continue;
        };
        if sha.is_empty() {
            continue;
        }
        records.push(CommitRecord {
            sha: sha.to_string(),
            parents: parents.split_whitespace().map(ToOwned::to_owned).collect(),
            summary: summary.chars().take(512).collect(),
        });
    }
    Ok(records)
}

pub(super) fn git_changed_paths(
    root: &Path,
    commit: &str,
    first_parent: Option<&str>,
) -> Result<Vec<String>, String> {
    let output = if let Some(parent) = first_parent {
        git_output_bytes(
            root,
            &[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "--find-copies",
                "--diff-filter=ACDMRT",
                parent,
                commit,
                "--",
            ],
        )?
    } else {
        git_output_bytes(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-status",
                "-z",
                "--find-renames",
                "--find-copies",
                "--root",
                "-r",
                "--diff-filter=ACDMRT",
                commit,
                "--",
            ],
        )?
    };
    let fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).replace('\\', "/"))
        .collect::<Vec<_>>();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = &fields[index];
        index += 1;
        let Some(path) = fields.get(index) else { break };
        index += 1;
        paths.push(path.clone());
        if (status.starts_with('R') || status.starts_with('C'))
            && let Some(new_path) = fields.get(index)
        {
            paths.push(new_path.clone());
            index += 1;
        }
    }
    Ok(paths)
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("Unable to execute bounded Git query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "Bounded Git query failed.".to_string()
        } else {
            format!("Bounded Git query failed: {detail}")
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn git_output_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("Unable to execute bounded Git query: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "Bounded Git query failed.".to_string()
        } else {
            format!("Bounded Git query failed: {detail}")
        });
    }
    Ok(output.stdout)
}
