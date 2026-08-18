//! JSONL and CLI-facing protocol orchestration.

use serde_json::json;
use std::io::{BufRead, Write};

use crate::model::PROTOCOL_SCHEMA;

use super::dispatch::{self, ErrorBody, Request};

pub fn serve_jsonl<R: BufRead, W: Write>(reader: R, mut writer: W) -> Result<(), String> {
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Unable to read JSONL request: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch::handle_request(request),
            Err(error) => json!({
                "schemaVersion": PROTOCOL_SCHEMA,
                "ok": false,
                "error": ErrorBody { code: "invalid-request".to_string(), message: error.to_string() },
            }),
        };
        serde_json::to_writer(&mut writer, &response)
            .map_err(|error| format!("Unable to write JSONL response: {error}"))?;
        writer
            .write_all(b"\n")
            .map_err(|error| format!("Unable to terminate JSONL response: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("Unable to flush JSONL response: {error}"))?;
    }
    Ok(())
}
