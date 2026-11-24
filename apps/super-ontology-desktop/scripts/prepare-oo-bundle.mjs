// Puts the `oo` binary and every helper it starts from beside itself (oo-docs-engine, oo-mysql-doctor, oo-load, oo-gateway, mcp-gateway) into build/oo-bundle/ (it ends up as Resources/oo/oo). The helpers come from the directory of oo (tools/docs-release/build_docs_distribution.sh) and must match its HELPERS.json: one missing, or not the recorded build, fails the bundle. The binary is an input of the build: a path given with
// --binary or OO_BINARY (CI downloads the arm64 artifact of the cli-binaries workflow and passes --sha256 from its checksum file).
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const source = option("--binary") ?? process.env.OO_BINARY;
const expected = option("--sha256") ?? process.env.OO_BINARY_SHA256;
if (!source || !path.isAbsolute(source) || !fs.existsSync(source)) {
  console.error("Give the oo binary as an absolute path: --binary <path> or OO_BINARY.");
  process.exit(2);
}
const HELPERS = ["oo-docs-engine", "oo-mysql-doctor", "oo-load", "oo-gateway", "mcp-gateway"];
const sourceDir = path.dirname(source);
const sha = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const fail = (message) => { console.error(message); process.exit(2); };
const manifestFile = path.join(sourceDir, "HELPERS.json");
if (!fs.existsSync(manifestFile)) fail("HELPERS.json was not found next to oo: build the set with tools/docs-release/build_docs_distribution.sh.");
const recorded = JSON.parse(fs.readFileSync(manifestFile, "utf8"));
if (recorded.files?.[path.basename(source)] !== sha(source)) fail("oo is not the build that HELPERS.json records.");
const helperSources = new Map();
for (const name of HELPERS) {
  const file = name === "oo-docs-engine" && (option("--engine") ?? process.env.OO_DOCS_ENGINE_BINARY) ? (option("--engine") ?? process.env.OO_DOCS_ENGINE_BINARY) : path.join(sourceDir, name);
  if (!fs.existsSync(file)) fail(`Helper ${name} is missing next to oo.`);
  if (recorded.files?.[name] !== sha(file)) fail(`Helper ${name} is not from the build HELPERS.json records (another build).`);
  helperSources.set(name, file);
}
const bytes = fs.readFileSync(source);
const sha256 = crypto.createHash("sha256").update(bytes).digest("hex");
if (expected && expected.toLowerCase() !== sha256) { console.error("sha256 of the oo binary does not match the expected value."); process.exit(3); }
if (process.platform === "darwin") {
  const archs = execFileSync("lipo", ["-archs", source], { encoding: "utf8" }).trim().split(/\s+/u);
  if (!archs.includes("arm64")) { console.error(`oo has no arm64 slice (found: ${archs.join(", ")}).`); process.exit(4); }
}
fs.chmodSync(source, 0o755);
const version = JSON.parse(execFileSync(source, ["--version", "--json"], { encoding: "utf8", timeout: 10_000 })).version;
const directory = path.join(root, "build", "oo-bundle");
fs.rmSync(directory, { recursive: true, force: true });
fs.mkdirSync(directory, { recursive: true });
fs.writeFileSync(path.join(directory, "oo"), bytes, { mode: 0o755 });
const helperHashes = {};
for (const [name, file] of helperSources) {
  if (process.platform === "darwin" && !execFileSync("lipo", ["-archs", file], { encoding: "utf8" }).trim().split(/\s+/u).includes("arm64")) fail(`${name} has no arm64 slice.`);
  fs.writeFileSync(path.join(directory, name), fs.readFileSync(file), { mode: 0o755 });
  helperHashes[name] = sha(file);
}
const engineSha256 = helperHashes["oo-docs-engine"];
fs.writeFileSync(path.join(directory, "HELPERS.json"), fs.readFileSync(manifestFile));
fs.writeFileSync(path.join(directory, "oo-bundle.json"), `${JSON.stringify({ version, sha256, bytes: bytes.length, engineSha256, commit: recorded.commit, helpers: helperHashes }, null, 2)}\n`);
console.log(`oo ${version} bundled (sha256 ${sha256.slice(0, 12)}...)`);
