import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createControlClient, ROUTES } from "@so-main/control-client.mjs";
import { createControlHandlers } from "@so-main/control-handlers.mjs";
import { createGrantStore } from "@so-main/grants.mjs";
import { registerSuperOntologyIpcHandlers } from "../../../electron/main/ipc-super-ontology.mjs";
import { DOCS_ERROR_DETAIL_KEYS } from "@so-shared/docs-contract.generated.mjs";
import { SuperOntologyError, pickDetails, toErrorResult } from "@so-shared/errors.mjs";
import { CONTRACT_TOKEN, startContractServer } from "../../support/contract-server.mjs";

const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
const OP = "op_0123456789abcdef";
const fastTimers = { setTimeout: (callback) => setTimeout(callback, 5) };

describe("control client", () => {
  let server;
  let token;
  let logs;
  let authFailures;
  const make = (extra = {}) => createControlClient({
    credentials: () => ({ baseUrl: server.baseUrl, token }),
    reloadCredentials: async () => true,
    onAuthFailure: () => { authFailures += 1; },
    logger: { debug: (...entry) => logs.push(entry), warn: (...entry) => logs.push(entry) },
    timers: fastTimers,
    ...extra,
  });
  const errorOf = (promise) => promise.then(() => { throw new Error("expected a rejection"); }, (error) => error);

  beforeEach(() => { logs = []; authFailures = 0; token = CONTRACT_TOKEN; });
  afterEach(async () => { await server?.close(); server = undefined; });

  describe("requests", () => {
    it("sends Bearer, OO-Docs-Api-Min, a loopback Host and no Origin", async () => {
      server = await startContractServer();
      const { value } = await make().request("drivesList", { query: { limit: 200 } });
      expect(value.items.length).toBeGreaterThan(0);
      const [sent] = server.requests;
      expect(sent.path).toBe("/v1/docs/drives?limit=200");
      expect(sent.headers).toMatchObject({
        authorization: `Bearer ${CONTRACT_TOKEN}`, "oo-docs-api-min": "1.0", host: `127.0.0.1:${server.port}`,
      });
      expect(sent.headers.origin).toBeUndefined();
    });

    it("sends the operationId as Idempotency-Key and reports a replay", async () => {
      server = await startContractServer();
      const body = { name: "Docs", backend: "fs", config: { root: "/Users/example/Drive" } };
      const first = await make().request("driveCreate", { body, operationId: OP });
      const second = await make().request("driveCreate", { body, operationId: OP });
      expect(first.replayed).toBe(false);
      expect(second.replayed).toBe(true);
      expect(server.requests.map((request) => request.headers["idempotency-key"])).toEqual([OP, OP]);
      expect(server.requests[0].headers["content-type"]).toBe("application/json");
    });

    it("refuses a side-effecting call without an operationId before sending anything", async () => {
      server = await startContractServer();
      const error = await errorOf(make().request("driveCreate", { body: {} }));
      expect(error).toMatchObject({ code: "SO_INVALID_REQUEST" });
      expect(server.requests).toHaveLength(0);
    });

    it("sends If-Match with the revision for PATCH and DELETE, and requires it", async () => {
      server = await startContractServer();
      await make().request("driveUpdate", { params: { id: DRIVE_ID }, body: { paused: true }, revision: 7 });
      await make().request("driveDelete", { params: { id: DRIVE_ID }, query: { purge: "false" }, revision: 8 });
      expect(server.requests.map((request) => [request.method, request.path, request.headers["if-match"]])).toEqual([
        ["PATCH", `/v1/docs/drives/${DRIVE_ID}`, '"r7"'],
        ["DELETE", `/v1/docs/drives/${DRIVE_ID}?purge=false`, '"r8"'],
      ]);
      expect(await errorOf(make().request("driveUpdate", { params: { id: DRIVE_ID }, body: { paused: true } }))).toMatchObject({ code: "SO_INVALID_REQUEST" });
    });

    it("builds the :sync path and keeps route parameters validated and encoded", async () => {
      server = await startContractServer();
      await make().request("driveSync", { params: { id: DRIVE_ID }, body: {}, operationId: OP });
      expect(server.requests[0].path).toBe(`/v1/docs/drives/${DRIVE_ID}:sync`);
      for (const bad of ["../etc", "not-a-uuid", `${DRIVE_ID}/files`, "", 7]) {
        expect(await errorOf(make().request("driveGet", { params: { id: bad } }))).toMatchObject({ code: "SO_INVALID_REQUEST" });
      }
      expect(await errorOf(make().request("noSuchRoute", {}))).toMatchObject({ code: "SO_INVALID_REQUEST" });
      expect(server.requests).toHaveLength(1);
    });

    it("joins list filters into the query string and skips unset values", async () => {
      server = await startContractServer();
      await make().request("filesList", { params: { id: DRIVE_ID }, query: { path: "a b", state: ["parsed", "published"], q: undefined, limit: 50, recursive: true } });
      expect(server.requests[0].path).toBe(`/v1/docs/drives/${DRIVE_ID}/files?path=a+b&state=parsed%2Cpublished&limit=50&recursive=true`);
    });

    it("only has routes under /v1/docs (plus the one read-only ontology schema of the atlas) and has no hosted-service path", async () => {
      for (const definition of Object.values(ROUTES)) expect(definition.template.startsWith("/v1/docs/") || definition.template === "/v1/ontology/schema").toBe(true);
      const source = await readFile(path.join(process.env.SO_MAIN_DIR ?? fileURLToPath(new URL("../../../electron/main/super-ontology", import.meta.url)), "control-client.mjs"), "utf8");
      expect(source).not.toMatch(/cloud:api-request|cloudEndpoint|process\.env/u);
    });
  });

  describe("errors", () => {
    it("reports not-running when there are no credentials", async () => {
      expect(await errorOf(createControlClient({ credentials: () => null }).request("health"))).toMatchObject({ code: "SO_ENGINE_NOT_RUNNING", retryable: true });
    });

    it("keeps code, retryable, status and only the allow-listed details; drops the message", async () => {
      server = await startContractServer({
        script: () => ({
          status: 412,
          body: { code: "DOCS_ENGINE_PRECONDITION_FAILED", message: `secret ${CONTRACT_TOKEN}`, retryable: false, details: { current_revision: 9, token: CONTRACT_TOKEN, stack: "x" } },
        }),
      });
      const error = await errorOf(make().request("driveUpdate", { params: { id: DRIVE_ID }, body: { paused: true }, revision: 1 }));
      expect(error).toMatchObject({ code: "DOCS_ENGINE_PRECONDITION_FAILED", retryable: false, httpStatus: 412, details: { current_revision: 9 } });
      expect(error.message).not.toContain("secret");
      expect(JSON.stringify(toErrorResult(error))).not.toContain(CONTRACT_TOKEN);
      expect(JSON.stringify(logs)).not.toContain(CONTRACT_TOKEN);
      expect(JSON.stringify(logs)).not.toContain("secret");
    });

    it("accepts the error object whether or not it is wrapped", async () => {
      server = await startContractServer({ script: () => ({ status: 404, body: { error: { code: "DOCS_ENGINE_NOT_FOUND", message: "m", retryable: false, details: {} } } }) });
      expect(await errorOf(make().request("driveGet", { params: { id: DRIVE_ID } }))).toMatchObject({ code: "DOCS_ENGINE_NOT_FOUND" });
    });

    it("retries a 429 once after Retry-After, then gives up", async () => {
      const limited = { status: 429, headers: { "Retry-After": "0" }, body: { code: "DOCS_ENGINE_RATE_LIMITED", message: "m", retryable: true, details: {} } };
      server = await startContractServer({ script: (_request, index) => (index === 1 ? limited : undefined) });
      expect((await make().request("drivesList")).status).toBe(200);
      expect(server.requests).toHaveLength(2);
      await server.close();
      server = await startContractServer({ script: () => limited });
      expect(await errorOf(make().request("drivesList"))).toMatchObject({ code: "DOCS_ENGINE_RATE_LIMITED", retryable: true });
      expect(server.requests).toHaveLength(2);
    });

    it("turns a slow response into SO_TIMEOUT (retryable) per call kind", async () => {
      server = await startContractServer({ script: () => ({ delayMs: 400, body: {} }) });
      const error = await errorOf(make({ timeouts: { read: 100, write: 100, compile: 100, share: 100 } }).request("drivesList"));
      expect(error).toMatchObject({ code: "SO_TIMEOUT", retryable: true });
      const write = await errorOf(make({ timeouts: { read: 100, write: 100, compile: 100, share: 100 } }).request("driveSync", { params: { id: DRIVE_ID }, body: {}, operationId: OP }));
      expect(write.code).toBe("SO_TIMEOUT");
    });

    it("uses the longer budgets of the spec: 10 s reads, 30 s writes, 60 s compile, 120 s shares", async () => {
      const { TIMEOUTS_MS } = await import("@so-main/control-client.mjs");
      expect(TIMEOUTS_MS).toEqual({ read: 10_000, write: 30_000, compile: 60_000, share: 120_000 });
      expect([ROUTES.drivesList.timeout, ROUTES.driveCreate.timeout, ROUTES.packCompile.timeout, ROUTES.sharesCreate.timeout]).toEqual(["read", "write", "compile", "share"]);
    });

    it("maps a refused connection to SO_ENGINE_UNREACHABLE (retryable)", async () => {
      server = await startContractServer();
      const closed = server;
      await closed.close();
      server = undefined;
      const client = createControlClient({ credentials: () => ({ baseUrl: closed.baseUrl, token }), timers: fastTimers });
      expect(await errorOf(client.request("drivesList"))).toMatchObject({ code: "SO_ENGINE_UNREACHABLE", retryable: true });
    });

    it("rejects a major version other than 1", async () => {
      server = await startContractServer({ script: () => ({ headers: { "OO-Docs-Api": "2.0" }, body: {} }) });
      expect(await errorOf(make().request("drivesList"))).toMatchObject({ code: "SO_ENGINE_INCOMPATIBLE" });
    });

    it("rejects non-JSON and oversized responses as SO_RESPONSE_INVALID", async () => {
      server = await startContractServer({ script: () => ({ raw: "<html>nope</html>" }) });
      expect(await errorOf(make().request("drivesList"))).toMatchObject({ code: "SO_RESPONSE_INVALID" });
      await server.close();
      server = await startContractServer({ script: () => ({ raw: `"${"x".repeat(9 * 1024 * 1024)}"` }) });
      expect(await errorOf(make().request("drivesList"))).toMatchObject({ code: "SO_RESPONSE_INVALID" });
      await server.close();
      server = await startContractServer({ script: () => ({ status: 500, raw: "boom" }) });
      expect(await errorOf(make().request("drivesList"))).toMatchObject({ code: "SO_RESPONSE_INVALID", httpStatus: 500 });
    });

    it("re-reads the token after a 401 and retries once; a persistent 401 is reported", async () => {
      server = await startContractServer();
      token = `oodt_${"C".repeat(43)}`;
      let reloads = 0;
      const rotated = make({ reloadCredentials: async () => { reloads += 1; token = CONTRACT_TOKEN; return true; } });
      expect((await rotated.request("drivesList")).status).toBe(200);
      expect(reloads).toBe(1);
      expect(server.requests).toHaveLength(2);

      token = `oodt_${"D".repeat(43)}`;
      const stuck = make({ reloadCredentials: async () => true });
      expect(await errorOf(stuck.request("drivesList"))).toMatchObject({ code: "DOCS_ENGINE_UNAUTHENTICATED" });
      expect(authFailures).toBe(1);
      expect(JSON.stringify(logs)).not.toContain("oodt_");
    });
  });

  describe("error details allow-list", () => {
    it("drops every key an error code does not list, for all codes in the contract", () => {
      for (const [code, keys] of Object.entries(DOCS_ERROR_DETAIL_KEYS)) {
        const input = { ...Object.fromEntries(keys.map((key) => [key, 1])), extra_key: "x", token: "y" };
        expect(Object.keys(pickDetails(code, input)).sort(), code).toEqual([...keys].sort());
      }
    });

    it("drops credential-looking and non-scalar values", () => {
      const [code] = Object.entries(DOCS_ERROR_DETAIL_KEYS).find(([, keys]) => keys.length > 0);
      const key = DOCS_ERROR_DETAIL_KEYS[code][0];
      expect(pickDetails(code, { [key]: "oodt_abc" })).toEqual({});
      expect(pickDetails(code, { [key]: { nested: true } })).toEqual({});
      expect(pickDetails(code, { [key]: ["a", "b"] })).toEqual({ [key]: ["a", "b"] });
      expect(pickDetails(code, "string")).toEqual({});
    });

    it("lets engine codes through the envelope but turns unknown codes into SO_INTERNAL", () => {
      expect(toErrorResult(new SuperOntologyError("DOCS_SOURCE_DRIVE_NOT_FOUND", { httpStatus: 404 })).error.code).toBe("DOCS_SOURCE_DRIVE_NOT_FOUND");
      expect(toErrorResult(new SuperOntologyError("DOCS_MADE_UP")).error.code).toBe("SO_INTERNAL");
    });
  });

  describe("channel handlers", () => {
    let lifecycle;
    let grants;
    let handlers;
    const sender = {};
    beforeEach(() => {
      lifecycle = { beginDriveChange: vi.fn() };
      grants = createGrantStore({ createId: () => "grant-0001-0001" });
    });
    const build = async (script) => {
      server = await startContractServer({ script });
      handlers = createControlHandlers({ client: make(), grants, lifecycle });
    };

    it("drives-list walks every page and returns plain Drive documents", async () => {
      const drive = (id) => ({ id, name: id, backend: "fs", config: { root: "/x" }, status: "active", revision: 1, counts: {}, created_at: "2026-11-17T00:00:00Z" });
      await build((request) => {
        const cursor = new URL(request.path, "http://x").searchParams.get("cursor");
        if (cursor === null) return { body: { items: [drive("a"), drive("b")], next_cursor: "c2", as_of_seq: 5 } };
        return { body: { items: [drive("c")], next_cursor: null, as_of_seq: 6 } };
      });
      expect((await handlers["drives-list"]({})).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
      expect(server.requests.map((request) => request.path)).toEqual(["/v1/docs/drives?limit=200", "/v1/docs/drives?limit=200&cursor=c2"]);
    });

    it("drive-create turns the grant into config.root, announces the Drive change once, and not on replay", async () => {
      await build();
      const { grantId } = grants.issue(sender, "drive-root", "/Users/example/Drive");
      const request = { operationId: OP, name: "Docs", rootGrantId: grantId, exclude: ["*.bak"], modelEnabled: false, llmDailyCalls: 100 };
      await handlers["drive-create"](request, { sender });
      expect(server.requests[0].body).toEqual({
        name: "Docs", backend: "fs", config: { root: "/Users/example/Drive", exclude: ["*.bak"] }, model_enabled: false, llm_daily_calls: 100,
      });
      expect(lifecycle.beginDriveChange).toHaveBeenCalledTimes(1);
      // A timed-out create is retried with the same operationId: the grant still works, the engine replays.
      const retried = await handlers["drive-create"](request, { sender });
      expect(retried.replayed).toBe(true);
      expect(lifecycle.beginDriveChange).toHaveBeenCalledTimes(1);
      await expect(handlers["drive-create"]({ ...request, operationId: "op_other_0123456789" }, { sender })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
    });

    it("a folder on this computer allows the model unless the request says otherwise", async () => {
      await build();
      const { grantId } = grants.issue(sender, "drive-root", "/Users/example/Drive");
      await handlers["drive-create"]({ operationId: OP, name: "Docs", rootGrantId: grantId }, { sender });
      expect(server.requests[0].body).toEqual({ name: "Docs", backend: "fs", config: { root: "/Users/example/Drive" }, model_enabled: true });
    });

    it("My files is created with the model allowed", async () => {
      server = await startContractServer({ script: (request) => (request.method === "GET" ? { body: { items: [], next_cursor: null, as_of_seq: 1 } } : undefined) });
      const noFs = { mkdir: async () => undefined };
      const mine = createControlHandlers({ client: make(), grants, lifecycle, managedFilesDir: "/Users/example/My files", fsPromises: noFs });
      await mine["my-files-ensure"]({ operationId: OP }).catch(() => undefined);
      const created = server.requests.find((request) => request.method === "POST");
      expect(created.body).toEqual({ name: "My files", backend: "fs", config: { root: "/Users/example/My files" }, model_enabled: true });
    });

    it("drive-create needs a grant from this window", async () => {
      await build();
      await expect(handlers["drive-create"]({ operationId: OP, name: "Docs", rootGrantId: "grant-9999-9999" }, { sender })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
      expect(server.requests).toHaveLength(0);
    });

    it("drive-update surfaces a 412 with the current revision", async () => {
      await build(() => ({ status: 412, body: { code: "DOCS_ENGINE_PRECONDITION_FAILED", message: "m", retryable: false, details: { current_revision: 12 } } }));
      await expect(handlers["drive-update"]({ driveId: DRIVE_ID, revision: 3, patch: { paused: true } })).rejects.toMatchObject({
        code: "DOCS_ENGINE_PRECONDITION_FAILED", details: { current_revision: 12 },
      });
    });

    it("drive-delete sends purge=false and announces the change only for a local folder Drive", async () => {
      await build(() => ({ body: { id: DRIVE_ID, name: "n", backend: "fs", config: {}, status: "deleting", revision: 4, counts: {}, created_at: "2026-11-17T00:00:00Z" } }));
      const deleted = await handlers["drive-delete"]({ driveId: DRIVE_ID, revision: 3 });
      expect(deleted.status).toBe("deleting");
      expect(server.requests[0].path).toBe(`/v1/docs/drives/${DRIVE_ID}?purge=false`);
      expect(lifecycle.beginDriveChange).toHaveBeenCalledTimes(1);
    });

    it("drive-sync passes deep and the mass-delete confirmation, and treats a running sync as success", async () => {
      await build();
      await handlers["drive-sync"]({ operationId: OP, driveId: DRIVE_ID, deep: true, confirmMassDelete: 3 });
      expect(server.requests[0].body).toEqual({ deep: true, confirm_mass_delete: 3 });
      await server.close();
      await build(() => ({ status: 409, body: { code: "DOCS_SOURCE_SYNC_RUNNING", message: "m", retryable: false, details: { generation: 4 } } }));
      const result = await handlers["drive-sync"]({ operationId: OP, driveId: DRIVE_ID });
      expect(result).toMatchObject({ status: "running" });
    });

    it("drive-tree and files-list build their queries and validate the shape", async () => {
      await build();
      const tree = await handlers["drive-tree"]({ driveId: DRIVE_ID, path: "Projects" });
      expect(Array.isArray(tree.dirs)).toBe(true);
      expect(server.requests[0].path).toBe(`/v1/docs/drives/${DRIVE_ID}/tree?path=Projects`);
      const page = await handlers["files-list"]({ driveId: DRIVE_ID, states: ["queued", "published"], sort: "state_since", limit: 500, cursor: "abc" });
      expect(page.items.length).toBeGreaterThan(0);
      expect(server.requests[1].path).toBe(`/v1/docs/drives/${DRIVE_ID}/files?state=queued%2Cpublished&sort=state_since&limit=500&cursor=abc`);
      await server.close();
      await build(() => ({ body: { items: "nope" } }));
      await expect(handlers["files-list"]({ driveId: DRIVE_ID })).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
    });

    it("the IPC envelope carries the replay flag and never a token", async () => {
      await build();
      const registered = new Map();
      registerSuperOntologyIpcHandlers({
        ipcMain: { handle: (channel, listener) => registered.set(channel, listener) },
        service: { handlers },
      });
      const { grantId } = grants.issue(sender, "drive-root", "/Users/example/Drive");
      const event = { sender };
      const body = { operationId: OP, name: "Docs", backend: "fs", rootGrantId: grantId };
      expect((await registered.get("super-ontology:drive-create")(event, body)).replayed).toBeUndefined();
      const replay = await registered.get("super-ontology:drive-create")(event, body);
      expect(replay).toMatchObject({ ok: true, replayed: true });
      expect(JSON.stringify(replay)).not.toContain("oodt_");
      const denied = await registered.get("super-ontology:drive-create")(event, { name: "x" });
      expect(denied).toEqual({ ok: false, error: { code: "SO_INVALID_REQUEST", retryable: false, details: {} } });
    });
  });
});
