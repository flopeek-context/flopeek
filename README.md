# Flopeek repository memory

Flopeek is a local-first product for versioned repository engineering context:
deterministic TypeScript/TSX source evidence, graph identity, Context Ref
freshness, and (as the diagnostic layers land) historical change candidates.

The active implementation authority is Rust. SQLite is the canonical local
store. No LLM, network service, target-runtime execution, or JavaScript fallback
is required to build repository evidence.

## Rust foundation

The root is a Cargo workspace containing `crates/flopeek-core`.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --locked
```

The CLI is built as `flopeek`:

```powershell
cargo run -p flopeek-core -- scan .
cargo run -p flopeek-core -- status .
cargo run -p flopeek-core -- serve
```

`scan` discovers only `.ts` and `.tsx`, parses bounded structural facts with
Tree-sitter, builds a deterministic graph, and commits evidence atomically to
`.flopeek/flopeek.sqlite3`. Source bodies are never persisted. Context Refs
carry project, graph, version, and node identity; an older reference resolves as
`stale`, never silently as current.

Static evidence does not prove runtime behavior or root cause. Dynamic dispatch,
reflection, generated source, and business intent remain explicitly unavailable.

## Scope and provenance

The TypeScript/TSX pilot is intentionally narrow. Historical Flopeek Core source
(`badsleepyday/flopeek-core`, `development`, `72a95fe1a6497683e96e90872438cd3c83b7272f`)
is provenance only; this repository is an independent product line. The active
machine contract is [`contracts/product.json`](contracts/product.json), and the
operating contract is [`AGENTS.md`](AGENTS.md).

Deterministic parser and history fixtures live under
[`fixtures/typescript`](fixtures/typescript). They include an A–E diagnostic
history in which retry and timeout edits are candidates, an unrelated edit ranks
low, and no edit is labeled a cause.
