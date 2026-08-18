# TypeScript diagnostic fixtures

These fixtures are intentionally small and deterministic. They exercise the Rust
TypeScript/TSX parser, graph identity, Context Ref freshness, and the historical
diagnosis contract without requiring a model or runtime execution.

`history/` is a source-level A–E history:

- A — checkout/payment is last-known-good;
- B — retry handling is introduced;
- C — an unrelated documentation-only change is introduced;
- D — the timeout branch changes;
- E — the current/bad state.

The history manifest describes relevance as a deterministic candidate signal. It
does not label any change as a cause or root cause.

`flows/` adds a bounded root `package.json` with supported and unsupported entry
forms, direct imports, a constructor, static call edges, cycle-safe traversal,
and a related TypeScript test. It exercises entry evidence, Flow Ref freshness,
deterministic static BFS ordering, and related-test false-positive controls.

The P1 maturity coverage also verifies:

- `scripts`, `bin`, `main`, and `module` entries, including explicit abstention
  for shell composition, flags, repository escapes, missing targets, JavaScript,
  and declaration-file targets;
- deterministic flow caps, cycle termination, and categorical omissions;
- strong direct-call/direct-construct versus weak direct-import test evidence;
- Flow Ref origin immutability, observation refresh, fingerprint matching, stale
  identity, and wrong-project handling;
- bounded historical flow candidates and Diagnostic Packets without runtime or
  root-cause claims.

All flow paths are static traversals over proven parser edges. The fixture does
not claim command invocation, execution order, runtime behavior, or business
intent.
