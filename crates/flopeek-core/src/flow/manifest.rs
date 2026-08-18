//! Package entry manifest evidence.

use super::*;
pub(super) fn parse_manifest(
    root: &Path,
    files: &[crate::model::SourceFile],
) -> Result<(EntryEvidence, bool), String> {
    let path = root.join("package.json");
    let Ok(bytes) = fs::read(&path) else {
        return Ok((
            EntryEvidence {
                schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
                status: "complete".to_string(),
                manifest: None,
                exact_fingerprint: blake3::hash(b"missing-package-json").to_hex().to_string(),
                effective_fingerprint: blake3::hash(b"empty-entry-manifest").to_hex().to_string(),
                records: Vec::new(),
                truncated: false,
                omissions: Vec::new(),
                limitations: vec!["root-package-json-absent-no-entry-evidence".to_string()],
            },
            false,
        ));
    };
    let manifest = EntryManifest {
        path: "package.json".to_string(),
        bytes: bytes.len() as u64,
        hash: blake3::hash(&bytes).to_hex().to_string(),
    };
    let exact_fingerprint = manifest.hash.clone();
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Ok((
            EntryEvidence {
                schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
                status: "truncated".to_string(),
                manifest: Some(manifest),
                exact_fingerprint,
                effective_fingerprint: String::new(),
                records: Vec::new(),
                truncated: true,
                omissions: vec![format!("package.json exceeds {MAX_MANIFEST_BYTES} bytes")],
                limitations: vec!["entry-manifest-byte-bound-reached".to_string()],
            },
            true,
        ));
    }
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) if value.is_object() => value,
        Ok(_) => {
            return Ok((
                unavailable_manifest(manifest, exact_fingerprint, "package-json-not-object"),
                true,
            ));
        }
        Err(_) => {
            return Ok((
                unavailable_manifest(manifest, exact_fingerprint, "package-json-invalid"),
                true,
            ));
        }
    };
    let mut records = Vec::new();
    if let Some(scripts) = value.get("scripts").and_then(Value::as_object) {
        for (key, command) in scripts {
            let command = command.as_str();
            let (runner, target, reason) = command.map(parse_script).unwrap_or((
                None,
                None,
                "script-command-not-literal".to_string(),
            ));
            records.push(entry_record("script", key, runner, target, reason, files));
        }
    }
    if let Some(bin) = value.get("bin") {
        match bin {
            Value::String(target) => records.push(entry_record(
                "bin",
                "bin",
                None,
                Some(target.to_string()),
                "".to_string(),
                files,
            )),
            Value::Object(map) => {
                for (key, target) in map {
                    records.push(entry_record(
                        "bin",
                        key,
                        None,
                        target.as_str().map(ToOwned::to_owned),
                        if target.is_string() {
                            String::new()
                        } else {
                            "bin-target-not-string".to_string()
                        },
                        files,
                    ));
                }
            }
            _ => records.push(entry_record(
                "bin",
                "bin",
                None,
                None,
                "bin-not-string-or-object".to_string(),
                files,
            )),
        }
    }
    for field in ["main", "module"] {
        if let Some(value) = value.get(field) {
            records.push(entry_record(
                field,
                field,
                None,
                value.as_str().map(ToOwned::to_owned),
                if value.is_string() {
                    String::new()
                } else {
                    "entry-target-not-string".to_string()
                },
                files,
            ));
        }
    }
    records.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));
    let effective = records
        .iter()
        .map(|record| {
            (
                &record.kind,
                &record.key,
                &record.runner,
                &record.target_path,
                &record.status,
                &record.reason,
            )
        })
        .collect::<Vec<_>>();
    let effective_fingerprint = blake3::hash(&serde_json::to_vec(&effective).unwrap_or_default())
        .to_hex()
        .to_string();
    Ok((
        EntryEvidence {
            schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
            status: "complete".to_string(),
            manifest: Some(manifest),
            exact_fingerprint,
            effective_fingerprint,
            records,
            truncated: false,
            omissions: Vec::new(),
            limitations: vec![
                "only-root-package-json-is-considered".to_string(),
                "package-exports-and-package-manager-wrappers-unsupported".to_string(),
            ],
        },
        true,
    ))
}

