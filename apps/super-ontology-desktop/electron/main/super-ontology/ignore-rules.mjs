// `.gitignore` and `.ooignore` rules and the code-project default skips of a folder the person drops, links or watches.
// The Rust fs walk implements the same semantics (crates/oo-connectors/src/objectstore/path/ignore.rs); both are tested against
// crates/oo-connectors/tests/fixtures/ignore-semantics.json, so a folder copied, linked and watched is read the same way.
//
// Semantics (a subset of gitignore): a rules file is `.gitignore` or `.ooignore` inside a folder; its patterns are relative to that folder and
// reach everything below it. A deeper folder's rules beat a shallower one's; inside one folder `.ooignore` beats `.gitignore`; inside a file
// the last matching line wins. `#` comments, blank lines, `\#` and `\!` literals, trailing spaces dropped unless escaped, `!` re-includes
// (never below an ignored folder, which is not visited), a trailing `/` matches folders only, a `/` inside a pattern anchors it to its folder,
// otherwise a name matches at any depth. `*` `?` `[a-z]` `[!a-z]` `[^a-z]` `\x`, and `**` as a whole segment (leading, middle, trailing).
// A rules file over 1 MiB, or a rule over 32 segments or with more than 4 `**`, is ignored.
import fsSync from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";

export const MAX_RULES_BYTES = 1024 * 1024;
export const MAX_SEGMENTS = 32;
export const MAX_DOUBLE_STARS = 4;
/** Read in this order inside one folder (the later file wins). */
export const RULE_FILES = Object.freeze([[".gitignore", "gitignore"], [".ooignore", "ooignore"]]);

/** Dependency and build folders of code projects (folders only: a file called `build` is a file). */
export const PROJECT_DIRS = Object.freeze(["target", "dist", "build", "venv"]);
const TEMPORARY_NAME = /^~\$|\.(part|tmp|swp|crdownload)$/iu;
const JUNK_NAME = /^(thumbs\.db|desktop\.ini|node_modules|__pycache__)$/iu;
const COMPILED_NAME = /\.(pyc|pyo)$/iu;

/**
 * Why the engine ignores a name by itself (10 §4.3.2 plus the code project defaults), or null: "hidden" (a leading dot: .git, .venv, .next,
 * .cache, .idea, .vscode, .DS_Store), "temporary" (Office locks, partial downloads, editor swap files) or "junk" (OS metadata, node_modules,
 * __pycache__, target, dist, build, venv as folders, compiled .pyc/.pyo files). The Rust `skip_reason_fs` is the same table.
 */
export function ignoreReason(name, isDir = false) {
  if (name.startsWith(".")) return "hidden";
  if (TEMPORARY_NAME.test(name)) return "temporary";
  if (JUNK_NAME.test(name)) return "junk";
  if (isDir ? PROJECT_DIRS.includes(name) : COMPILED_NAME.test(name)) return "junk";
  return null;
}

export const isIgnoredName = (name, isDir = false) => ignoreReason(name, isDir) !== null;

function parseLine(raw, source) {
  let line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
  while (line.endsWith(" ") && !line.endsWith("\\ ")) line = line.slice(0, -1);
  if (line === "" || line.startsWith("#")) return null;
  let negated = false;
  let body = line;
  if (body.startsWith("!")) { negated = true; body = body.slice(1); }
  if (body.startsWith("\\#") || body.startsWith("\\!")) body = body.slice(1);
  const dirOnly = body.length > 1 && body.endsWith("/");
  if (dirOnly) body = body.slice(0, -1);
  if (body === "" || body === "/") return null;
  const anchored = body.includes("/");
  if (body.startsWith("/")) body = body.slice(1);
  const segs = [];
  if (!anchored) segs.push(null);
  for (const part of body.split("/")) {
    if (part === "") continue;
    if (part === "**") { if (segs[segs.length - 1] !== null) segs.push(null); } else segs.push(Array.from(part));
  }
  if (segs.length === 0 || segs.length > MAX_SEGMENTS || segs.filter((seg) => seg === null).length > MAX_DOUBLE_STARS) return null;
  return { segs, negated, dirOnly, source };
}

/** The rules of one file's text; `source` is "gitignore" or "ooignore". */
export function parseRules(text, source) {
  const rules = [];
  for (const line of text.split("\n")) {
    const rule = parseLine(line, source);
    if (rule) rules.push(rule);
  }
  return rules;
}

function classMatch(pattern, start, c) {
  let i = start + 1;
  const negate = pattern[i] === "!" || pattern[i] === "^";
  if (negate) i += 1;
  let hit = false;
  let first = true;
  for (;;) {
    const item = pattern[i];
    if (item === undefined) return -1;
    if (item === "]" && !first) return hit !== negate ? i + 1 : -1;
    first = false;
    let low = item;
    if (item === "\\") { i += 1; low = pattern[i]; if (low === undefined) return -1; }
    if (pattern[i + 1] === "-" && pattern[i + 2] !== undefined && pattern[i + 2] !== "]") {
      hit ||= low <= c && c <= pattern[i + 2];
      i += 3;
    } else {
      hit ||= low === c;
      i += 1;
    }
  }
}

/** The index in `pattern` after matching `c` at `p`, or -1. */
function single(pattern, p, c) {
  const head = pattern[p];
  if (head === undefined || head === "*") return -1;
  if (head === "?") return p + 1;
  if (head === "\\") return pattern[p + 1] === c ? p + 2 : -1;
  if (head === "[") return classMatch(pattern, p, c);
  return head === c ? p + 1 : -1;
}

