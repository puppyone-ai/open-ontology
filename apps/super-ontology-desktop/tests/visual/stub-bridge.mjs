/* global window */
// A deterministic stand-in for the preload bridge, installed before the page loads: the built renderer shows fixed data, so its screens can be
// compared pixel for pixel. Answers the channels the shown screens read; every other channel answers an empty page.
export function stubBridgeSource(scenario) {
  return `(${function install(scenario) {
    const now = "2026-11-10T10:00:00Z";
    const snapshot = (status) => ({ revision: 1, status, reason: null, code: null, preflight: null, health: status === "healthy" ? { events_head: 0 } : null, restarts: { count: 0, nextAttemptAt: null }, ownedByDesktop: true, keepEngineOnQuit: false, startedAt: now, connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 }, mode: "local", remote: null });
    const drive = (id, name, files, extra = {}) => ({ id, name, backend: "fs", config: { root: "/Users/example/" + name }, status: "active", revision: 1, counts: { published: files }, files_total: files, bytes_total: files * 2048, pending_review: 0, created_at: now, freshness: { state: "up_to_date" }, ...extra });
    const entity = (n, name, type, facts) => ({ id: "00000000-0000-4000-8000-0000000001" + String(n).padStart(2, "0"), key: name.toLowerCase().replace(/ /g, "-"), type, name, counts: { facts } });
    const drives = scenario.engine === "starting" ? [] : [drive("00000000-0000-4000-8000-000000000001", "My files", 3), drive("00000000-0000-4000-8000-000000000002", "Team docs", 12)];
    const entities = [entity(1, "Dana Lee", "Person", 4), entity(2, "Project Alpha", "Project", 5), entity(3, "Harbor feed", "Topic", 2), entity(4, "Projects/alpha.md", "Document", 3)];
    const empty = { items: [], next_cursor: null };
    const cli = (data, extra = {}) => ({ ok: true, exit: 0, data, error: null, cli: "oo command --json", ...extra });
    const database = { name: "shop", backend: "postgres", kind: "database", status: "configured", config: { host: "db.example.test", port: 5432, database: "shop", user: "reader", secret_ref: "(set)", tls_ca: true, include: ["public.customers", "public.orders"], exclude: ["public.audit_log"] } };
    const rows = Array.from({ length: 12 }, (_, i) => [i + 1, "Customer " + (i + 1), i % 4 === 0 ? null : "customer" + (i + 1) + "@example.test", (i * 37.5).toFixed(2)]);

    // The build the page sees: stage states for the screens "stopped for review", "failed with a cause" and "done".
    const STAGES = ["load", "candidate", "builder", "self_review", "publish", "gateway", "first_token", "share_prompt"];
    const stages = (states) => STAGES.map((name, index) => ({ name, status: states[index]?.[0] ?? "pending", completed: states[index]?.[0] === "complete" ? 1 : 0, total: 1, attempts: states[index] ? 1 : 0, error: states[index]?.[1] ?? null }));
    const done = ["complete", "complete", "complete", "complete", "complete", "complete", "complete", "complete"].map((x) => [x]);
    const building = { status: "running", publication: null, gateway: null, profile: null, modelCalls: null, review: false, stages: stages([["complete"], ["complete"], ["running"]]) };
    const reviewing = { status: "blocked", publication: "not_published", gateway: null, profile: null, modelCalls: 5, review: true, stages: stages([["complete"], ["complete"], ["complete"], ["blocked", "REVIEW_FILE_REQUIRED: non-TTY never auto-accepts"]]) };
    const finished = { status: "complete", publication: "published", gateway: "verified", profile: "shop", modelCalls: 5, review: false, stages: stages(done) };
    const failed = { status: "blocked", publication: "not_published", gateway: null, profile: null, modelCalls: null, review: false, stages: stages([["blocked", scenario.failure ?? "BUILD_LOAD_FAILED: oo-load exited 1: source error: tls fact=tls_handshake (a source on 127.0.0.1 or localhost is reached from the runtime as host.docker.internal; its server certificate must list host.docker.internal as a subject alternative name and carry the serverAuth usage)"]]) };
    const job = (id, action, state, build, error = null) => ({ jobId: id, action, state, exit: state === "succeeded" ? 0 : state === "running" ? null : 1, error, cli: "oo " + action + " --config stack.yaml --yes --json", startedAt: 0, offset: 0, next: 0, lines: [], build, gateway: state === "succeeded" && action === "build" ? true : null });
    const JOB_UP = "00000000-0000-4000-8000-0000000000a1"; const JOB_BUILD = "00000000-0000-4000-8000-0000000000a2"; const JOB_RESUME = "00000000-0000-4000-8000-0000000000a3";
    let buildStarted = false;
    const answer = (channel, request) => {
      switch (channel) {
        case "cli-job-start": buildStarted = buildStarted || request.action === "build"; return { jobId: request.action === "up" ? JOB_UP : JOB_BUILD, cli: "oo " + request.action + " --config stack.yaml --yes --json" };
        case "cli-job-read": {
          if (request.jobId === JOB_UP) return job(JOB_UP, "up", scenario.job === "up-running" ? "running" : "succeeded", null);
          if (request.jobId === JOB_RESUME) return job(JOB_RESUME, "build", scenario.resume === "running" ? "running" : "succeeded", scenario.resume === "running" ? { ...building, stages: stages([["complete"], ["complete"], ["complete"], ["complete"], ["running"]]) } : finished);
          if (scenario.job === "failed") return job(JOB_BUILD, "build", "failed", failed, { code: "BUILD_LOAD_FAILED", message: failed.stages[0].error.replace(/^BUILD_LOAD_FAILED: /u, ""), hint: null, stage: "load" });
          if (scenario.job === "running") return job(JOB_BUILD, "build", "running", building);
          return job(JOB_BUILD, "build", "review", reviewing);
        }
        case "cli-review-read": return scenario.review === "pending" || (buildStarted && !scenario.job?.startsWith("fail") && (scenario.job ?? "review") === "review" && scenario.review !== "done") ? { build: reviewing, digest: scenario.digest } : scenario.review === "done" ? { build: finished, digest: null } : { build: null, digest: null };
        case "cli-review-submit": return scenario.submitFails ? { ok: false, error: { code: "REVIEW_FANOUT", message: "", hint: null }, cli: "oo build --resume --config stack.yaml --review-file '<review.json>' --yes --json" } : { ok: true, jobId: JOB_RESUME, cli: "oo build --resume --config stack.yaml --review-file '<review.json>' --yes --json" };
        case "cli-db-kinds": return { kinds: [{ id: "postgres", label: "PostgreSQL", port: 5432, nameField: "database", tls: "required", wired: true, preflight: true }, { id: "mysql", label: "MySQL", port: 3306, nameField: "database", tls: "required", wired: true, preflight: true }, { id: "oracle", label: "Oracle", port: 1521, nameField: "service", tls: "optional", wired: true, preflight: true }, { id: "mariadb", label: "MariaDB", port: 3306, nameField: "database", tls: "optional", wired: false, preflight: false }], stackFile: "/stack.yaml", stackExists: true, secretsDir: "/secrets" };
        case "cli-db-list": return cli({ stackExists: scenario.databases === true, sources: scenario.databases === true ? [database] : [] });
        case "cli-db-check": return cli({ signed_in: true, checks: [{ name: "resolve", status: "ok", detail: "127.0.0.1" }, { name: "port", status: "ok", detail: "2 ms" }, { name: "tls", status: "ok", detail: "db.example.test - issuer example-ca - until 2027-01-01" }, { name: "sign_in", status: "ok", detail: "ok" }] });
        case "cli-db-test": return cli({ reachable: true });
        case "cli-doctor": return cli({ status: "ok", model: { status: "ok" }, sources: [{ source_id: "shop", status: "ok", probe: { capabilities: { catalog: { state: "available" }, log_cdc: { state: "needs_setup" } } }, dba_requirements: [{ name: "wal_level", observed: "replica", required: "logical", dba_statement: "ALTER SYSTEM SET wal_level = 'logical';" }] }] });
        case "cli-pipeline": return scenario.review === "none" || scenario.job ? cli({ status: "not_started", data_published_at: null, tables: { stale: [], review_pending: [] } }) : cli({ status: "complete", data_published_at: "2026-11-11T10:00:00Z", tables: { stale: [], review_pending: [] } });
        case "cli-gateway-read": return scenario.gateway === false ? { configured: false, url: null, transport: "production", tokenFile: null, caFile: null } : { configured: true, url: "https://gateway.example.test", transport: "production", tokenFile: "/t", caFile: null };
        case "cli-query": switch (request?.op) {
          case "tables": return cli({ tables: [{ name: "public.customers" }, { name: "public.orders" }, { name: "public.products" }] });
          case "describe": return cli({ table: request.table, columns: ["id", "name", "email", "balance"], column_details: [{ name: "id", type: "integer" }, { name: "name", type: "text" }, { name: "email", type: "text" }, { name: "balance", type: "numeric" }] });
          case "knowledge-search": return cli({ items: [{ path: "ontology/entities/customer.yaml", snippet: "customer" }, { path: "tables/public.customers.md", snippet: "customers" }], has_more: false });
          case "knowledge-cat": return cli({ path: request.path, content: "entity: customer\nkeys:\n  - id\n" });
          default: return cli({ query_id: "q_1", columns: [{ name: "id", type: "integer" }, { name: "name", type: "text" }, { name: "email", type: "text" }, { name: "balance", type: "numeric" }], rows, returned_rows: rows.length, truncated: false, duration_ms: 12, warnings: [] }, { cli: "oo query --max-rows 500 --json --gateway https://gateway.example.test --token-file /t -- 'select * from public.customers'" });
        }
        case "cli-diagnostics": return { platform: { os: "darwin", arch: "arm64", node: "24", app: "0.1.0" }, paths: { stackFile: "/stack.yaml", stackExists: true, secretsDir: "/secrets", dataDir: "/data" }, oo: { path: "/usr/local/bin/oo" }, version: cli({ name: "oo", version: "0.2.1" }), check: cli({ ok: true }), runtime: cli({ installed: true }), serve: cli({ running: true }) };
        case "ontology-versions": return { items: [{ n: 3, facts_added: 6, facts_retracted: 0, published_at: "2026-11-09T10:00:00Z", as_of: "2026-11-09T10:00:00Z" }, { n: 2, facts_added: 4, facts_retracted: 1, published_at: "2026-11-05T10:00:00Z", as_of: "2026-11-05T10:00:00Z" }], next_cursor: null };
        case "engine-read": case "engine-preflight": case "engine-start": case "engine-restart": return snapshot(scenario.engine);
        case "drives-list": return drives;
        case "entities-search": return { items: entities, next_cursor: null };
        case "agent-token-list": return empty;
        case "model-config-read": return { configured: false };
        case "events-subscribe": return { instanceId: "stub", seq: 0 };
        default: return empty;
      }
    };
    const frozen = Date.parse("2026-11-12T10:00:00Z");
    window.localStorage.setItem("so.firstRun.done", scenario.firstRun ? "0" : "1");
    if (scenario.firstRun) window.localStorage.removeItem("so.firstRun.done");
    window.superOntology = {
      invoke: async (channel, request) => ({ ok: true, value: answer(channel, request) }),
      subscribe: () => () => undefined,
      pathsForFiles: () => ({ paths: [], unreadable: 0 }),
      openExternal: async () => undefined,
      shell: { info: async () => ({ name: "Super Ontology", version: "0.1.0", platform: "darwin", arch: "arm64", locale: "en-US", packaged: true, license: "Apache-2.0", releaseUrl: "" }), readSettings: async () => ({ language: "en", ooExecutable: null, runtime: "auto", keepEngineOnQuit: false, remote: null }), writeSettings: async (patch) => patch, onSettingsChanged: () => () => undefined, onMenuCommand: () => () => undefined, readNotice: async () => "", copyToClipboard: async () => true, openReleasePage: async () => true },
    };
    void frozen;
  }})(${JSON.stringify(scenario)})`;
}
