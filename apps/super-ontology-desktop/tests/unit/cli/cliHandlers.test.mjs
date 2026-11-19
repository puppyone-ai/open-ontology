import { describe, expect, it, beforeEach, afterEach } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createCliHandlers } from "../../../electron/main/super-ontology/cli-handlers.mjs";
import { displayCommand, failureOf, parseJson, redact } from "../../../electron/main/super-ontology/cli-runner.mjs";
import { parseSuperOntologyRequest } from "../../../src/shared/super-ontology/requests.mjs";
import { DB_KINDS, DB_WIRED_KIND_IDS } from "../../../src/shared/super-ontology/db-kinds.mjs";

const FAKE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../support/fake-oo-cli.mjs");
const sender = {};
let root; let log; let handlers; let grantsIssued;
const grants = { resolve: (_s, id) => { if (!grantsIssued.has(id)) throw new Error("no grant"); return grantsIssued.get(id); } };
const model = { configured: true, provider: "openai-compatible", base_url: "https://model.example.test/v1", model: "m", key_ref: "file:/x/model-key" };
const calls = async () => (await fs.readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean).map((l) => JSON.parse(l));

beforeEach(async () => {
  root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-cli-")));
  log = path.join(root, "calls.jsonl");
  grantsIssued = new Map([["ca-grant", path.join(root, "ca.pem")], ["token-grant", path.join(root, "token")]]);
  await fs.mkdir(path.join(root, "data", "state"), { recursive: true });
  handlers = createCliHandlers({ locator: null, grants, stateDir: path.join(root, "data", "state"), client: { request: async () => ({ value: model }) }, environment: { SO_CLI_OO: FAKE, SO_CLI_HOME: path.join(root, "home"), FAKE_OO_LOG: log }, homedir: () => root });
  process.env.FAKE_OO_LOG = log;
});
afterEach(async () => { delete process.env.FAKE_OO_LOG; delete process.env.FAKE_OO_BAD_LOGIN; await fs.rm(root, { recursive: true, force: true }); });

const base = { kind: "postgres", host: "db.example.test", port: 5432, database: "shop", user: "ro", password: "fake-pw-123", tlsCaGrantId: "ca-grant" };

describe("database kinds", () => {
  it("lists every connector family once and marks what the CLI can drive", () => {
    expect(DB_KINDS.map((k) => k.id)).toEqual(expect.arrayContaining(["postgres", "mysql", "mariadb", "oracle", "sqlserver", "db2", "dm8", "kingbase", "opengauss", "oceanbase", "tidb", "polardb", "doris", "starrocks", "clickhouse", "mongodb", "opensearch"]));
    expect(new Set(DB_KINDS.map((k) => k.id)).size).toBe(DB_KINDS.length);
    expect(DB_WIRED_KIND_IDS).toEqual(["postgres", "mysql", "oracle", "sqlserver", "db2", "kingbase", "dm8"]);
  });
  it("refuses a kind the CLI cannot add", () => {
    expect(() => parseSuperOntologyRequest("cli-db-add", { ...base, kind: "mariadb", name: "x", include: ["a.b"], operationId: "op_0123456789abcdef" })).toThrow();
  });
});

describe("request validation", () => {
  const op = { operationId: "op_0123456789abcdef" };
  it("accepts a full add request and rejects shell-ish table entries", () => {
    expect(parseSuperOntologyRequest("cli-db-add", { ...base, name: "shop", include: ["public.customers"], ...op }).name).toBe("shop");
    for (const bad of ["a b", "a;drop", "a'b", ""]) expect(() => parseSuperOntologyRequest("cli-db-add", { ...base, name: "shop", include: [bad], ...op })).toThrow();
    expect(() => parseSuperOntologyRequest("cli-db-add", { ...base, name: "shop", include: [], ...op })).toThrow();
  });
  it("never accepts a host path or an oo argument vector", () => {
    expect(() => parseSuperOntologyRequest("cli-db-check", { ...base, args: ["--help"] })).toThrow();
    expect(() => parseSuperOntologyRequest("cli-query", { op: "query", sql: "select 1", path: "/etc/passwd", configFile: "/x" })).toThrow();
    expect(() => parseSuperOntologyRequest("cli-ontology-export", { grantId: "abcdefgh", files: [{ name: "../x.yaml", text: "a" }], ...op })).toThrow();
    expect(() => parseSuperOntologyRequest("cli-ontology-export", { grantId: "abcdefgh", files: [{ name: "/abs.yaml", text: "a" }], ...op })).toThrow();
  });
});

