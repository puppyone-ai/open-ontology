// Gate: no foreign product name and no private term in the app's files. The attribution in NOTICE is the only place the name of
// the source project may appear. Prints file names and counts only, never a matched term.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(appRoot, "..", "..");
const SOURCE_NAME = new RegExp(["pup", "pyone"].join(""), "iu");
const ALLOWED_NAME_FILES = new Set(["NOTICE"]);
const SKIP_DIRECTORIES = new Set(["node_modules", "dist", "release", ".git", "oo-bundle", "generated", "icon.iconset"]);
const BINARY = /\.(icns|png|jpg|jpeg|webp|ico|woff2?|ttf|dmg|zip)$/iu;

const extra = process.argv.slice(2).map((entry) => path.resolve(repoRoot, entry));
const roots = [appRoot, path.join(repoRoot, "packages", "design-system"), path.join(repoRoot, ".github", "workflows", "super-ontology-desktop.yml"), ...extra]
  .filter((entry, index, all) => fs.existsSync(entry) && all.indexOf(entry) === index);

let privateTerms = [];
const termsFile = path.join(os.homedir(), ".config", "open-ontology", "private-terms.txt");
if (fs.existsSync(termsFile)) {
  privateTerms = fs.readFileSync(termsFile, "utf8").split(/\r?\n/u).map((line) => line.trim()).filter(Boolean).map((line) => line.toLowerCase());
}

function* files(entry) {
  const stat = fs.statSync(entry);
  if (stat.isFile()) { yield entry; return; }
  for (const item of fs.readdirSync(entry, { withFileTypes: true })) {
    if (item.isDirectory()) { if (!SKIP_DIRECTORIES.has(item.name)) yield* files(path.join(entry, item.name)); } else if (item.isFile()) yield path.join(entry, item.name);
  }
}

const failures = [];
let scanned = 0;
for (const root of roots) {
  for (const file of files(root)) {
    if (BINARY.test(file)) continue;
    scanned += 1;
    const relative = path.relative(repoRoot, file);
    const text = fs.readFileSync(file, "utf8");
    const lower = text.toLowerCase();
    if (!ALLOWED_NAME_FILES.has(path.basename(file)) && SOURCE_NAME.test(`${text}\n${relative}`)) failures.push(`${relative}: foreign product name`);
    const hits = privateTerms.filter((term) => lower.includes(term) || relative.toLowerCase().includes(term)).length;
    if (hits > 0) failures.push(`${relative}: private term (${hits})`);
  }
}
if (failures.length > 0) { console.error(`brand gate failed (${failures.length}):\n${failures.join("\n")}`); process.exit(1); }
console.log(`brand gate ok: ${scanned} files, ${privateTerms.length > 0 ? "private terms checked" : "private-term list not present"}`);
