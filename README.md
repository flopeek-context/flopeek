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

Portable repository identity is opt-in and explicit. A tracked root
`.flopeek-repository.json` using `flopeek-repository-identity/v1` supplies a
stable `repo_<uuid>` identity; the scanner only reads it. With a valid
manifest, `projectId` and new Context Refs are repository-scoped across
checkouts. Without one, Flopeek remains usable in explicit `checkout-local`
mode and reports cross-checkout context as unavailable. Legacy checkout-local
refs are retained through same-database aliases and are never silently treated
as portable. Checkout paths and manifest bodies are not persisted in portable
evidence.

Context Refs use node-level freshness. A reference is `current` when its
canonical AST/evidence fingerprint and sorted direct-edge signatures still
match; focused symbol, rename/removal, and direct import/call changes resolve
as `stale` with an explicit reason. Legacy references use a conservative
file-level fallback or resolve as `unresolved`. Resolution includes origin and
current graph bases, observation IDs, fingerprint scope, and the deterministic
freshness reason.

Each scan also records an immutable observation event. Repeated scans of the
same observation are idempotent; a later scan is linked with `observed-after`,
which describes local observation order rather than Git ancestry or runtime
execution. `getObservationContinuity` returns a bounded chain and explicitly
reports structural-graph changes, truncation, omissions, and limitations.

Context reconciliation is read-only and conservative. When an origin node is
missing, an exact-compatible current canonical ref is reported as a stale
candidate with reason `unique-exact-compatible-fingerprint-candidate`;
ambiguous, missing, legacy, or corrupted evidence remains `stale`, `unresolved`,
or `unavailable` with a deterministic reason. Candidate evidence is never
stored as a successor URI. `superseded` is reserved for future proven lineage
or explicit attributed verification. This does not claim semantic renames,
runtime equivalence, or business intent. `reconcileContextRef` exposes the
bounded candidate evidence and evaluation event without storing guessed
mappings.

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

Class semantics add qualified class members for instance methods, static
methods, explicit constructors, and interface method signatures. Overload
declarations coalesce by structural identity. `extends`, `implements`, and
interface `extends` are structural edges. Private `this` calls, static class
calls, and statically defensible `new Class()` expressions resolve to symbol
nodes; default constructors use an explicit `constructs` edge to the class.
Public or inherited `this` dispatch, `super`, computed members, mixins,
getters/setters, callable fields, and other dynamic forms remain unresolved
with deterministic reasons.

Framework-neutral TypeScript context flows are also available from the root
`package.json`. Literal `scripts`, `bin`, `main`, and `module` targets are
bounded and resolved to known `.ts`/`.tsx` files; unsupported runners, shell
composition, JavaScript output, declaration files, missing targets, and
repository escapes remain explicit entry evidence. Each supported entry has a
stable Flow Ref and a deterministic breadth-first static traversal that follows
only proven `calls` and `constructs` edges, with cycle and byte/step bounds.
The traversal is structural evidence, never execution order or a runtime claim.

Related-test evidence is classified centrally: direct calls and constructions
are strong, direct non-type-only imports are weak, and test-to-test, naming-only,
transitive, and type-only relationships are excluded. JSONL exposes
`listFlows`, `getFlow`, `resolveFlowRef`, and `getRelatedTests`; `scan` returns
both node Context Refs and Flow Refs. Flow freshness compares entry identity,
step fingerprints, traversed topology, and exact related-test records while
keeping origin observation provenance immutable. The root manifest's exact
fingerprint is stored only as bounded metadata, never its body or script text.

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
node and flow freshness, bounded candidates, related-test evidence, omissions,
and unresolved limitations. Historical snapshots include bounded package
manifest metadata so entry changes can be reported as candidates without
claiming causality.

Last-known-good is explicit engineering evidence. The JSONL methods
`createLastKnownGoodBinding`, `getLastKnownGood`, `listLastKnownGoodHistory`, and
`validateLastKnownGood` persist an append-only binding per Diagnostic Context.
Only a human actor may confirm, reject, revoke, or supersede a binding; agents and
tools may propose one. Legacy `lastKnownGoodBasis` remains readable as
`legacy-unbound` and is not used for new historical diagnosis. Flopeek never
infers last-known-good from tests, commit messages, graph similarity, or candidate
ranking.

Adjacent local observations also expose `getObservationDelta`. The response
compares only the direct predecessor event and records bounded source-path,
node, edge, and flow changes when both graph derivation and fingerprint
contracts are compatible. Legacy or incompatible graph contracts are
`unavailable`; they are never compared by guesswork. The delta reports exact
source/config/entry basis relations from each observation's immutable source
manifest, truncation, and omissions without source bodies, runtime order, rename
claims, or root-cause claims. Observation events
remain local `observed-after` evidence rather than Git ancestry.

`getHistoricalContextContinuity` compares one Context Ref across two adjacent
Git snapshots. The default target is `HEAD` and the default source is its first
parent. It reports bounded path, focused-node, direct-edge, and focused-flow
changes plus exact-fingerprint lineage candidates. Rename/copy evidence is
candidate evidence only; it never creates a successor URI or automatic
supersession. Dirty state, incompatible snapshots, missing parents, and
repository mismatches remain explicitly unavailable.

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