describe("database source flow through oo", () => {
  it("checks a connection without saving and never puts the password in argv", async () => {
    const result = await handlers["cli-db-check"](base, { sender });
    expect(result.ok).toBe(true);
    expect(result.data.signed_in).toBe(true);
    const [call] = await calls();
    expect(call.args.slice(0, 3)).toEqual(["init", "--engine", "postgres"]);
    expect(JSON.stringify(call.args)).not.toContain("fake-pw");
    expect(result.cli).toContain("<password file>");
    expect(result.cli).not.toContain("fake-pw");
    const left = await fs.readdir(path.join(root, "home", ".config", "open-ontology", "secrets"));
    expect(left.filter((f) => f.startsWith("chk_"))).toEqual([]);
  });

  it("reports a refused sign-in as a failure with a next step code", async () => {
    process.env.FAKE_OO_BAD_LOGIN = "1";
    const result = await handlers["cli-db-check"](base, { sender });
    expect(result.ok).toBe(false);
    expect(result.error.code).toBe("SO_CHECK_NOT_SIGNED_IN");
  });

  it("creates the stack with the first source, then adds the next through oo source add with the password on stdin", async () => {
    const first = await handlers["cli-db-add"]({ ...base, name: "shop", include: ["public.customers"] }, { sender });
    expect(first.ok).toBe(true);
    const stack = JSON.parse(await fs.readFile(path.join(root, "data", "stack.yaml"), "utf8"));
    expect(stack.version).toBe("oo.stack/v1");
    expect(stack.sources[0]).toMatchObject({ id: "shop", engine: "postgres", scope: { include: ["public.customers"] } });
    expect(JSON.stringify(stack)).not.toContain("fake-pw");
    expect(stack.sources[0].password.secret_ref).toMatch(/^file:.*src_shop_pw$/u);
    const pw = path.join(root, "home", ".config", "open-ontology", "secrets", "src_shop_pw");
    expect((await fs.stat(pw)).mode & 0o777).toBe(0o600);
    expect(await fs.readFile(pw, "utf8")).toBe("fake-pw-123");

    const second = await handlers["cli-db-add"]({ ...base, kind: "mysql", name: "crm", port: undefined, include: ["crm.accounts"], exclude: ["crm.audit"] }, { sender });
    expect(second.ok).toBe(true);
    const add = (await calls()).find((c) => c.args[0] === "source" && c.args[1] === "add");
    expect(add.args).toEqual(expect.arrayContaining(["mysql", "--name", "crm", "--port", "3306", "--include", "crm.accounts", "--exclude", "crm.audit", "--password-stdin"]));
    expect(add.stdinHasPassword).toBe(true);
    expect(JSON.stringify(add.args)).not.toContain("fake-pw");
    expect(second.cli).toContain("--password-stdin");
  });

  it("needs a model for the first source and says so", async () => {
    const h = createCliHandlers({ locator: null, grants, stateDir: path.join(root, "data", "state"), client: { request: async () => ({ value: { configured: false } }) }, environment: { SO_CLI_OO: FAKE, SO_CLI_HOME: path.join(root, "home2") }, homedir: () => root });
    const result = await h["cli-db-add"]({ ...base, name: "shop", include: ["a.b"] }, { sender });
    expect(result).toMatchObject({ ok: false, error: { code: "SO_MODEL_REQUIRED" } });
    await expect(fs.stat(path.join(root, "data", "stack.yaml"))).rejects.toThrow();
  });

  it("requires a certificate authority for a kind that needs one", async () => {
    const { tlsCaGrantId: _drop, ...noCa } = base;
    const result = await handlers["cli-db-add"]({ ...noCa, name: "shop", include: ["a.b"] }, { sender });
    expect(result.error.code).toBe("SO_TLS_CA_REQUIRED");
  });

  it("lists, tests and removes by name", async () => {
    await handlers["cli-db-add"]({ ...base, name: "shop", include: ["public.customers"] }, { sender });
    expect((await handlers["cli-db-list"]()).data.sources[0].name).toBe("shop");
    expect((await handlers["cli-db-test"]({ name: "shop" })).ok).toBe(true);
    expect((await handlers["cli-db-remove"]({ name: "shop" })).ok).toBe(true);
  });
});

describe("pipeline jobs", () => {
  it("runs oo up in the background, keeps a redacted tail and finishes", async () => {
    await handlers["cli-db-add"]({ ...base, name: "shop", include: ["public.customers"] }, { sender });
    const { jobId } = await handlers["cli-job-start"]({ action: "up" });
    let job;
    for (let i = 0; i < 50; i += 1) { job = await handlers["cli-job-read"]({ jobId }); if (job.state !== "running") break; await new Promise((r) => setTimeout(r, 40)); }
    expect(job.state).toBe("succeeded");
    expect(job.lines.join("\n")).toContain("up: step one");
    expect(job.lines.join("\n")).not.toContain("ooa_abcdef");
    await expect(handlers["cli-job-read"]({ jobId: "00000000-0000-4000-8000-000000000000" })).rejects.toThrow();
  });
});

