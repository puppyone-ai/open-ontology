#!/usr/bin/env node
// A stand-in `oo` for the CLI parity tests: it answers the commands the app runs with canned JSON and records its argument vectors
// (never stdin) in $FAKE_OO_LOG, so a test can see exactly what the app asked for. Synthetic data only.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const log = process.env.FAKE_OO_LOG;
const stdinText = () => { try { return fs.readFileSync(0, "utf8"); } catch { return ""; } };
const read = args.includes("--password-stdin") ? stdinText() : "";
if (log) fs.appendFileSync(log, `${JSON.stringify({ args, stdinLength: read.length, stdinHasPassword: read.includes("fake-pw") })}\n`);
const out = (value, code = 0) => { process.stdout.write(`${JSON.stringify(value)}\n`); process.exit(code); };
const [a, b] = args;
if (a === "--version") out({ name: "oo", version: "0.2.1" });
if (a === "init" && args.includes("--engine")) out({ protocol: "oo.init-connect/v1", signed_in: !process.env.FAKE_OO_BAD_LOGIN, written: false, checks: [{ name: "resolve", status: "ok", detail: "127.0.0.1" }, { name: "port", status: "ok", detail: "2 ms" }, { name: "tls", status: "ok", detail: "localhost" }, { name: "sign_in", status: process.env.FAKE_OO_BAD_LOGIN ? "blocked" : "ok", detail: "ok" }] });
if (a === "init") out({ command: "init", normalized: true, status: "validated", protocol: "oo.admin/v1" });
if (a === "source" && b === "add") out({ name: args[args.indexOf("--name") + 1], backend: args[2], kind: "database", status: "configured" });
if (a === "source" && b === "ls") out({ protocol: "oo.sources/v1", engine_running: false, sources: [{ name: "shop", backend: "postgres", kind: "database", status: "configured", config: { host: "db.example.test", port: 5432, database: "shop", user: "ro", secret_ref: "(set)", tls_ca: true, include: ["public.customers"], exclude: [] } }] });
if (a === "source" && b === "test") out({ ok: true, reachable: true, name: args[2] });
if (a === "source" && b === "rm") out({ name: args[2], removed: true });
if (a === "doctor") out({ protocol: "oo.doctor/v1", status: "ok", model: { status: "ok" }, sources: [{ source_id: "shop", status: "ok" }] });
if (a === "status") out({ protocol: "oo.build-progress/v1", status: "complete", data_published_at: "2026-11-19T10:00:00Z", tables: { stale: [], review_pending: [] } });

// `oo build` in the shapes the app reads: stops for the owner's review (FAKE_OO_REVIEW=1, from a recorded real Builder draft), continues
// with `--resume --review-file`, or names a cause when a stage is blocked (FAKE_OO_BUILD_FAIL="CODE: cause").
const STAGES = ["load", "candidate", "builder", "self_review", "publish", "gateway", "first_token", "share_prompt"];
const chain = (status, states) => ({ protocol: "oo.build-chain/v1", status, publication: status === "complete" ? "published" : "not_published", gateway_health: status === "complete" ? "verified" : "unmeasured", model_calls: 5, stages: STAGES.map((name, index) => ({ name, status: states[index]?.[0] ?? "pending", attempts: states[index] ? 1 : 0, completed: states[index]?.[0] === "complete" ? 1 : 0, total: 1, error: states[index]?.[1] ?? null })), ...(status === "complete" ? { profile: "shop" } : {}) });
if (a === "build" && (process.env.FAKE_OO_REVIEW || process.env.FAKE_OO_BUILD_FAIL)) {
  const config = args[args.indexOf("--config") + 1];
  const state = path.join(path.dirname(config), ".oo");
  const chainFile = path.join(state, "lite", "work", "build", ".oo", "chain.json");
  const write = (file, text) => { fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 }); fs.writeFileSync(file, text, { mode: 0o600 }); };
  const recorded = path.join(path.dirname(fileURLToPath(import.meta.url)), "recorded");
  const emit = (doc, code) => { write(chainFile, JSON.stringify(doc)); out(doc, code); };
  const complete = ["complete", "complete", "complete", "complete", "complete", "complete", "complete", "complete"].map((x) => [x]);
  if (args.includes("--review-file")) {
    const file = args[args.indexOf("--review-file") + 1];
    const mode = fs.statSync(file).mode & 0o777;
    const document = JSON.parse(fs.readFileSync(file, "utf8"));
    if (log) fs.appendFileSync(log, `${JSON.stringify({ reviewFile: document, reviewFileMode: mode, reviewFileFolderMode: fs.statSync(path.dirname(file)).mode & 0o777 })}\n`);
    if (mode !== 0o600 || document.protocol !== "oo.build-review/v1") out({ error: { code: "REVIEW_PRIVATE_FILE_REQUIRED", message: "REVIEW_PRIVATE_FILE_REQUIRED: the review file must be mode 0600" } }, 1);
    const profiles = path.join(os.homedir(), ".config", "open-ontology", "profiles");
    fs.mkdirSync(profiles, { recursive: true, mode: 0o700 });
    fs.writeFileSync(path.join(profiles, "shop.json"), JSON.stringify({ schema_version: 1, gateway: "https://127.0.0.1:9443", token_file: path.join(state, "first-user.token"), ca_file: path.join(state, "ca.crt") }), { mode: 0o600 });
    emit(chain("complete", complete), 0);
  }
  if (process.env.FAKE_OO_BUILD_FAIL) emit(chain("blocked", [["blocked", process.env.FAKE_OO_BUILD_FAIL]]), 1);
  for (const name of ["adjudication.json", "review-input.json"]) write(path.join(state, name), fs.readFileSync(path.join(recorded, name)));
  emit(chain("blocked", [["complete"], ["complete"], ["complete"], ["blocked", "REVIEW_FILE_REQUIRED: non-TTY never auto-accepts"]]), 1);
}
if (a === "up" || a === "build") { process.stderr.write(`${a}: step one\n${a}: step two with ooa_abcdef123456 hidden\n`); out({ command: a, ok: true }); }
if (a === "check") out({ ok: true });
if (a === "docs" && b === "runtime") out({ installed: true });
if (a === "serve") out({ protocol: "oo.docs.serve/v1", running: false }, 1);
if (a === "query") out({ query_id: "q_1", columns: [{ name: "id", type: "int" }, { name: "name", type: "text" }], rows: [[1, "Ada Example"], [2, "=cmd()"]], returned_rows: 2, truncated: false, duration_ms: 5, warnings: [] });
if (a === "freshness") out({ receipts: { state: "live" }, warnings: [] });
if (a === "validate") out({ valid: true });
if (a === "tables") out({ tables: [{ name: "public.customers" }, { name: "public.orders" }] });
if (a === "describe") out({ table: "public.customers", columns: ["id", "name"], column_details: [{ name: "id", type: "integer" }, { name: "name", type: "text" }] });
if (a === "knowledge" && b === "search") out({ items: [{ path: "ontology/entities/customer.yaml", snippet: "customer" }], has_more: false });
if (a === "knowledge" && b === "cat") out({ path: args.at(-1), content: "entity: customer\nkeys:\n  - id\n" });
process.stderr.write(JSON.stringify({ error: { code: "SO_UNKNOWN", message: "unknown command" } }));
process.exit(2);
