// Puts the `oo` binary into build/oo-bundle/ (it ends up as Resources/oo/oo). The binary is an input of the build: a path given with
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
fs.writeFileSync(path.join(directory, "oo-bundle.json"), `${JSON.stringify({ version, sha256, bytes: bytes.length }, null, 2)}\n`);
console.log(`oo ${version} bundled (sha256 ${sha256.slice(0, 12)}...)`);