fn unavailable_manifest(
    manifest: EntryManifest,
    exact_fingerprint: String,
    reason: &str,
) -> EntryEvidence {
    EntryEvidence {
        schema_version: ENTRY_EVIDENCE_SCHEMA.to_string(),
        status: "unavailable".to_string(),
        manifest: Some(manifest),
        exact_fingerprint,
        effective_fingerprint: String::new(),
        records: Vec::new(),
        truncated: false,
        omissions: vec![reason.to_string()],
        limitations: vec!["entry-manifest-cannot-be-parsed".to_string()],
    }
}

fn parse_script(command: &str) -> (Option<String>, Option<String>, String) {
    if command.is_empty()
        || command
            .chars()
            .any(|c| matches!(c, '&' | '|' | ';' | '>' | '<' | '`' | '$' | '\n' | '\r'))
    {
        return (
            None,
            None,
            "script-command-complex-or-shell-composed".to_string(),
        );
    }
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    if tokens
        .iter()
        .any(|token| token.starts_with('-') || token.contains('"') || token.contains('\''))
    {
        return (
            None,
            None,
            "script-command-flags-or-quoting-unsupported".to_string(),
        );
    }
    let (runner, target) = match tokens.as_slice() {
        [runner, target]
            if matches!(*runner, "tsx" | "ts-node" | "ts-node-esm" | "node" | "bun") =>
        {
            ((*runner).to_string(), *target)
        }
        ["bun", "run", target] | ["deno", "run", target] => (format!("{} run", tokens[0]), *target),
        _ => return (None, None, "unsupported-script-runner-or-arity".to_string()),
    };
    (Some(runner), Some(target.to_string()), String::new())
}

fn entry_record(
    kind: &str,
    key: &str,
    runner: Option<String>,
    target: Option<String>,
    reason: String,
    files: &[crate::model::SourceFile],
) -> EntryRecord {
    let mut record = EntryRecord {
        key: key.to_string(),
        kind: kind.to_string(),
        runner,
        target_path: None,
        target_node_id: None,
        status: "unresolved".to_string(),
        reason,
    };
    if record.reason.is_empty() {
        let Some(target) = target else {
            record.reason = "entry-target-missing".to_string();
            return record;
        };
        match resolve_target(&target, files) {
            Ok(path) => {
                record.target_path = Some(path);
                record.status = "resolved".to_string();
                record.reason = "known-typescript-target".to_string();
            }
            Err(reason) => record.reason = reason,
        }
    }
    record
}

fn resolve_target(target: &str, files: &[crate::model::SourceFile]) -> Result<String, String> {
    if target.is_empty()
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.contains(':')
    {
        return Err("entry-target-absolute-or-invalid".to_string());
    }
    let target = target.replace('\\', "/");
    let path = PathBuf::from(&target);
    let normalized = normalize_relative_path(&path)
        .map_err(|_| "entry-target-escapes-repository".to_string())?;
    let known = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let extension = Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension == "js" || extension == "jsx" {
        return Err("entry-target-javascript-output-unsupported".to_string());
    }
    if normalized.ends_with(".d.ts") {
        return Err("entry-target-declaration-file-unsupported".to_string());
    }
    let mut candidates = Vec::new();
    if known.contains(normalized.as_str()) {
        candidates.push(normalized.clone());
    } else if extension.is_empty() {
        for suffix in [".ts", ".tsx"] {
            candidates.push(format!("{normalized}{suffix}"));
        }
        candidates.push(format!("{normalized}/index.ts"));
        candidates.push(format!("{normalized}/index.tsx"));
    } else {
        candidates.push(normalized.clone());
    }
    if let Some(candidate) = candidates
        .iter()
        .find(|candidate| known.contains(candidate.as_str()))
    {
        return Ok(candidate.clone());
    }
    let declaration_candidates = [
        format!("{normalized}.d.ts"),
        format!("{normalized}/index.d.ts"),
    ];
    if declaration_candidates
        .iter()
        .any(|candidate| known.contains(candidate.as_str()))
    {
        return Err("entry-target-declaration-file-unsupported".to_string());
    }
    Err("entry-target-missing-or-not-typescript".to_string())
}

pub(super) fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    file.ends_with(".test.ts")
        || file.ends_with(".test.tsx")
        || file.ends_with(".spec.ts")
        || file.ends_with(".spec.tsx")
        || lower
            .split('/')
            .any(|part| matches!(part, "test" | "tests" | "__tests__"))
}
