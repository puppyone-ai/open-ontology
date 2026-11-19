import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createControlHandlers } from "@so-main/control-handlers.mjs";
import { createGrantStore } from "@so-main/grants.mjs";
import { createSecretFiles } from "@so-main/secret-files.mjs";
import { SuperOntologyError } from "@so-shared/errors.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

// F-S11-01: oss, cos and r2 Drives go through the same main-process path as s3, with the backend, the cloud fields and the policy.
const OP = "op_cloud_0123456789abcdef";
const SECRET = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const ACCOUNT = "0123456789abcdef0123456789abcdef";
let root;
let directory;
let calls;
beforeEach(() => { root = fs.mkdtempSync(path.join(os.tmpdir(), "sd-cloud-")); directory = path.join(root, "secrets"); });
afterEach(() => { fs.rmSync(root, { recursive: true, force: true }); });

const driveDoc = (backend, config) => ({ id: "d1", name: "Team", backend, config, status: "active", revision: 1, counts: {}, created_at: "2026-11-17T00:00:00Z" });
function handlers(respond) {
  calls = [];
  const client = { request: vi.fn(async (name, options) => { calls.push({ name, options }); return respond(name, options); }) };
  return createControlHandlers({ client, grants: createGrantStore(), lifecycle: { beginDriveChange: vi.fn() }, secretFiles: createSecretFiles({ directory, fsPromises: fs.promises }) });
}
const ok = (backend) => (_name, options) => ({ value: driveDoc(backend, options.body.config), replayed: false });
const create = (backend, extra) => parseSuperOntologyRequest("drive-create", { operationId: OP, name: "Team", backend, bucket: "team-docs", accessKeyId: "AKIAEXAMPLE", secretAccessKey: SECRET, ...extra });

describe("registering oss, cos and r2 (F-S11-01)", () => {
  it.each([
    ["oss", { region: "cn-hangzhou", internal: true }, { region: "cn-hangzhou", internal: true }],
    ["cos", { region: "ap-beijing" }, { region: "ap-beijing" }],
    ["r2", { accountId: ACCOUNT, pathStyle: true }, { account_id: ACCOUNT, path_style: true }],
  ])("%s: sends its backend and cloud fields, no endpoint, and key references only", async (backend, input, config) => {
    const h = handlers(ok(backend));
    const result = await h["drive-create"](create(backend, input), {});
    expect(calls[0].options.body).toMatchObject({ name: "Team", backend, config: { bucket: "team-docs", ...config } });
    expect(calls[0].options.body.config).not.toHaveProperty("endpoint");
    expect(JSON.stringify(calls[0].options.body)).not.toContain(SECRET);
    expect(calls[0].options.body.config.secret_access_key.secret_ref).toMatch(/^file:\//u);
    expect(result.value).toMatchObject({ backend });
    expect(fs.readdirSync(directory)).toHaveLength(2);
  });

  it("s3 still needs an endpoint, and an endpoint override of a cloud is held to the same https rule", async () => {
    const h = handlers(ok("s3"));
    await expect(h["drive-create"](create("s3", {}), {})).rejects.toMatchObject({ code: "DOCS_SOURCE_ENDPOINT_INVALID" });
    await expect(h["drive-create"](create("oss", { region: "cn-hangzhou", endpoint: "http://oss.example.com" }), {})).rejects.toMatchObject({ code: "DOCS_SOURCE_ENDPOINT_INVALID" });
    expect(calls).toHaveLength(0);
    await h["drive-create"](create("oss", { region: "cn-hangzhou", endpoint: "https://oss.example.com" }), {});
    expect(calls[0].options.body.config.endpoint).toBe("https://oss.example.com");
  });

  it("puts the access policy into config.policy for a bucket and for a folder, and leaves it out when empty", async () => {
    const h = handlers(ok("cos"));
    await h["drive-create"](create("cos", { region: "ap-beijing", allowPrefixes: ["Projects"], denyPrefixes: ["Projects/Payroll"] }), {});
    expect(calls[0].options.body.config.policy).toEqual({ allow: ["Projects"], deny: ["Projects/Payroll"] });
    await h["drive-create"](create("cos", { region: "ap-beijing", denyPrefixes: [] }), {});
    expect(calls[1].options.body.config).not.toHaveProperty("policy");

    const grants = createGrantStore();
    const sender = {};
    const grant = grants.issue(sender, "drive-root", "/Users/example/Docs");
    const client = { request: vi.fn(async (_name, options) => ({ value: driveDoc("fs", options.body.config), replayed: false })) };
    const folder = createControlHandlers({ client, grants, lifecycle: { beginDriveChange: vi.fn() }, secretFiles: null });
    await folder["drive-create"](parseSuperOntologyRequest("drive-create", { operationId: OP, name: "Docs", backend: "fs", rootGrantId: grant.grantId, denyPrefixes: ["Private"] }), { sender });
    expect(client.request.mock.calls[0][1].body.config).toEqual({ root: "/Users/example/Docs", policy: { deny: ["Private"] } });
  });

  it("removes the key files when the engine refuses a cloud Drive (a region it does not know), and deleting one removes them too", async () => {
    const refused = handlers(() => { throw new SuperOntologyError("DOCS_SOURCE_ENDPOINT_INVALID", { httpStatus: 400 }); });
    await expect(refused["drive-create"](create("oss", { region: "not a region" }), {})).rejects.toMatchObject({ code: "DOCS_SOURCE_ENDPOINT_INVALID" });
    expect(fs.readdirSync(directory)).toEqual([]);
    const h = handlers((name, options) => (name === "driveCreate" ? ok("r2")(name, options) : { value: driveDoc("r2", calls[0].options.body.config), replayed: false }));
    await h["drive-create"](create("r2", { accountId: ACCOUNT }), {});
    expect(fs.readdirSync(directory)).toHaveLength(2);
    await h["drive-delete"]({ driveId: "d1", revision: 1 });
    expect(fs.readdirSync(directory)).toEqual([]);
  });
});
