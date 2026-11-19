// A stand-in for `oo serve` (10 §8.6) used by main-process tests. It is a real
// separate process with a real loopback HTTP server, so spawning, signals,
// exit codes, file permissions and Bearer checks are exercised for real.
// Behaviour is steered by environment variables and by files in the state
// directory, never by anything secret.
import { randomBytes } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const args = process.argv.slice(2);
if (args[0] === "--version") {
  process.stdout.write(`${JSON.stringify({ version: "0.3.0" })}\n`);
  process.exit(0);
}
if (args[0] !== "serve") process.exit(2);

const flag = (name) => args.includes(name);
const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : null);
const stateDir = option("--state-dir");
if (!stateDir) process.exit(2);
const serveJson = path.join(stateDir, "serve.json");
const tokenFile = path.join(stateDir, "serve.token");
const env = process.env;

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

if (flag("--status")) {
  try {
    const document = JSON.parse(fs.readFileSync(serveJson, "utf8"));
    if (alive(document.pid)) { process.stdout.write(`${JSON.stringify(document)}\n`); process.exit(0); }
  } catch { /* Not running. */ }
  process.exit(1);
}

fs.mkdirSync(stateDir, { recursive: true, mode: 0o700 });
const invocations = path.join(stateDir, "fake-oo-invocations.jsonl");
fs.appendFileSync(invocations, `${JSON.stringify(args)}\n`);
fs.appendFileSync(path.join(stateDir, "fake-oo-exchange.jsonl"), `${JSON.stringify(env.OO_DOCS_EXCHANGE ?? null)}\n`);
const launchNumber = fs.readFileSync(invocations, "utf8").trim().split("\n").length;

const mode = env.FAKE_OO_MODE ?? "ok";
if (mode === "exit-6-once" && launchNumber === 1) process.exit(6);
if (/^exit-\d+$/u.test(mode)) process.exit(Number(mode.slice(5)));
const crashFirst = Number(env.FAKE_OO_CRASH_FIRST ?? 0);

process.on("SIGTERM", () => {
  if (env.FAKE_OO_IGNORE_TERM === "1") return;
  fs.writeFileSync(path.join(stateDir, "fake-oo-terminated"), String(Date.now()));
  fs.rmSync(serveJson, { force: true });
  fs.rmSync(tokenFile, { force: true });
  process.exit(0);
});

const token = `oodt_${randomBytes(32).toString("base64url")}`;

function healthControl() {
  try { return JSON.parse(fs.readFileSync(path.join(stateDir, "fake-health.json"), "utf8")); } catch { return null; }
}

/** Optional documents for the Drive endpoints: `<state>/fake-docs.json` = { drives, files, dirs, history: { <file id>: StateEntry[] }, as_of_seq }, read on every request. */
function fakeDocs() {
  try { return JSON.parse(fs.readFileSync(path.join(stateDir, "fake-docs.json"), "utf8")); } catch { return null; }
}

