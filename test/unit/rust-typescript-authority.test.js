"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { createConfiguredCoreClient, createSurfaceCoreRuntime } = require("../../src/core-runtime");
const { NativeProtocolClient } = require("../../src/native-protocol-client");
const { nativeTestCommand } = require("../helpers/native-test-command");

const ROOT = path.resolve(__dirname, "..", "..");

function nativeClient() {
  return new NativeProtocolClient({
    ...nativeTestCommand(ROOT),
    requestTimeoutMs: 120_000,
  });
}

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "flopeek-rust-ts-authority-"));
  fs.mkdirSync(path.join(root, "src"), { recursive: true });
  fs.writeFileSync(path.join(root, "package.json"), JSON.stringify({ name: "rust-ts-authority-fixture" }));
  fs.writeFileSync(path.join(root, "src", "checkout.ts"), [
    "export interface Checkout { id: string }",
    "export function loadCheckout(id: string): Checkout { return { id }; }",
    "",
  ].join("\n"));
  fs.writeFileSync(path.join(root, "src", "Checkout.tsx"), [
    'import { loadCheckout } from "./checkout";',
    "export function CheckoutView() {",
    '  const checkout = loadCheckout("current");',
    "  return <button data-checkout={checkout.id}>Pay</button>;",
    "}",
    "",
  ].join("\n"));
  return root;
}

test("Rust surface owns TypeScript/TSX truth and persists only to SQLite", async (context) => {
  const root = fixture();
  const runtime = createSurfaceCoreRuntime({ coreMode: "rust", native: nativeClient() });
  context.after(async () => {
    await runtime.core.close();
    fs.rmSync(root, { recursive: true, force: true });
  });

  assert.equal(runtime.selection.requestedMode, "rust");
  assert.equal(runtime.selection.selectedImplementation, "native");
  assert.equal(runtime.selection.sourceAuthority, "rust");
  assert.equal(runtime.selection.persistedAuthority, "sqlite");
  assert.equal(runtime.selection.fallback, null);

  const graph = await runtime.core.scan(root);
  assert.equal(runtime.core.sourceAuthority, "rust");
  assert.equal(runtime.core.parserHost, "rust-tree-sitter-source/v19");
  assert.equal(runtime.core.factEnvelopeHost, "rust-native-structural-batch/v1");
  assert.equal(graph.analysis.graphState.persistence, "sqlite");
  assert.equal(fs.existsSync(path.join(root, ".flopeek", "native-core.sqlite3")), true);
  assert.equal(fs.existsSync(path.join(root, ".flopeek", "graph.json")), false);

  const sourceFiles = graph.nodes
    .filter((node) => node.kind === "file" && node.sourceScope === "application")
    .map((node) => node.path)
    .sort();
  assert.deepEqual(sourceFiles, ["src/Checkout.tsx", "src/checkout.ts"]);
  assert.ok(graph.edges.some((edge) => edge.type === "imports" || edge.type === "calls"));
});

test("Rust authority failure is explicit and never invokes JavaScript", async () => {
  let javascriptReads = 0;
  const failure = Object.assign(new Error("native process unavailable"), { code: "rust-core-unavailable" });
  const nativeCore = {
    schemaVersion: "flopeek-core-client/v6",
    implementation: "native-experimental",
    sourceAuthority: "rust",
    scan: async () => { throw failure; },
    refresh: async () => { throw failure; },
    getLastCompleteGraph: async () => null,
    materializeGraph: async () => null,
    getScanStatus: async () => null,
    getProjectOverview: async () => null,
    findNodes: async () => null,
    getNode: async () => null,
    getRequestFlows: async () => null,
    getEntryFlows: async () => null,
    getFlowProjection: async () => null,
    getFlowContextCard: async () => null,
    getChangeImpact: async () => null,
    getGraphDelta: async () => null,
    getChangedContexts: async () => null,
    getRelatedTests: async () => null,
    getContextCard: async () => null,
    resolveContextRef: async () => null,
    close: async () => {},
  };
  const options = { mode: "rust", nativeCore };
  Object.defineProperty(options, "javascript", {
    get() {
      javascriptReads += 1;
      throw new Error("JavaScript authority must remain unavailable");
    },
  });
  const core = createConfiguredCoreClient(options);
  await assert.rejects(() => core.scan("ignored"), (error) => error === failure);
  assert.equal(javascriptReads, 0);
  await core.close();
});
