//! Trusted local command boundary for Last-Known-Good lifecycle actions.

use flopeek_core::model::{
    EvidenceReference, LastKnownGoodProposalRequest, LastKnownGoodTransitionRequest,
};
use flopeek_core::store;
use serde::Serialize;
use std::env;
use std::path::PathBuf;

fn value(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|arg| arg == name)
        .ok_or_else(|| format!("missing required option {name}"))?;
    args.get(index + 1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or_else(|| format!("option {name} requires a value"))
}

fn optional(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .filter(|value| !value.starts_with('-'))
        .cloned()
}

fn root(args: &[String]) -> Result<PathBuf, String> {
    let root = optional(args, "--path").map_or(
        env::current_dir().map_err(|error| error.to_string())?,
        PathBuf::from,
    );
    root.canonicalize()
        .map_err(|error| format!("Unable to resolve project path: {error}"))
}

fn context(args: &[String]) -> Result<String, String> {
    value(args, "--context")
}

fn expected_tip(args: &[String]) -> Result<Option<String>, String> {
    let value = value(args, "--expected-tip")?;
    if value == "none" {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    serde_json::to_writer_pretty(std::io::stdout(), value)
        .map_err(|error| format!("Unable to encode LKG CLI response: {error}"))?;
    println!();
    Ok(())
}

pub fn run_lkg(args: &[String]) -> Result<i32, String> {
    let action = args
        .first()
        .ok_or_else(|| "lkg requires an action".to_string())?;
    if matches!(action.as_str(), "--help" | "-h" | "help") {
        println!(
            "Usage: flopeek lkg <propose|review|confirm|reject|revoke|get|history> --context ID [options]\n\nTransitions require --actor, --reason, --idempotency-key, --expected-tip, and --ack-human.\nActor identity is caller-attributed, not authenticated."
        );
        return Ok(0);
    }
    let root = root(args)?;
    let context_id = context(args)?;
    match action.as_str() {
        "propose" => {
            let request = LastKnownGoodProposalRequest {
                context_id,
                git_revision: value(args, "--revision")?,
                actor: value(args, "--actor")?,
                reason: value(args, "--reason")?,
                evidence: Vec::<EvidenceReference>::new(),
                expected_tip_event_id: expected_tip(args)?,
                idempotency_key: value(args, "--idempotency-key")?,
                max_paths: optional(args, "--max-paths").and_then(|value| value.parse().ok()),
                max_snapshot_bytes: optional(args, "--max-snapshot-bytes")
                    .and_then(|value| value.parse().ok()),
            };
            print_json(&store::propose_last_known_good(&root, request)?)?;
        }
        "review" => print_json(&store::get_last_known_good_review_packet(
            &root,
            &context_id,
        )?)?,
        "get" => print_json(&store::get_last_known_good_protocol(&root, &context_id)?)?,
        "history" => {
            let limit = optional(args, "--limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(128);
            print_json(&store::list_last_known_good_protocol(
                &root,
                &context_id,
                limit,
            )?)?;
        }
        "confirm" | "reject" | "revoke" => {
            if !args.iter().any(|arg| arg == "--ack-human") {
                return Err(
                    "human lifecycle actions require --ack-human; actor identity is caller-attributed, not authenticated"
                        .to_string(),
                );
            }
            let request = LastKnownGoodTransitionRequest {
                context_id,
                actor: value(args, "--actor")?,
                reason: value(args, "--reason")?,
                evidence: Vec::new(),
                expected_tip_event_id: expected_tip(args)?,
                idempotency_key: value(args, "--idempotency-key")?,
                candidate_id: optional(args, "--candidate"),
            };
            let event = match action.as_str() {
                "confirm" => store::confirm_last_known_good_local(&root, request)?,
                "reject" => store::reject_last_known_good_local(&root, request)?,
                _ => store::revoke_last_known_good_local(&root, request)?,
            };
            print_json(&event)?;
        }
        other => return Err(format!("unsupported lkg action {other:?}")),
    }
    Ok(0)
}
