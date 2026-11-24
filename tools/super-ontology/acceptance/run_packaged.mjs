#!/usr/bin/env node
// Drives the PACKAGED Super Ontology app through its own IPC (the same channels the renderer uses) over the DevTools protocol:
// launch with an isolated HOME, start the engine, ensure My files, optionally set the model, upload the acceptance folder (a drop
// of the folder), and record per file the state timeline until every file is settled. Writes <out>/packaged-run.json.
//
//   node run_packaged.mjs --app "<.../Super Ontology.app>" --project <dir> --home <isolated HOME> --out <results dir>
//        [--model] [--export-token F] [--hold] [--runtime auto|lite|compose] [--timeout-min 40] [--language en] [--keep]
// --model sets the model from the key files named by --model-endpoint / --model-key: `oo docs model set --base-url file:<path> --key-ref file:<path>`,
// so the values are never read, printed or logged here; they are read by the engine side. --ask <questions.jsonl> asks every question through the
// --export-token <file> writes a read-only Agent token (0600) for the scorer; --hold keeps the app running afterwards so ask_questions.mjs and
// scrape_unlinked.mjs can attach to it (SIGTERM ends it).
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : fallback; };
const flag = (name) => args.includes(name);
const execFileP = promisify(execFile);
const sleep = (ms) => new Promise((r) => { setTimeout(r, ms); });
const appPath = opt("--app");
const project = opt("--project");
const home = opt("--home");
const out = opt("--out");
if (!appPath || !project || !home || !out) { console.error("usage: --app --project --home --out [--model] [--export-token F] [--hold] [--runtime] [--timeout-min]"); process.exit(2); }
const HERE = path.dirname(new URL(import.meta.url).pathname);
const harness = path.resolve(HERE, "../../../apps/super-ontology-desktop/tests/e2e/harness/cdp.mjs");
const { connectToPage, readDevToolsPort } = await import(harness);
const log = (...a) => console.error(`[${((Date.now() - T0) / 1000).toFixed(1)}s]`, ...a);
const T0 = Date.now();

await fs.mkdir(out, { recursive: true });
const profile = path.join(home, "profile");
await fs.mkdir(profile, { recursive: true });
await fs.rm(path.join(profile, "DevToolsActivePort"), { force: true });
await fs.writeFile(path.join(profile, "settings.json"), JSON.stringify({ language: opt("--language", "en"), runtime: opt("--runtime", "auto") }), { flag: "wx", mode: 0o600 }).catch(() => undefined);
// The engine reads <state>/engine/docs.json at start. A reasoning model needs minutes per extraction call, and the default of 2 parallel extractions
// would take an hour for 45 files, so a run may ask for --extract-parallel like an operator would; without it the engine's own default (8 since lane AD) applies.
if (flag("--model") && opt("--extract-parallel")) {
  const engineDir = path.join(profile, "engine", "state", "engine");
  await fs.mkdir(engineDir, { recursive: true, mode: 0o700 });
  await fs.writeFile(path.join(engineDir, "docs.json"), JSON.stringify({ version: "oo.docs/v1", concurrency: { extract_llm: Number(opt("--extract-parallel")) } }), { mode: 0o600 });
}
const exe = path.join(appPath, "Contents/MacOS/Super Ontology");
const env = { ...process.env, HOME: home, SUPER_ONTOLOGY_USER_DATA: profile, DOCKER_CONFIG: path.join(os.homedir(), ".docker"), OO_DOCS_LANE: process.env.OO_DOCS_LANE || "super-ontology-ab" };
const child = spawn(exe, [`--user-data-dir=${profile}`, "--remote-debugging-port=0"], { env, stdio: ["ignore", "pipe", "pipe"] });
const mainLog = [];
child.stdout.on("data", (c) => mainLog.push(String(c)));
child.stderr.on("data", (c) => mainLog.push(String(c)));
const finish = async (code) => {
  await fs.writeFile(path.join(out, "main-process.log"), mainLog.join("")).catch(() => undefined);
  if (!flag("--keep")) { child.kill("SIGTERM"); await new Promise((r) => { child.once("exit", r); setTimeout(r, 30_000); }); }
  process.exit(code);
};
process.on("SIGINT", () => finish(130));
const cdp = await connectToPage(await readDevToolsPort(profile), { urlPattern: /^app:/u });
const call = (channel, request = {}) => cdp.evaluate(`window.superOntology.invoke(${JSON.stringify(channel)}, ${JSON.stringify(request)})`);
const run = { startedAt: new Date().toISOString(), app: appPath, files: {}, events: [] };
const note = (what, extra = {}) => { run.events.push({ t: Date.now() - T0, what, ...extra }); log(what, JSON.stringify(extra).slice(0, 300)); };

