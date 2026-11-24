#!/usr/bin/env node
// Generates the app's contract modules straight from contracts/ (docs-api, doc-ir, context-pack). There is no vendored copy of the
// contract files: the inputs are read from the repository, and the outputs are checked in because the packaged app cannot read contracts/.
//   node scripts/gen-contract.mjs           write the generated files
//   node scripts/gen-contract.mjs --check   fail when any generated file differs from what contracts/ produces now (drift)
// Outputs (all under src/shared/super-ontology/):
//   docs-contract.generated.mjs / .d.mts   plain JS for the main process: error detail allow-list, retryability, event types
//   docs-api.generated.ts                  types, structural guards, endpoints, error codes and event types for the renderer
//   doc-ir.generated.ts, context-pack.generated.ts   types of the two companion schemas
// Only Node built-ins are used, so the check runs on any CI runner.
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const contractsRoot = path.resolve(appRoot, "..", "..", "contracts");
const outDirectory = path.join(appRoot, "src", "shared", "super-ontology");

const mode = process.argv[2];
if (mode !== undefined && mode !== "--check") {
  console.error("Usage: gen-contract.mjs [--check]");
  process.exit(2);
}

const INPUTS = {
  schema: "docs-api/docs-api.v1.schema.json",
  errors: "docs-api/errors.v1.json",
  events: "docs-api/events.v1.schema.json",
  changelog: "docs-api/CHANGELOG.md",
  docIr: "docs-ir/doc-ir.v1.schema.json",
  contextPack: "context-pack/context-pack.v1.schema.json",
};
const readText = (relative) => fs.readFileSync(path.join(contractsRoot, relative), "utf8");
const readJson = (relative) => JSON.parse(readText(relative));
const sha256 = (relative) => createHash("sha256").update(fs.readFileSync(path.join(contractsRoot, relative))).digest("hex");
const fingerprints = Object.fromEntries(Object.values(INPUTS).map((relative) => [relative, sha256(relative).slice(0, 16)]));

const schema = readJson(INPUTS.schema);
const errors = readJson(INPUTS.errors);
const events = readJson(INPUTS.events);

