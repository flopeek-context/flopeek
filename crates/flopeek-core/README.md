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

The crate also derives framework-neutral TypeScript context flows from a bounded
root `package.json`. Literal supported script runners (`tsx`, `ts-node`,
`ts-node-esm`, `node`, `bun`, `bun run`, and `deno run`) plus literal `bin`,
`main`, and `module` targets become stable entry records when they resolve to a
known TypeScript/TSX file. Shell-composed commands, flags, wrappers, absolute
or escaping paths, JavaScript output, declaration files, and missing targets
are reported as explicit unavailable entry evidence. Flow traversal is a
deterministic, bounded BFS over only proven `calls` and `constructs` edges;
imports, declarations, heritage, dynamic references, and related-test matches
never become flow transitions.

`listFlows`, `getFlow`, `resolveFlowRef`, and `getRelatedTests` expose the
persisted flow projection. Related tests use only direct call/construct
evidence (strong) or direct non-type-only imports (weak), excluding naming,
global-symbol, test-to-test, type-only, and transitive guesses. Flow Refs keep
their origin observation immutable and resolve current versus stale using entry,
step, topology, and related-test fingerprints. Package manifest metadata is
limited to relative path, size, hash, normalized entry facts, bounds, and
omissions; command and manifest bodies are never persisted.