let snap = await call("engine-read");
note("engine-read", { status: snap?.value?.status ?? snap?.status });
if (!["healthy", "running", "ready"].includes(snap?.value?.status)) {
  snap = await call("engine-start", {});
  note("engine-start", { ok: snap.ok, status: snap?.value?.status, error: snap?.error?.code });
}
for (let i = 0; i < 360; i += 1) {
  snap = await call("engine-read");
  const status = snap?.value?.status;
  if (["healthy", "running", "ready"].includes(status)) break;
  if (["failed", "error"].includes(status)) { note("engine-failed", snap); await finish(3); }
  await sleep(1000);
}
run.engine = snap?.value ?? snap;
note("engine-up", { mode: run.engine?.mode ?? run.engine?.runtime, runtime: run.engine?.runtimeChoice });
const my = await call("my-files-ensure", { operationId: crypto.randomUUID() });
const drive = my.value;
note("my-files", { ok: my.ok, id: drive?.id, error: my.error?.code });
run.driveId = drive?.id;
// Since lane AH the app shares the CLI's engine: the state folder is the CLI's own (under HOME), not <profile>/engine/state.
const stateDir = opt("--state-dir") ?? path.join(home, ".local", "state", "open-ontology", "docs");
const ooBin = path.join(appPath, "Contents/Resources/oo/oo");
const oo = async (...a) => {
  try { const r = await execFileP(ooBin, [...a, "--state-dir", stateDir, "--json"], { env: { ...env }, timeout: 120_000, maxBuffer: 1 << 24 }); return { ok: true, out: r.stdout }; }
  catch (e) { return { ok: false, out: String(e.stdout ?? ""), err: String(e.stderr ?? "").slice(0, 300) }; }
};
if (flag("--model")) {
  const set = await oo("docs", "model", "set", "--provider", "openai-compatible", "--base-url", `file:${opt("--model-endpoint")}`, "--model", opt("--model-name", "gcli/grok-4.7"), "--key-ref", `file:${opt("--model-key")}`);
  note("model-set", { ok: set.ok, err: set.err });
  const on = await oo("docs", "drive", "set", drive.id, "--enable-model");
  note("drive-enable-model", { ok: on.ok, err: on.err });
}
const t0 = Date.now();
const started = await call("upload-start", { driveId: drive.id, folder: "", paths: [project] });
note("upload-start", { ok: started.ok, items: started.value?.items?.length, skipped: started.value?.skipped, error: started.error?.code });
if (!started.ok) await finish(4);
// `upload-start` answers at once (lane AI) and the walk reports through `upload-progress`: the number of files to wait for is what the folder holds.
const walk = async (dir) => (await Promise.all((await fs.readdir(dir, { withFileTypes: true })).map((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)])))).flat();
run.uploadItems = started.value.items ? started.value.items.map((i) => i.relative) : (await walk(project)).map((f) => path.relative(path.dirname(project), f));
const deadline = Date.now() + Number(opt("--timeout-min", "40")) * 60_000;
const TERMINAL = new Set(["published", "unsupported", "parse_failed", "extract_failed", "encrypted", "too_large", "duplicate", "failed", "quarantined", "extract_partial"]);
let settled = false;
while (Date.now() < deadline) {
  const page = await call("files-list", { driveId: drive.id, recursive: true, limit: 500 });
  const items = page?.value?.items ?? [];
  for (const f of items) {
    const key = f.path;
    const rec = (run.files[key] ??= { id: f.id, timeline: [] });
    const last = rec.timeline.at(-1);
    if (!last || last.state !== f.state) rec.timeline.push({ state: f.state, ms: Date.now() - t0 });
    rec.state = f.state; rec.detail = f;
  }
  const states = items.map((f) => f.state);
  const open = states.filter((s) => !TERMINAL.has(s)).length;
  log(`files=${items.length} open=${open}`, [...new Set(states)].join(","));
  if (items.length >= run.uploadItems.length && open === 0) { settled = true; break; }
  await sleep(2000);
}
run.settled = settled;
run.uploadMs = Date.now() - t0;
await fs.writeFile(path.join(out, "packaged-run.json"), JSON.stringify(run, null, 1));
note("done", { settled, files: Object.keys(run.files).length });
if (opt("--export-token")) {
  await fs.rm(opt("--export-token"), { force: true });
  const made = await oo("agent", "token", "create", "--name", "ab-score", "--scope", "docs:read", "--all-drives", "--ttl", "6h", "--output-file", opt("--export-token"));
  note("token-export", { ok: made.ok, err: made.err });
}
if (flag("--hold")) { note("holding"); await new Promise((r) => { process.on("SIGTERM", r); process.on("SIGUSR1", r); }); }
await finish(settled ? 0 : 5);
