#!/usr/bin/env node
// A stand-in `oo` for the CLI parity tests: it answers the commands the app runs with canned JSON and records its argument vectors
// (never stdin) in $FAKE_OO_LOG, so a test can see exactly what the app asked for. Synthetic data only.
import fs from "node:fs";

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
