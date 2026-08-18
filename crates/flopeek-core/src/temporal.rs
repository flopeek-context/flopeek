//! Pure temporal identity rules shared by storage and protocol orchestration.

pub const NODE_FINGERPRINT_CONTRACT: &str = "node-ast-and-direct-edges/v1";
pub const LEGACY_FILE_FINGERPRINT_CONTRACT: &str = "legacy-file-v1";

pub fn observation_event_id(
    project_id: &str,
    predecessor_event_id: Option<&str>,
    observation_id: &str,
) -> String {
    let input = format!(
        "flopeek-observation-event-v1\0{project_id}\0{}\0{observation_id}",
        predecessor_event_id.unwrap_or_default()
    );
    format!(
        "observation_event_{}",
        blake3::hash(input.as_bytes()).to_hex()
    )
}

pub fn fingerprint_contract(scope: &str) -> &'static str {
    match scope {
        "ast-and-direct-edges" => NODE_FINGERPRINT_CONTRACT,
        "legacy-file-v1" => LEGACY_FILE_FINGERPRINT_CONTRACT,
        _ => LEGACY_FILE_FINGERPRINT_CONTRACT,
    }
}
