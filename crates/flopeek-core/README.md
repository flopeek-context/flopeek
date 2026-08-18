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

Direct TypeScript import bindings are represented explicitly for named aliases,
default imports, namespace imports, side-effect imports, and type-only imports.
Symbol-level call edges also follow bounded local re-export/barrel chains and the
root `tsconfig.json` `baseUrl`/`paths` subset, including local JSONC `extends`
chains. Relative and mapped lookup covers known extensions and directory
indexes. Unresolved, ambiguous, external, dynamic, type-only, cyclic, and
invalid-config references remain bounded resolution evidence without guessing.
Nested project configs, project references, package resolution, and package
exports are explicitly unavailable.

Class semantics expose qualified instance/static method nodes, explicit
constructor nodes, interface method signatures, and structural `extends` /
`implements` edges. Overloads coalesce by identity. Private `this` calls,
static class calls, and statically defensible constructor expressions resolve
to symbol-level edges; public or inherited dispatch, `super`, computed
members, mixins, getters/setters, and callable fields remain explicitly
unresolved.
