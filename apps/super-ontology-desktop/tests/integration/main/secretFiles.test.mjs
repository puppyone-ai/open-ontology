import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createControlHandlers, isAllowedEndpoint } from "@so-main/control-handlers.mjs";
import { createGrantStore } from "@so-main/grants.mjs";
import { createSecretFiles } from "@so-main/secret-files.mjs";
import { SuperOntologyError, toErrorResult } from "@so-shared/errors.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const OP = "op_s3_0123456789abcdef";
const SECRET = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
let root;
let directory;
beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "sd-secrets-"));
  directory = path.join(root, "secrets");
});
afterEach(() => { fs.rmSync(root, { recursive: true, force: true }); });
const mode = (file) => fs.statSync(file).mode & 0o777;
const refPath = (ref) => ref.secret_ref.slice("file:".length);

describe("secret files (F-S4-01)", () => {
  it("writes each key to its own 0600 file in a 0700 directory and returns only file: references", async () => {
    const secrets = createSecretFiles({ directory, fsPromises: fs.promises });
    const refs = await secrets.writeSet(OP, { access_key_id: "AKIAEXAMPLE", secret_access_key: SECRET, session_token: "token-1" });
    expect(Object.keys(refs)).toEqual(["access_key_id", "secret_access_key", "session_token"]);
    for (const ref of Object.values(refs)) {
      expect(ref.secret_ref.startsWith(`file:${directory}/`)).toBe(true);
      expect(mode(refPath(ref))).toBe(0o600);
    }
    expect(mode(directory)).toBe(0o700);
    expect(fs.readFileSync(refPath(refs.secret_access_key), "utf8")).toBe(SECRET);
    expect(JSON.stringify(refs)).not.toContain(SECRET);
  });

  it("skips an absent session token and repairs a directory that is too open", async () => {
    fs.mkdirSync(directory, { mode: 0o755 });
    fs.chmodSync(directory, 0o755);
    const refs = await createSecretFiles({ directory, fsPromises: fs.promises }).writeSet(OP, { access_key_id: "a", secret_access_key: "b" });
    expect(Object.keys(refs)).toEqual(["access_key_id", "secret_access_key"]);
    expect(mode(directory)).toBe(0o700);
  });

  it("writes the same files for the same operation id, so a retried request sends the same body", async () => {
    const secrets = createSecretFiles({ directory, fsPromises: fs.promises });
    const first = await secrets.writeSet(OP, { access_key_id: "a", secret_access_key: "b" });
    const second = await secrets.writeSet(OP, { access_key_id: "a", secret_access_key: "b2" });
    expect(second).toEqual(first);
    expect(fs.readdirSync(directory)).toHaveLength(2);
    expect(fs.readFileSync(refPath(second.secret_access_key), "utf8")).toBe("b2");
    const other = await secrets.writeSet("op_other_0123456789abc", { access_key_id: "a", secret_access_key: "b" });
    expect(other.access_key_id).not.toEqual(first.access_key_id);
  });

  it("removes only its own files when a Drive goes, and nothing outside the directory", async () => {
    const secrets = createSecretFiles({ directory, fsPromises: fs.promises });
    const refs = await secrets.writeSet(OP, { access_key_id: "a", secret_access_key: "b" });
    const outside = path.join(root, "keep.txt");
    fs.writeFileSync(outside, "keep");
    const removed = await secrets.removeConfigSecrets({ ...refs, session_token: { secret_ref: `file:${outside}` }, extra: 1 });
    expect(removed).toBe(2);
    expect(fs.readdirSync(directory)).toEqual([]);
    expect(fs.readFileSync(outside, "utf8")).toBe("keep");
    expect(await secrets.removeConfigSecrets({ access_key_id: { secret_ref: "env:OO_KEY" }, secret_access_key: { secret_ref: `file:${directory}/../keep.txt` } })).toBe(0);
    expect(fs.existsSync(outside)).toBe(true);
  });

  it("never puts a secret in an error: an unusable directory is SO_SECRET_STORE_UNAVAILABLE", async () => {
    fs.writeFileSync(directory, "a file where the directory should be");
    const secrets = createSecretFiles({ directory, fsPromises: fs.promises });
    const failure = await secrets.writeSet(OP, { access_key_id: "a", secret_access_key: SECRET }).catch((error) => error);
    expect(failure).toBeInstanceOf(SuperOntologyError);
    expect(failure.code).toBe("SO_SECRET_STORE_UNAVAILABLE");
    expect(JSON.stringify(toErrorResult(failure))).not.toContain(SECRET);
    await expect(createSecretFiles({ directory: undefined }).writeSet(OP, { access_key_id: "a", secret_access_key: "b" })).rejects.toMatchObject({ code: "SO_SECRET_STORE_UNAVAILABLE" });
  });

  it("removes the files already written when a later one fails", async () => {
    const real = fs.promises;
    let opened = 0;
    const flaky = { ...real, open: async (...args) => { opened += 1; if (opened === 2) throw new Error("disk full"); return real.open(...args); }, mkdir: real.mkdir, lstat: real.lstat, chmod: real.chmod, rename: real.rename, rm: real.rm };
    await expect(createSecretFiles({ directory, fsPromises: flaky }).writeSet(OP, { access_key_id: "a", secret_access_key: "b" })).rejects.toMatchObject({ code: "SO_SECRET_STORE_UNAVAILABLE" });
    expect(fs.readdirSync(directory)).toEqual([]);
  });
});