const server = http.createServer((request, response) => {
  if (request.url === "/livez") { response.writeHead(204).end(); return; }
  const send = (status, body, headers = {}) => {
    response.writeHead(status, { "content-type": "application/json", "OO-Docs-Api": "1.0", ...headers });
    response.end(JSON.stringify(body));
  };
  if (request.headers.authorization !== `Bearer ${token}`) {
    send(401, { code: "DOCS_ENGINE_UNAUTHENTICATED", message: "no", retryable: false, details: {} });
    return;
  }
  // Every authenticated request, for scenarios that check which routes the application used (`<state>/fake-requests.jsonl`).
  if (!request.url?.startsWith("/v1/docs/events")) { try { fs.appendFileSync(path.join(stateDir, "fake-requests.jsonl"), `${JSON.stringify({ method: request.method, url: request.url })}\n`); } catch { /* Best effort. */ } }
  if (request.url?.startsWith("/v1/docs/events")) {
    // Held-open event stream with the engine's own `retry` hint. Nothing is pushed unless a scenario appends events (one JSON
    // object per line, each with a `seq`) to `<state>/fake-events.ndjson`: every connection then receives the lines it has not seen.
    response.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store", "OO-Docs-Api": "1.0" });
    response.write("retry: 3000\n\n");
    const eventsFile = path.join(stateDir, "fake-events.ndjson");
    let offset = 0;
    const timer = setInterval(() => {
      let size = 0;
      try { size = fs.statSync(eventsFile).size; } catch { return; }
      if (size <= offset) return;
      const fd = fs.openSync(eventsFile, "r");
      const buffer = Buffer.alloc(size - offset);
      fs.readSync(fd, buffer, 0, buffer.length, offset);
      fs.closeSync(fd);
      const text = buffer.toString("utf8");
      const complete = text.slice(0, text.lastIndexOf("\n") + 1);
      offset += Buffer.byteLength(complete);
      for (const line of complete.split("\n")) {
        if (!line) continue;
        const event = JSON.parse(line);
        response.write(`${Number.isSafeInteger(event.seq) ? `id: ${event.seq}\n` : ""}event: ${event.type}\ndata: ${line}\n\n`);
      }
    }, 20);
    response.on("close", () => clearInterval(timer));
    return;
  }
  // Drive registration (F-S11-01 T1): `POST /v1/docs/drives` validates the shape the real engine validates for the object-store
  // backends, stores the Drive in the fake documents and answers with it; the same Idempotency-Key answers the same Drive again.
  // The body is also kept in `<state>/fake-drive-requests.jsonl` so a scenario can check what the application really sent.
  if (request.method === "POST" && request.url === "/v1/docs/drives") {
    let raw = "";
    request.on("data", (chunk) => { raw += chunk; });
    request.on("end", () => {
      let body = null;
      try { body = JSON.parse(raw); } catch { /* Refused below. */ }
      fs.appendFileSync(path.join(stateDir, "fake-drive-requests.jsonl"), `${JSON.stringify({ key: request.headers["idempotency-key"] ?? null, body })}\n`);
      const refuse = (code) => send(400, { code, message: "no", retryable: false, details: {} });
      const config = body?.config;
      if (!body || typeof body.name !== "string" || !config || typeof config !== "object") { refuse("DOCS_SOURCE_INVALID_CONFIG"); return; }
      const cloud = ["s3", "oss", "cos", "r2"].includes(body.backend);
      if (!cloud && body.backend !== "fs") { refuse("DOCS_SOURCE_INVALID_CONFIG"); return; }
      if (cloud) {
        if (typeof config.bucket !== "string" || config.bucket.length < 3) { refuse("DOCS_SOURCE_BUCKET_NOT_FOUND"); return; }
        if (body.backend === "s3" && typeof config.endpoint !== "string") { refuse("DOCS_SOURCE_ENDPOINT_INVALID"); return; }
        if ((body.backend === "oss" || body.backend === "cos") && !config.region && !config.endpoint) { refuse("DOCS_SOURCE_ENDPOINT_INVALID"); return; }
        if (body.backend === "r2" && !/^[0-9a-f]{32}$/u.test(config.account_id ?? "")) { refuse("DOCS_SOURCE_ENDPOINT_INVALID"); return; }
        if (typeof config.secret_access_key?.secret_ref !== "string" || typeof config.access_key_id?.secret_ref !== "string") { refuse("DOCS_SOURCE_CREDENTIAL_MISSING"); return; }
      }
      const current = fakeDocs() ?? {};
      current.drives ??= []; current.files ??= []; current.dirs ??= [];
      const known = request.headers["idempotency-key"] ? current.drives.find((drive) => drive.created_by_key === request.headers["idempotency-key"]) : null;
      if (known) { send(200, known, { "OO-Idempotent-Replay": "true" }); return; }
      const drive = {
        id: `00000000-0000-4000-8000-${String(current.drives.length + 1).padStart(12, "e")}`, name: body.name, backend: body.backend, config, status: "active", revision: 1,
        counts: {}, files_total: 0, created_at: new Date().toISOString(), model_enabled: false, created_by_key: request.headers["idempotency-key"] ?? null,
      };
      current.drives.push(drive);
      fs.writeFileSync(path.join(stateDir, "fake-docs.json"), JSON.stringify(current));
      send(201, drive);
    });
    return;
  }
  const docs = fakeDocs();
  // The model the engine holds (`docs.model`, set by a scenario): the database pipeline reads it when the first stack file is written.
  if (request.method === "GET" && request.url?.split("?")[0] === "/v1/docs/config/model") { send(200, docs?.model ?? { configured: false }); return; }
  if (docs && request.method === "GET" && request.url?.startsWith("/v1/docs/drives")) {
    const url = new URL(request.url, "http://127.0.0.1");
    const files = url.pathname.match(/^\/v1\/docs\/drives\/([^/]+)\/(files|tree)$/u);
    if (files) {
      const folder = url.searchParams.get("path") ?? "";
      const recursive = url.searchParams.get("recursive") === "true";
      const states = (url.searchParams.get("state") ?? "").split(",").filter(Boolean);
      const inFolder = (docs.files ?? []).filter((file) => file.drive_id === files[1] && (folder === "" || file.path.startsWith(`${folder}/`))
        && (recursive || !file.path.slice(folder === "" ? 0 : folder.length + 1).includes("/")) && (states.length === 0 || states.includes(file.state)));
      const sorted = inFolder.sort((a, b) => a.path.localeCompare(b.path));
      // Pages like the engine's: `limit` items from the opaque `cursor` (an offset); without a limit everything comes at once.
      const limit = Number(url.searchParams.get("limit") ?? 0);
      const from = Number(url.searchParams.get("cursor") ?? 0) || 0;
      const items = limit > 0 ? sorted.slice(from, from + limit) : sorted;
      const page = { items, next_cursor: limit > 0 && from + limit < sorted.length ? String(from + limit) : null, as_of_seq: docs.as_of_seq ?? 1 };
      send(200, files[2] === "tree" ? { dirs: (docs.dirs ?? []).filter((dir) => dir.drive_id === files[1] && dir.path.split("/").length === (folder === "" ? 1 : folder.split("/").length + 1)).map(({ drive_id: _drive, ...dir }) => dir), files: page } : page);
      return;
    }
    if (url.pathname === "/v1/docs/drives") { send(200, { items: docs.drives ?? [], next_cursor: null, as_of_seq: docs.as_of_seq ?? 1 }); return; }
    const one = (docs.drives ?? []).find((drive) => url.pathname === `/v1/docs/drives/${drive.id}`);
    if (one) { send(200, one); return; }
  }
  // Share preview and creation (the Share dialog compiles a pack on its Settings step, then creates the Share).
  if (request.method === "POST" && request.url?.split("?")[0] === "/v1/docs/context-packs:compile") {
    request.resume();
    request.on("end", () => send(200, { id: "00000000-0000-4000-8000-0000000000c1", pack_digest: "a".repeat(64), version: 1, drive_id: docs.drives?.[0]?.id ?? "00000000-0000-4000-8000-0000000000d1", scope: [""], manifest: {}, tokens: { brief: 120, budget: 800, total: 120 }, brief_preview: "# BRIEF\n", created_at: new Date().toISOString() }));
    return;
  }
  if (request.method === "POST" && request.url?.split("?")[0] === "/v1/docs/shares") {
    request.resume();
    request.on("end", () => send(201, { id: "00000000-0000-4000-8000-0000000000e1", pack_id: "00000000-0000-4000-8000-0000000000c1", pack_digest: "a".repeat(64), drive_id: docs.drives?.[0]?.id ?? "00000000-0000-4000-8000-0000000000d1", scope: [""], role: "suggester", budget: 800, version: 1, state: "active", created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 7 * 86_400_000).toISOString(), revision: 1 }));
    return;
  }
  if (docs?.suggestions) {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const find = (id) => docs.suggestions.find((item) => item.id === id);
    const save = () => fs.writeFileSync(path.join(stateDir, "fake-docs.json"), JSON.stringify(docs));
    const digest = url.pathname === "/v1/docs/suggestions:digest";
    if (request.method === "GET" && digest) {
      const pending = docs.suggestions.filter((item) => item.status === "pending");
      const inEffect = docs.suggestions.filter((item) => ["auto_accepted", "superseded_silently"].includes(item.status));
      send(200, { day: "2026-11-18", counts: { pending: pending.length }, total: docs.suggestions.length, in_effect: inEffect, pending });
      return;
    }
    const one = url.pathname.match(/^\/v1\/docs\/suggestions\/([^/:]+)(\/diff|:decide)?$/u);
    if (one && find(one[1])) {
      const item = find(one[1]);
      if (request.method === "GET" && one[2] === "/diff") { send(200, docs.diffs[item.id]); return; }
      if (request.method === "GET") { send(200, item); return; }
      if (request.method === "POST" && one[2] === ":decide") {
        let body = "";
        request.on("data", (chunk) => { body += chunk; });
        request.on("end", () => { const decision = JSON.parse(body).decision; item.status = { accept: "accepted", reject: "rejected", undo: "undone" }[decision]; item.revision += 1; docs.decisions = [...(docs.decisions ?? []), [item.id, decision]]; save(); send(200, item); });
        return;
      }
    }
    if (request.method === "GET" && url.pathname === "/v1/docs/shares" && docs.shares) { send(200, { items: docs.shares, next_cursor: null, as_of_seq: docs.as_of_seq ?? 1 }); return; }
    if (request.method === "GET" && /^\/v1\/docs\/shares\/[^/]+\/updates$/u.test(url.pathname) && docs.shares) { send(200, { items: [] }); return; }
    if (request.method === "GET" && url.pathname === "/v1/docs/suggestions") { send(200, { items: docs.suggestions.filter((item) => !url.searchParams.get("status") || item.status === url.searchParams.get("status")) }); return; }
    const blocks = url.pathname.match(/^\/v1\/docs\/files\/([^/]+)\/blocks$/u);
    if (blocks) { send(200, { items: docs.blocks?.[blocks[1]] ?? [], next_cursor: null, as_of_seq: 1 }); return; }
    if (request.method === "POST" && /:sync$/u.test(url.pathname)) { request.resume(); request.on("end", () => send(200, { status: "running" })); return; }
  }
  if (docs && request.method === "GET") {
    const detail = request.url?.match(/^\/v1\/docs\/files\/([^/?]+)/u);
    const found = detail ? (docs.files ?? []).find((file) => file.id === detail[1]) : null;
    if (found) { send(200, { ...found, parse: null, llm: null, versions: [], history: docs.history?.[found.id] ?? [] }); return; }
  }
  if (request.url?.startsWith("/v1/docs/drives") && request.method === "GET") {
    send(200, { items: [], next_cursor: null, as_of_seq: 0 });
    return;
  }
  if (request.url === "/v1/docs/health") {
    const control = healthControl();
    if (control) { send(control.http ?? 200, control.body ?? {}, control.headers ?? {}); return; }
    send(200, {
      api_version: "1.0", engine_version: "0.3.0", status: "ok",
      components: { db: { status: "ok" }, parser: { status: "ok" }, model: { status: "not_configured" }, cas: { status: "ok" } },
      queue: { queued: 0, running: 0 }, events_head: 0,
    });
    return;
  }
  send(404, { code: "DOCS_ENGINE_NOT_FOUND", message: "no", retryable: false, details: {} });
});

setTimeout(() => {
  server.listen(Number(option("--port") ?? 0), "127.0.0.1", () => {
    const { port } = server.address();
    fs.writeFileSync(tokenFile, `${token}\n`, { mode: 0o600 });
    fs.writeFileSync(serveJson, JSON.stringify({
      protocol: "oo.docs.serve/v1", pid: process.pid, port, api_version: "1.0",
      token_file: tokenFile, state_dir: stateDir, started_at: new Date().toISOString(),
    }), { mode: env.FAKE_OO_BAD_PERMS === "1" ? 0o644 : 0o600 });
    if (env.FAKE_OO_BAD_PERMS === "1") fs.chmodSync(serveJson, 0o644);
    if (crashFirst >= launchNumber || env.FAKE_OO_CRASH_ALWAYS === "1") {
      setTimeout(() => process.exit(1), Number(env.FAKE_OO_CRASH_MS ?? 150));
    }
  });
}, Number(env.FAKE_OO_START_DELAY_MS ?? 0));
