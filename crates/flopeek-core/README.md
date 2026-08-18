# `flopeek-core`

Rust implementation authority for the Flopeek repository-memory foundation.

The crate owns TypeScript/TSX discovery and parsing, graph identity and edges,
Context Ref freshness, SQLite persistence, and the `flopeek` CLI/JSONL boundary.
It stores hashes and structural facts, not source bodies or credentials.
Diagnostic Contexts, Assertions, and historical candidates are persisted in
explicit SQLite domain tables; they never advance `graphVersion`.
Bounded immutable Git-revision graph snapshots are cached separately under
`.flopeek/diagnostics/history/`.

The crate deliberately does not execute a target application, guess dynamic
dispatch, require an LLM, or emit runtime/root-cause claims.
