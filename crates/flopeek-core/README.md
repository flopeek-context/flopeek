# `flopeek-core`

Rust implementation authority for the Flopeek repository-memory foundation.

The crate owns TypeScript/TSX discovery and parsing, graph identity and edges,
Context Ref freshness, SQLite persistence, and the `flopeek` CLI/JSONL boundary.
It stores hashes and structural facts, not source bodies or credentials.

The crate deliberately does not execute a target application, guess dynamic
dispatch, require an LLM, or emit runtime/root-cause claims.
