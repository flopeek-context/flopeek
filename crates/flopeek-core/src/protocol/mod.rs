//! Small JSONL boundary over the Rust/SQLite authority.

mod api;
mod dispatch;
mod orchestration;
mod params;

#[cfg(test)]
mod tests;

pub use api::serve_jsonl;
pub use orchestration::{scan_project, status_project};
