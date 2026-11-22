#!/usr/bin/env node
// Asks the frozen questions of the profile test set through the Super Ontology app's own Ask loop (`runAskLoop`, `createAskTools`, the model client),
// headless, against a running docs engine (no Electron, no GUI): the loop, its prompt, its tools and its citation rules are the app's. The engine of the
// test serves `/v1/docs/*` only; the ontology search of the gateway (`/v1/ontology/search`) is answered here from knowledge search, so the `search` tool
// returns the same blocks `find_text` does and every source is a real file and block of the engine.
//   node ask_headless.mjs --base-url http://127.0.0.1:PORT --token T --questions questions.jsonl --out DIR [--only q01,q02] [--parallel 2]
// The model is the one the engine is configured with (`GET /v1/docs/config/model`); its endpoint and key are read from the `file:` references, in this
// process only, and never printed.
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : fallback; };
const here = path.dirname(fileURLToPath(import.meta.url));
const ask = path.resolve(here, "../../../apps/super-ontology-desktop/electron/main/super-ontology/ask");
const { runAskLoop } = await import(path.join(ask, "ask-loop.mjs"));
const { createAskTools, createSourceRegistry } = await import(path.join(ask, "ask-tools.mjs"));
const { createModelClient, readKeyRef } = await import(path.join(ask, "model-client.mjs"));

const base = opt("--base-url");
const token = opt("--token");
const headers = { Authorization: `Bearer ${token}`, Accept: "application/json", "OO-Docs-Api-Min": "1.0" };
const get = async (route, signal) => {
  const response = await fetch(`${base}${route}`, { headers, signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(25_000)]) : AbortSignal.timeout(25_000) });
  const text = await response.text();
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { json = null; }
  return { status: response.status, json };
};

/** `/v1/ontology/search` of the gateway, answered from knowledge search. */
async function ontologySearch(route, signal) {
  const q = new URL(route, "http://x").searchParams;
  const res = await get(`/v1/docs/search?q=${encodeURIComponent(q.get("q") ?? "")}&limit=${q.get("limit") ?? 8}`, signal);
  if (res.status !== 200) return res;
  const items = (res.json.items ?? []).map((hit) => {
    const e = hit.evidence?.[0] ?? {};
    return { side: "document", type: hit.kind, name: hit.subject?.name ?? (hit.hpath?.at?.(-1) ?? ""), urn: `oo:doc/${hit.id}`, text: hit.text,
      provenance: e.path ? { kind: "file", file_id: e.file_id, drive_id: e.drive_id, path: e.path, anchor: e.anchor, block_id: e.block_id } : null };
  });
  return { status: 200, json: { items, warnings: [] } };
}

const http = async (method, route, body, signal) => {
  if (route.startsWith("/v1/ontology/search")) return ontologySearch(route, signal);
  if (route.startsWith("/v1/ontology/")) return { status: 404, json: { error: { code: "NOT_SERVED", message: "no gateway in this harness" } } };
  return get(route, signal);
};
const control = async (name, options = {}) => {
  const query = new URLSearchParams(options.query ?? {}).toString();
  const route = { constraintsList: "/v1/docs/constraints", entitiesList: "/v1/docs/entities", entityGet: `/v1/docs/entities/${encodeURIComponent(options.params?.eid ?? "")}` }[name];
  const res = await get(`${route}${query ? `?${query}` : ""}`);
  if (res.status !== 200) throw new Error(`control ${name} ${res.status}`);
  return { value: res.json };
};

let config = (await get("/v1/docs/config/model")).json;
if (!config?.configured) { console.error("the engine has no model configured"); process.exit(3); }
const key = await readKeyRef(config.key_ref);
if (typeof config.base_url === "string" && config.base_url.startsWith("file:")) config = { ...config, base_url: await readKeyRef(config.base_url) };
const model = createModelClient({});
const bound = { chat: ({ system, messages, tools, signal }) => model.chat({ config, key, system, messages, tools, signal }) };

const only = new Set((opt("--only", "") || "").split(",").filter(Boolean));
const questions = (await fs.readFile(opt("--questions"), "utf8")).split("\n").filter(Boolean).map((l) => JSON.parse(l)).filter((q) => only.size === 0 || only.has(q.id));
const queue = [...questions];
const results = [];
const worker = async () => {
  for (let q = queue.shift(); q; q = queue.shift()) {
    const t = Date.now();
    const registry = createSourceRegistry();
    const tools = createAskTools({ http, registry, control });
    let v;
    try {
      v = await runAskLoop({ question: q.question, model: bound, tools, registry, signal: AbortSignal.timeout(240_000) });
    } catch (error) { v = { status: `error:${String(error?.message ?? error).slice(0, 80)}` }; }
    results.push({ id: q.id, ms: Date.now() - t, ok: v.status === "answered" || v.status === "unanswerable", status: v.status, answer: v.answer ?? "", reason: v.reason ?? "",
      sources: (v.sources ?? []).map((x) => ({ kind: x.kind, path: x.path, anchor: x.anchor })), steps: (v.steps ?? []).map((s) => `${s.tool}: ${s.arg}`), uncited: v.uncited, budget: v.budget });
    console.error(`ask ${q.id} ${v.status} ${Date.now() - t}ms`);
  }
};
await Promise.all(Array.from({ length: Number(opt("--parallel", "2")) }, worker));
results.sort((a, b) => a.id.localeCompare(b.id));
await fs.mkdir(opt("--out"), { recursive: true });
await fs.writeFile(path.join(opt("--out"), "ask-results.json"), JSON.stringify(results, null, 1));
console.error(`asked ${results.length}`);