const problems = [];
// The contract version is the newest section of the docs-api CHANGELOG ("## 1.11 (...)"); every file must agree on the schema line.
const sections = [...readText(INPUTS.changelog).matchAll(/^## (\d+(?:\.\d+)*(?:-[A-Za-z0-9]+)?)(?: |$)/gmu)].map((m) => m[1]);
const numeric = sections.filter((version) => /^\d+(\.\d+)*$/u.test(version));
const contractVersion = numeric.sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).at(-1) ?? sections.at(-1);
if (!contractVersion) problems.push("docs-api/CHANGELOG.md has no version section");
for (const [label, version] of [["schema", schema["x-docs-api-version"]], ["errors", errors.api_version], ["events", events["x-docs-api-version"]]]) {
  if (version !== schema["x-docs-api-version"]) problems.push(`the ${label} file says ${version} but the schema says ${schema["x-docs-api-version"]}`);
}

// ---------------------------------------------------------------- naming helpers
const snake = (name) => name.replace(/([a-z0-9])([A-Z])/gu, "$1_$2").toUpperCase();
const pascal = (name) => name.split(/[_-]/u).map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join("");
const literal = (value) => JSON.stringify(value);
const comment = (text, indent = "") => (text
  ? `${indent}/** ${String(text).replace(/\*\//gu, "* /").replace(/\s+/gu, " ").trim()} */\n`
  : "");

// ---------------------------------------------------------------- schema -> TypeScript
const enumArrays = []; // { name, values, path }

function refName(ref) {
  const match = /^#\/\$defs\/([A-Za-z0-9_]+)$/u.exec(ref);
  if (!match) throw new Error(`Unsupported $ref: ${ref}`);
  return pascal(match[1]);
}

function typeOf(node, where) {
  if (node === true || node === undefined) return "unknown";
  if (node.$ref) return refName(node.$ref) === "Page" ? "Page<unknown>" : refName(node.$ref);
  if (node.enum) return node.enum.map(literal).join(" | ");
  if (node.const !== undefined) return literal(node.const);
  if (node.oneOf || node.anyOf) return (node.oneOf ?? node.anyOf).map((entry) => typeOf(entry, where)).join(" | ");
  if (node.allOf) return node.allOf.map((entry) => `(${typeOf(entry, where)})`).join(" & ");
  const types = Array.isArray(node.type) ? node.type : [node.type];
  return [...new Set(types.map((type) => {
    switch (type) {
      case "string": return "string";
      case "integer": case "number": return "number";
      case "boolean": return "boolean";
      case "null": return "null";
      case "array": return node.items ? `Array<${typeOf(node.items, where)}>` : "unknown[]";
      case "object": return objectType(node, where);
      case undefined: return "unknown";
      default: throw new Error(`Unsupported type ${type} at ${where}`);
    }
  }))].join(" | ");
}

function objectType(node, where) {
  const properties = node.properties ?? {};
  const names = Object.keys(properties);
  if (names.length === 0) return "Record<string, unknown>";
  const required = new Set(node.required ?? []);
  const members = names.map((name) => `${comment(properties[name].description, "  ")}  ${literal(name)}${required.has(name) ? "" : "?"}: ${typeOf(properties[name], `${where}/${name}`)};`);
  return `{\n${members.join("\n")}\n}`;
}

function collectEnums(node, pathParts) {
  if (!node || typeof node !== "object") return;
  if (Array.isArray(node.enum)) {
    const name = pathParts.filter((part) => part !== "properties" && part !== "items").map(snake).join("_");
    enumArrays.push({ name: `${name}_VALUES`, values: node.enum, where: `/${pathParts.join("/")}` });
  }
  for (const [key, child] of Object.entries(node)) {
    if (key === "properties") for (const [name, value] of Object.entries(child)) collectEnums(value, [...pathParts, "properties", name]);
    else if (key === "items" || key === "oneOf" || key === "allOf") {
      if (Array.isArray(child)) child.forEach((entry, index) => collectEnums(entry, [...pathParts, key, String(index)]));
      else collectEnums(child, [...pathParts, key]);
    }
  }
}

// A shallow structural guard: required keys present and primitive kinds right. It deliberately accepts unknown
// enum values and extra fields (forward compatibility, 10 §8.1); use the enum guards to detect unknown values.
function primitiveCheck(node) {
  if (!node || node.$ref || node.enum || node.const !== undefined || node.oneOf || node.anyOf || node.allOf) return null;
  const types = Array.isArray(node.type) ? node.type : [node.type];
  const checks = types.map((type) => ({
    string: (v) => `typeof ${v} === "string"`,
    integer: (v) => `Number.isInteger(${v})`,
    number: (v) => `typeof ${v} === "number"`,
    boolean: (v) => `typeof ${v} === "boolean"`,
    null: (v) => `${v} === null`,
    array: (v) => `Array.isArray(${v})`,
    object: (v) => `isRecord(${v})`,
  }[type]));
  return checks.some((check) => check === undefined) ? null : checks;
}

function guardFor(name, node) {
  if (node.type !== "object" && !(Array.isArray(node.type) && node.type.includes("object"))) return null;
  const lines = [`export function is${name}(value: unknown): value is ${name} {`, "  if (!isRecord(value)) return false;"];
  for (const key of node.required ?? []) lines.push(`  if (!(${literal(key)} in value)) return false;`);
  for (const [key, property] of Object.entries(node.properties ?? {})) {
    const checks = primitiveCheck(property);
    if (!checks) continue;
    const access = `value[${literal(key)}]`;
    const presence = (node.required ?? []).includes(key) ? "" : `${access} !== undefined && `;
    lines.push(`  if (${presence}!(${checks.map((check) => check(access)).join(" || ")})) return false;`);
  }
  lines.push("  return true;", "}");
  return lines.join("\n");
}


const BANNER = `// @generated by scripts/gen-contract.mjs from contracts/. Do not edit by hand; run \`node scripts/gen-contract.mjs\`.`;
const defs = schema.$defs;
const out = [];
out.push(`${BANNER}
// Source: docs-api ${contractVersion} (${fingerprints[INPUTS.schema]}).

export const DOCS_API_VERSION = ${literal(contractVersion)};
export const DOCS_API_FINGERPRINTS = ${literal(fingerprints)} as const;

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export type Page<T> = { items: T[]; next_cursor: string | null; as_of_seq: number };
`);

out.push("// ---- enums (top level and nested) ----");
for (const [name, node] of Object.entries(defs)) collectEnums(node, [name]);
for (const { name, values, where } of enumArrays) out.push(`/** ${where} */\nexport const ${name} = ${literal(values)} as const;`);
out.push("");

out.push("// ---- types and structural guards ----");
for (const [name, node] of Object.entries(defs)) {
  if (name === "Page") continue;
  out.push(`${comment(node.description)}export type ${name} = ${typeOf(node, name)};`);
  if (Array.isArray(node.enum)) {
    const array = enumArrays.find((entry) => entry.where === `/${name}`).name;
    out.push(`export function is${name}(value: unknown): value is ${name} {\n  return (${array} as readonly unknown[]).includes(value);\n}`);
  } else {
    const guard = guardFor(name, node);
    if (guard) out.push(guard);
  }
}
out.push("");

out.push("// ---- endpoints (x-endpoints) ----");
out.push(`export type DocsEndpoint = {
  method: "GET" | "POST" | "PATCH" | "PUT" | "DELETE";
  path: string;
  request: string | null;
  response: string | null;
  idempotencyKey: boolean;
  ifMatch: boolean;
  since: string;
};`);
const endpoints = schema["x-endpoints"].map((entry) => ({
  method: entry.method, path: entry.path, request: entry.request, response: entry.response,
  idempotencyKey: entry.idempotency_key === true, ifMatch: entry.if_match === true, since: entry.since,
}));
out.push(`export const DOCS_ENDPOINTS: readonly DocsEndpoint[] = ${JSON.stringify(endpoints, null, 2)};`);
out.push("");

out.push("// ---- error codes (errors.v1.json) ----");
const codes = errors.codes.map((entry) => ({
  code: entry.code,
  http: entry.http,
  stateCode: entry.state_code === true,
  retryable: entry.retryable === true,
  i18nKey: entry.i18n_key,
  detailKeys: entry.details.map((detail) => {
    const match = /^[a-z][a-z0-9_]*/u.exec(detail);
    if (!match) throw new Error(`Cannot read a detail key from ${literal(detail)} of ${entry.code}`);
    return match[0];
  }),
}));
out.push(`export const DOCS_ERROR_CODE_LIST = ${literal(codes.map((entry) => entry.code))} as const;
export type DocsErrorCode = (typeof DOCS_ERROR_CODE_LIST)[number];
export type DocsErrorCodeInfo = {
  code: DocsErrorCode;
  http: number | null;
  stateCode: boolean;
  retryable: boolean;
  i18nKey: string;
  /** Keys of the \`details\` object the engine may send for this code (the pass-through allow-list). */
  detailKeys: readonly string[];
};
export const DOCS_ERROR_CODES: readonly DocsErrorCodeInfo[] = ${JSON.stringify(codes, null, 2)};
export function isDocsErrorCode(value: unknown): value is DocsErrorCode {
  return (DOCS_ERROR_CODE_LIST as readonly unknown[]).includes(value);
}`);
out.push("");

out.push("// ---- events (events.v1.schema.json) ----");
const eventTypes = events.properties.type.enum;
const persistent = events["x-persistent"];
const transient = events["x-transient"];
for (const type of eventTypes) {
  if (!persistent.includes(type) && !transient.includes(type)) problems.push(`Event type ${type} is neither x-persistent nor x-transient`);
}
out.push(`export const DOCS_EVENT_TYPES = ${literal(eventTypes)} as const;
export type DocsEventType = (typeof DOCS_EVENT_TYPES)[number];
export const DOCS_PERSISTENT_EVENT_TYPES = ${literal(persistent)} as const;
export const DOCS_TRANSIENT_EVENT_TYPES = ${literal(transient)} as const;
export function isDocsEventType(value: unknown): value is DocsEventType {
  return (DOCS_EVENT_TYPES as readonly unknown[]).includes(value);
}
export function isPersistentDocsEvent(type: string): boolean {
  return (DOCS_PERSISTENT_EVENT_TYPES as readonly string[]).includes(type);
}
`);
const docsApiTs = `${out.join("\n")}\n`;

// Companion schemas: types only (no enum tables or guards).
function companionTs(relative, rootName) {
  const root = readJson(relative);
  const lines = [`${BANNER}`, `// Source: ${relative} (${fingerprints[relative]}).`, ""];
  for (const [name, node] of Object.entries(root.$defs ?? {})) lines.push(`${comment(node.description)}export type ${pascal(name)} = ${typeOf(node, name)};`);
  lines.push(`${comment(root.description)}export type ${rootName} = ${typeOf({ ...root, $defs: undefined }, rootName)};`, "");
  return lines.join("\n");
}
const docIrTs = companionTs(INPUTS.docIr, "DocIr");
const contextPackTs = companionTs(INPUTS.contextPack, "ContextPack");

const detailKeysByCode = Object.fromEntries(codes.map((entry) => [entry.code, entry.detailKeys]));
const retryableByCode = Object.fromEntries(codes.map((entry) => [entry.code, entry.retryable]));
const mainJs = `// @generated by scripts/gen-contract.mjs from contracts/docs-api (docs-api ${contractVersion}). Do not edit by hand.
export const DOCS_API_VERSION = ${literal(contractVersion)};
/** Allow-list of \`details\` keys per engine error code (errors.v1.json); everything else is dropped before it reaches the renderer. */
export const DOCS_ERROR_DETAIL_KEYS = Object.freeze(${JSON.stringify(detailKeysByCode, null, 2)});
export const DOCS_ERROR_RETRYABLE = Object.freeze(${JSON.stringify(retryableByCode, null, 2)});
export const DOCS_PERSISTENT_EVENT_TYPES = Object.freeze(${literal(persistent)});
export const DOCS_TRANSIENT_EVENT_TYPES = Object.freeze(${literal(transient)});
`;
const mainTypes = `// @generated by scripts/gen-contract.mjs. Do not edit by hand.
export const DOCS_API_VERSION: string;
export const DOCS_ERROR_DETAIL_KEYS: Readonly<Record<string, readonly string[]>>;
export const DOCS_ERROR_RETRYABLE: Readonly<Record<string, boolean>>;
export const DOCS_PERSISTENT_EVENT_TYPES: readonly string[];
export const DOCS_TRANSIENT_EVENT_TYPES: readonly string[];
`;
const outputs = [
  ["docs-contract.generated.mjs", mainJs],
  ["docs-contract.generated.d.mts", mainTypes],
  ["docs-api.generated.ts", docsApiTs],
  ["doc-ir.generated.ts", docIrTs],
  ["context-pack.generated.ts", contextPackTs],
].map(([name, content]) => [path.join(outDirectory, name), content]);

if (problems.length === 0 && mode === "--check") {
  for (const [file, content] of outputs) {
    const relative = path.relative(appRoot, file);
    if (!fs.existsSync(file)) problems.push(`${relative} is missing`);
    else if (fs.readFileSync(file, "utf8") !== content) problems.push(`${relative} is out of date with contracts/; run node scripts/gen-contract.mjs`);
  }
}
if (problems.length > 0) {
  console.error(`contract ${mode === "--check" ? "check" : "generation"} failed:`);
  for (const problem of problems) console.error(`- ${problem}`);
  process.exit(1);
}
if (mode === "--check") {
  console.log(`contract OK: docs-api ${contractVersion}, ${enumArrays.length} enums, ${endpoints.length} endpoints, ${codes.length} error codes, ${eventTypes.length} event types.`);
} else {
  for (const [file, content] of outputs) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
    console.log(`wrote ${path.relative(appRoot, file)}`);
  }
}
