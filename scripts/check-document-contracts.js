"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { DOCUMENTS, assertGeneratedDocuments, buildProductContractFromInputs, canonicalText, loadProductContractInputs } = require("./product-contract");

const root = path.resolve(__dirname, "..");
const read = (relativePath) => fs.readFileSync(path.join(root, relativePath), "utf8");
const failures = [];
const requireMatch = (text, pattern, label) => { if (!pattern.test(text)) failures.push(`Missing ${label}.`); };
const rejectMatch = (text, pattern, label) => { if (pattern.test(text)) failures.push(`Stale ${label}.`); };

const packageJson = JSON.parse(read("package.json"));
const support = read("SUPPORT.md");
const roadmap = read("ROADMAP.md");
const architecture = read("ARCHITECTURE.md");
const releasing = read("RELEASING.md");

if (packageJson.license !== "Apache-2.0") failures.push("package.json must declare Apache-2.0.");
if (!fs.existsSync(path.join(root, "LICENSE"))) failures.push("LICENSE must exist.");
requireMatch(releasing, /Public release is intentionally disabled while identity isolation is pending/, "pending identity-isolation release block");
requireMatch(releasing, /must not reuse the imported records/, "legacy approval quarantine");
requireMatch(support, /Public package and GitHub release promotion are blocked until identity isolation is complete/, "support release block");
requireMatch(architecture, /Imported release automation is disabled until identity isolation is complete/, "architecture release block");
rejectMatch(`${support}\n${roadmap}\n${architecture}`, /private development source of truth|private-development to public-source projection|public repository creation, visibility change/i, "retired private-to-public source model");
rejectMatch(`${support}\n${roadmap}\n${architecture}`, /export:public-repository|audit:public-repository|public-snapshot\.yml/, "retired public snapshot tooling");
requireMatch(support, /fixture gate reports 40\/40 expected relationships/, "current fixture corpus total");

try {
  const productContract = buildProductContractFromInputs(loadProductContractInputs(root));
  const committed = read("contracts/product-contract.json");
  if (canonicalText(committed) !== canonicalText(`${JSON.stringify(productContract, null, 2)}\n`)) failures.push("Generated product contract manifest is stale.");
  assertGeneratedDocuments(productContract, Object.fromEntries(DOCUMENTS.map((name) => [name, read(name)])));
} catch (error) {
  failures.push(`Product contract validation failed: ${error.message}`);
}

if (failures.length) {
  process.stderr.write(`Document contract check failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}\n`);
  process.exit(1);
}
process.stdout.write("Document contracts are current.\n");