describe("gateway queries", () => {
  it("says it is not connected before a Gateway is saved", async () => {
    const result = await handlers["cli-query"]({ op: "tables" });
    expect(result).toMatchObject({ ok: false, error: { code: "SO_GATEWAY_NOT_CONNECTED" } });
    expect(result.cli).toContain("oo tables");
  });

  it("connects with granted files, then runs each op with the right flags", async () => {
    const set = await handlers["cli-gateway-set"]({ url: "http://127.0.0.1:8443/", tokenGrantId: "token-grant", transport: "local-development" }, { sender });
    expect(set).toMatchObject({ configured: true, url: "http://127.0.0.1:8443" });
    const mode = (await fs.stat(path.join(root, "data", "gateway.json"))).mode & 0o777;
    expect(mode).toBe(0o600);
    const q = await handlers["cli-query"]({ op: "query", sql: "select * from public.customers", maxRows: 100 });
    expect(q.ok).toBe(true);
    const call = (await calls()).find((c) => c.args[0] === "query");
    expect(call.args).toEqual(expect.arrayContaining(["--max-rows", "100", "--gateway", "http://127.0.0.1:8443", "--transport", "local-development", "--token-file", path.join(root, "token")]));
    expect(call.args.slice(-2)).toEqual(["--", "select * from public.customers"]);
    for (const [request, head] of [[{ op: "tables" }, "tables"], [{ op: "describe", table: "public.customers" }, "describe"], [{ op: "validate", sql: "select 1" }, "validate"], [{ op: "freshness" }, "freshness"], [{ op: "knowledge-search", text: "customer", pathPrefix: "ontology/" }, "knowledge"], [{ op: "knowledge-cat", path: "ontology/entities/customer.yaml" }, "knowledge"]]) {
      expect((await handlers["cli-query"](request)).ok, head).toBe(true);
    }
    await expect(handlers["cli-gateway-set"]({ url: "http://user:pw@x.example.test", tokenGrantId: "token-grant" }, { sender })).rejects.toThrow();
    await expect(handlers["cli-gateway-set"]({ url: "http://x.example.test/path", tokenGrantId: "token-grant" }, { sender })).rejects.toThrow();
  });
});

describe("export and diagnostics", () => {
  it("writes the YAML files into a new folder and refuses to escape it", async () => {
    grantsIssued.set("dir-grant", root);
    const result = await handlers["cli-ontology-export"]({ grantId: "dir-grant", files: [{ name: "ontology-v1.yaml", text: "a: 1\n" }, { name: "gateway/ontology/x.yaml", text: "b: 2\n" }] }, { sender });
    expect(result.count).toBe(2);
    expect(await fs.readFile(path.join(result.directory, "gateway", "ontology", "x.yaml"), "utf8")).toBe("b: 2\n");
    await expect(handlers["cli-ontology-export"]({ grantId: "dir-grant", files: [{ name: "a/../../escape.yaml", text: "x" }] }, { sender })).rejects.toThrow();
  });
  it("reports version, check, runtime and serve", async () => {
    const report = await handlers["cli-diagnostics"]();
    expect(report.version.data.version).toBe("0.2.1");
    expect(report.check.ok).toBe(true);
    expect(report.serve.data.running).toBe(false);
    expect(report.paths.stackExists).toBe(false);
  });
});

describe("runner helpers", () => {
  it("quotes only when needed and hides secret-looking text", () => {
    expect(displayCommand(["source", "add", "a b", "x'y"])).toBe("oo source add 'a b' 'x'\\''y'");
    expect(redact("token ood" + "t_abcdefghij and sk-abcdefgh12")).not.toMatch(/abcdefg/u);
  });
  it("reads the error document from either stream and the last json line", () => {
    expect(failureOf({ code: 1, stdout: '{"x":1}\n', stderr: '{"error":{"code":"A_B","message":"nope"}}' })).toMatchObject({ code: "A_B", message: "nope" });
    expect(failureOf({ code: 2, stdout: "", stderr: '{"class":"local","message":"READINESS_TRANSPORT","hint":"check"}' })).toMatchObject({ code: "READINESS_TRANSPORT", hint: "check" });
    expect(failureOf({ timedOut: true, stdout: "", stderr: "" }).code).toBe("SO_TIMEOUT");
    expect(parseJson("noise\n{\"a\":1}\n")).toEqual({ a: 1 });
  });
});
