# Flopeek repository memory

Flopeek is a local-first product for versioned repository engineering context:
deterministic TypeScript/TSX source evidence, graph identity, Context Ref
freshness, versioned diagnostic memory, and bounded historical change
candidates.

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
cargo run -p flopeek-core -- diagnose . CONTEXT_ID
cargo run -p flopeek-core -- packet . CONTEXT_ID
cargo run -p flopeek-core -- serve
```

`scan` discovers only `.ts` and `.tsx`, parses bounded structural facts with
Tree-sitter, builds a deterministic structural graph, and commits evidence
atomically to `.flopeek/flopeek.sqlite3`. Source bodies are never persisted.
Graph IDs represent TypeScript structural evidence and do not change merely
because the Git revision, whitespace, comments, or raw byte hash changed. Each
source state is retained as an immutable graph observation with its exact
source fingerprint and Git revision.

Context Refs use node-level freshness. A reference is `current` when its
canonical AST/evidence fingerprint and sorted direct-edge signatures still
match; focused symbol, rename/removal, and direct import/call changes resolve
as `stale` with an explicit reason. Legacy references use a conservative
file-level fallback or resolve as `unresolved`. Resolution includes origin and
current graph bases, observation IDs, fingerprint scope, and the deterministic
freshness reason.

TypeScript import evidence records named aliases, default imports, namespace
imports, side-effect imports, and type-only imports. Direct calls resolve through
same-module declarations, relative imports, bounded re-export/barrel chains, and
the root `tsconfig.json` `baseUrl`/`paths` subset. Local `extends` chains accept
JSONC and are bounded; nested project configs, project references, package
resolution, and package exports remain unavailable. Relative and mapped lookup
is deterministic across `.ts`, `.tsx`, `.d.ts`, and directory index files while
repository escapes are rejected. Resolved edges point from the caller symbol to
the callee symbol. Ambiguous, missing, external, dynamic, type-only, cyclic,
invalid-config, and unsupported module references remain explicit in bounded
graph `resolution_evidence` records and are never guessed. Each observation
retains config path/hash provenance without storing config bodies.

Static evidence does not prove runtime behavior or root cause. Dynamic dispatch,
reflection, generated source, and business intent remain explicitly unavailable.

Diagnostic Contexts, Assertions, and historical candidates are stored in the
SQLite authority with explicit domain tables; they do not advance
`graphVersion`.
Immutable Git-revision graph snapshots used for adjacent comparisons are cached
under `.flopeek/diagnostics/history/`; cache identity includes revision,
derivation, parser, path bound, and snapshot-byte bound, so a low-bound result
cannot satisfy a later high-bound request. Historical diagnosis compares each
non-root commit with its first parent (and roots with the empty tree), retains
both sides of rename/copy records, and reports only bounded candidate changes.
Dirty or source-mismatched history is explicitly `unavailable`; the packet can
still return current static evidence. Candidates are never labeled causes.
The JSONL service exposes `createDiagnosticContext`, `getDiagnosticContext`,
`appendDiagnosticAssertion`, `diagnoseHistory`, and `getDiagnosticPacket` in
addition to graph queries. Assertions retain their kind, actor, evidence class,
and lifecycle separately from deterministic historical candidates. A historical
packet reports the current graph basis, an explicit last-known-good revision,
node freshness, bounded candidates, omissions, and unresolved limitations.

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