describe("object-store Drive registration in the main process", () => {
  const driveDoc = (config) => ({ id: "d1", name: "Team", backend: "s3", config, status: "active", revision: 1, counts: {}, created_at: "2026-11-17T00:00:00Z" });
  const request = { operationId: OP, name: "Team", backend: "s3", endpoint: "https://s3.example.com", region: "eu-west-1", bucket: "team-docs", prefix: "shared/", pathStyle: true, accessKeyId: "AKIAEXAMPLE", secretAccessKey: SECRET, exclude: ["*.bak"] };
  let secrets;
  let lifecycle;
  let calls;
  const handlersFor = (respond) => {
    calls = [];
    const client = { request: vi.fn(async (name, options) => { calls.push({ name, options }); return respond(name, options); }) };
    return createControlHandlers({ client, grants: createGrantStore(), lifecycle, secretFiles: secrets });
  };
  beforeEach(() => { secrets = createSecretFiles({ directory, fsPromises: fs.promises }); lifecycle = { beginDriveChange: vi.fn() }; });

  it("sends the engine secret references only, never a key, and does not restart the engine", async () => {
    const handlers = handlersFor((_name, options) => ({ value: driveDoc(options.body.config), replayed: false }));
    const result = await handlers["drive-create"](request, {});
    const body = calls[0].options.body;
    expect(body).toMatchObject({ name: "Team", backend: "s3", config: { endpoint: "https://s3.example.com", region: "eu-west-1", bucket: "team-docs", prefix: "shared/", path_style: true, exclude: ["*.bak"] } });
    expect(JSON.stringify(body)).not.toContain(SECRET);
    expect(JSON.stringify(body)).not.toContain("AKIAEXAMPLE");
    expect(body.config.access_key_id.secret_ref).toMatch(/^file:\//u);
    expect(body.config).not.toHaveProperty("session_token");
    expect(calls[0].options.operationId).toBe(OP);
    expect(JSON.stringify(result)).not.toContain(SECRET);
    expect(lifecycle.beginDriveChange).not.toHaveBeenCalled();
    expect(fs.readdirSync(directory)).toHaveLength(2);
  });

  it("removes the key files when the engine refuses, and keeps them when the outcome is unknown", async () => {
    const refused = handlersFor(() => { throw new SuperOntologyError("DOCS_SOURCE_AUTH_FAILED", { httpStatus: 400 }); });
    await expect(refused["drive-create"](request, {})).rejects.toMatchObject({ code: "DOCS_SOURCE_AUTH_FAILED" });
    expect(fs.readdirSync(directory)).toEqual([]);
    const unknown = handlersFor(() => { throw new SuperOntologyError("SO_TIMEOUT", { retryable: true }); });
    await expect(unknown["drive-create"](request, {})).rejects.toMatchObject({ code: "SO_TIMEOUT" });
    expect(fs.readdirSync(directory)).toHaveLength(2);
    const retried = handlersFor((_name, options) => ({ value: driveDoc(options.body.config), replayed: true }));
    await expect(retried["drive-create"](request, {})).resolves.toMatchObject({ replayed: true });
    expect(fs.readdirSync(directory)).toHaveLength(2);
  });

  it("reports field-level errors before anything is written", async () => {
    const handlers = handlersFor(() => ({ value: driveDoc({}), replayed: false }));
    await expect(handlers["drive-create"]({ ...request, endpoint: "http://s3.example.com" }, {})).rejects.toMatchObject({ code: "DOCS_SOURCE_ENDPOINT_INVALID" });
    await expect(handlers["drive-create"]({ ...request, endpoint: "not a url" }, {})).rejects.toMatchObject({ code: "DOCS_SOURCE_ENDPOINT_INVALID" });
    await expect(handlers["drive-create"]({ ...request, accessKeyId: undefined }, {})).rejects.toMatchObject({ code: "DOCS_SOURCE_CREDENTIAL_MISSING" });
    await expect(handlers["drive-create"]({ ...request, bucket: undefined }, {})).rejects.toMatchObject({ code: "DOCS_SOURCE_BUCKET_NOT_FOUND" });
    expect(calls).toHaveLength(0);
    expect(fs.existsSync(directory)).toBe(false);
  });

  it("accepts https anywhere and http only for this computer", () => {
    for (const endpoint of ["https://s3.example.com", "https://s3.example.com:9000/", "http://localhost:9000", "http://127.0.0.1:9000", "http://[::1]:9000"]) expect(isAllowedEndpoint(endpoint), endpoint).toBe(true);
    for (const endpoint of ["http://s3.example.com", "ftp://x", "https://user:pw@s3.example.com", "s3.example.com", ""]) expect(isAllowedEndpoint(endpoint), endpoint).toBe(false);
  });

  it("deleting an object-store Drive removes its key files; a folder Drive has none", async () => {
    const handlers = handlersFor((name, options) => {
      if (name === "driveCreate") return { value: driveDoc(options.body.config), replayed: false };
      return { value: driveDoc(calls[0].options.body.config), replayed: false };
    });
    await handlers["drive-create"](request, {});
    expect(fs.readdirSync(directory)).toHaveLength(2);
    await handlers["drive-delete"]({ driveId: "d1", revision: 1 });
    expect(fs.readdirSync(directory)).toEqual([]);
  });

  it("validates the channel request: the keys are optional text, unknown fields are still refused", () => {
    const parsed = parseSuperOntologyRequest("drive-create", request);
    expect(parsed).toMatchObject({ backend: "s3", bucket: "team-docs", secretAccessKey: SECRET });
    expect(() => parseSuperOntologyRequest("drive-create", { ...request, backend: "gcs" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { ...request, extra: 1 })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { ...request, bucket: "ab" })).toThrow();
  });
});
