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