function globName(pattern, name) {
  const text = Array.from(name);
  let p = 0; let t = 0; let star = null;
  while (t < text.length) {
    if (p < pattern.length && pattern[p] === "*") { star = [p + 1, t]; p += 1; continue; }
    const next = single(pattern, p, text[t]);
    if (next >= 0) { p = next; t += 1; continue; }
    if (star === null) return false;
    star = [star[0], star[1] + 1];
    p = star[0];
    t = star[1];
  }
  while (p < pattern.length && pattern[p] === "*") p += 1;
  return p === pattern.length;
}

function matchSegs(pattern, pi, parts, ti) {
  if (pi === pattern.length) return ti === parts.length;
  const seg = pattern[pi];
  if (seg === null) {
    if (pi === pattern.length - 1) return ti < parts.length;
    for (let skip = ti; skip <= parts.length; skip += 1) if (matchSegs(pattern, pi + 1, parts, skip)) return true;
    return false;
  }
  return ti < parts.length && globName(seg, parts[ti]) && matchSegs(pattern, pi + 1, parts, ti + 1);
}

/**
 * The rule sets of the folders above the entry being looked at, shallowest first. Immutable: `with` returns a new stack.
 * `ignored(rel, isDir)` takes the entry's path from the root as segments (the entry last) and answers the deciding rule's source
 * ("gitignore" or "ooignore") or null.
 */
export function createIgnoreStack(layers = []) {
  return Object.freeze({
    isEmpty: layers.length === 0,
    with(depth, rules) { return rules.length === 0 ? this : createIgnoreStack([...layers, { depth, rules }]); },
    ignored(rel, isDir) {
      let state = null;
      for (const { depth, rules } of layers) {
        if (depth >= rel.length) continue;
        const below = rel.slice(depth);
        for (let i = rules.length - 1; i >= 0; i -= 1) {
          const rule = rules[i];
          if ((!rule.dirOnly || isDir) && matchSegs(rule.segs, 0, below, 0)) { state = { ignored: !rule.negated, source: rule.source }; break; }
        }
      }
      return state?.ignored ? state.source : null;
    },
  });
}

/** Reads `.gitignore` then `.ooignore` of a folder. Absent, unreadable, oversized or not regular files add nothing. */
export async function loadDirRules(directory, fsPromises = fs) {
  const rules = [];
  for (const [name, source] of RULE_FILES) {
    const file = path.join(directory, name);
    try {
      const stat = await fsPromises.lstat(file);
      if (!stat.isFile() || stat.size > MAX_RULES_BYTES) continue;
      rules.push(...parseRules(await fsPromises.readFile(file, "utf8"), source));
    } catch { /* No such file, or not readable. */ }
  }
  return rules;
}

/** Decides one path against an in-memory tree of rules files (`{"sub/.gitignore": "text"}`), ancestors first: how a walk would. For tests. */
export function decideTree(files, relativePath, isDir) {
  const parts = relativePath.split("/");
  let stack = createIgnoreStack();
  for (let depth = 0; depth < parts.length; depth += 1) {
    const directory = parts.slice(0, depth).join("/");
    const rules = [];
    for (const [name, source] of RULE_FILES) {
      const text = files[directory === "" ? name : `${directory}/${name}`];
      if (typeof text === "string") rules.push(...parseRules(text, source));
    }
    stack = stack.with(depth, rules);
    const last = depth + 1 === parts.length;
    const source = stack.ignored(parts.slice(0, depth + 1), !last || isDir);
    if (source) return source;
  }
  return null;
}

/** {@link loadDirRules} for a caller that cannot wait (a file system event handler): same files, same limits. */
export function loadDirRulesSync(directory, fileSystem = fsSync) {
  const rules = [];
  for (const [name, source] of RULE_FILES) {
    const file = path.join(directory, name);
    try {
      const stat = fileSystem.lstatSync(file);
      if (!stat.isFile() || stat.size > MAX_RULES_BYTES) continue;
      rules.push(...parseRules(fileSystem.readFileSync(file, "utf8"), source));
    } catch { /* No such file, or not readable. */ }
  }
  return rules;
}

/**
 * A resolver for the watcher: is a path (relative to the watched root, '/' separated) ignored by the default skips or by rules files found on
 * disk? Rules per folder are cached (the files are tiny and read once); a change to a rules file (`invalidate`) drops the cache. The last name is
 * taken as a file unless `isDir`. Synchronous, so an event handler stays one.
 */
export function createPathIgnoreResolver(root, { fileSystem = fsSync } = {}) {
  const cache = new Map();
  const rulesOf = (relativeDirectory) => {
    let rules = cache.get(relativeDirectory);
    if (!rules) { rules = loadDirRulesSync(path.join(root, ...relativeDirectory.split("/").filter(Boolean)), fileSystem); cache.set(relativeDirectory, rules); }
    return rules;
  };
  return Object.freeze({
    invalidate() { cache.clear(); },
    isIgnored(relativePath, isDir = false) {
      const parts = relativePath.split("/");
      let stack = createIgnoreStack();
      for (let depth = 0; depth < parts.length; depth += 1) {
        const last = depth + 1 === parts.length;
        if (ignoreReason(parts[depth], !last || isDir)) return true;
        stack = stack.with(depth, rulesOf(parts.slice(0, depth).join("/")));
        if (stack.ignored(parts.slice(0, depth + 1), !last || isDir)) return true;
      }
      return false;
    },
  });
}
