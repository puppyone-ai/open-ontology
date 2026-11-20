#!/usr/bin/env node
// Asks every question of gold/questions.jsonl through the packaged app's own Ask channel (`ask-run`, the one the Ontology > Ask screen uses) on a
// running app (run_packaged.mjs --hold), 2 at a time, and writes ask-results.json for score_ask.py.
//   node ask_questions.mjs --profile <HOME>/profile --questions <questions.jsonl> --out <dir> [--only q01,q02] [--parallel 2]
import fs from "node:fs/promises";
import path from "node:path";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : fallback; };
const here = path.dirname(new URL(import.meta.url).pathname);
const { connectToPage, readDevToolsPort } = await import(path.resolve(here, "../../../apps/super-ontology-desktop/tests/e2e/harness/cdp.mjs"));
const cdp = await connectToPage(await readDevToolsPort(opt("--profile")), { urlPattern: /^app:/u });
const call = (channel, request = {}) => cdp.evaluate(`window.superOntology.invoke(${JSON.stringify(channel)}, ${JSON.stringify(request)})`);
const only = new Set((opt("--only", "") || "").split(",").filter(Boolean));
const questions = (await fs.readFile(opt("--questions"), "utf8")).split("\n").filter(Boolean).map((l) => JSON.parse(l)).filter((q) => only.size === 0 || only.has(q.id));
const queue = [...questions];
const results = [];
const worker = async () => {
  for (let q = queue.shift(); q; q = queue.shift()) {
    const t = Date.now();
    let r;
    try { r = await call("ask-run", { askId: `ab-ask-${q.id}`, question: q.question }); } catch (e) { r = { ok: false, error: { code: String(e.message).slice(0, 80) } }; }
    const v = r.value ?? {};
    results.push({ id: q.id, ms: Date.now() - t, ok: r.ok, status: v.status ?? r.error?.code, answer: v.answer ?? "", reason: v.reason ?? "", sources: (v.sources ?? []).map((x) => ({ kind: x.kind, path: x.path, anchor: x.anchor })), steps: (v.steps ?? []).length, uncited: v.uncited, budget: v.budget });
    console.error(`ask ${q.id} ${v.status ?? r.error?.code} ${Date.now() - t}ms`);
  }
};
await Promise.all(Array.from({ length: Number(opt("--parallel", "2")) }, worker));
results.sort((a, b) => a.id.localeCompare(b.id));
await fs.mkdir(opt("--out"), { recursive: true });
await fs.writeFile(path.join(opt("--out"), "ask-results.json"), JSON.stringify(results, null, 1));
console.error(`asked ${results.length}`);
cdp.close();
