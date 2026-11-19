import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createDatabaseHandlers } from "../../../electron/main/super-ontology/database-handlers.mjs";
import { parseSuperOntologyRequest } from "../../../src/shared/super-ontology/requests.mjs";

const PEM = "-----BEGIN CERTIFICATE-----\nMIIBabcdefghijklmnop\n-----END CERTIFICATE-----";
const request = { kind: "postgres", name: "erp", host: "db.internal", port: 5432, database: "demo", user: "reader", password: "s3cret-pw", caPem: PEM, include: ["public.orders"] };
const roots = [];
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });

function setup(runner) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "so-db-"));
  roots.push(root);
  const calls = [];
  const run = async (file, args, options) => { calls.push({ file, args, ...options }); return runner(args, options, calls); };
  const handlers = createDatabaseHandlers({ lifecycle: { launchInfo: () => ({ ooPath: "/bin/oo", environment: { PATH: "/bin" } }) }, directory: path.join(root, "db"), secretsDirectory: path.join(root, "secrets"), stateDir: path.join(root, "state"), run });
  return { root, calls, handlers };
}
const ok = (stdout = "{}") => ({ code: 0, stdout, stderr: "" });
const listing = (...names) => ok(JSON.stringify({ sources: names.map((name) => ({ name, kind: "database", backend: "postgres", config: { host: "db.internal", port: 5432, database: "demo", user: "reader", include: ["public.orders"], tls_ca: true } })) }));

describe("database source handlers", () => {
  it("adds through `oo source add` with the password on stdin only, a 0600 trust anchor and the placeholder bootstrap", async () => {
    const { root, calls, handlers } = setup((args) => (args[1] === "ls" ? listing("erp") : ok()));
    const added = await handlers["db-source-add"](request);
    expect(added).toMatchObject({ name: "erp", host: "db.internal", tls: true });
    const add = calls.find((call) => call.args[1] === "add");
    expect(add.input).toBe("s3cret-pw");
    expect(add.args).toContain("--password-stdin");
    expect(JSON.stringify(calls.map((call) => call.args))).not.toContain("s3cret-pw");
    expect(add.env.OO_DOCS_SECRETS_DIR).toBe(path.join(root, "secrets"));
    expect(calls.some((call) => call.args[1] === "rm" && call.args[2] === "placeholder" && call.args.includes("--keep-secret"))).toBe(true);
    const ca = path.join(root, "secrets", "db-erp-ca.pem");
    expect(fs.statSync(ca).mode & 0o777).toBe(0o600);
    expect(add.args[add.args.indexOf("--tls-ca-file") + 1]).toBe(ca);
    expect(add.args.filter((arg) => arg === "--include")).toHaveLength(1);
  });

  it("removes the files it created when the CLI refuses, and maps the code", async () => {
    const { root, handlers } = setup(() => ({ code: 1, stdout: "", stderr: `${JSON.stringify({ error: { code: "SO_SOURCE_EXISTS", message: "x" } })}\n` }));
    await expect(handlers["db-source-add"](request)).rejects.toMatchObject({ code: "SO_DB_SOURCE_EXISTS" });
    expect(fs.existsSync(path.join(root, "secrets", "db-erp-ca.pem"))).toBe(false);
    expect(fs.existsSync(path.join(root, "db", "stack.yaml"))).toBe(false);
  });

  it("lists database sources only, hides the placeholder, and answers empty before the first add", async () => {
    const { handlers } = setup((args) => (args[1] === "ls" ? listing("placeholder", "erp") : ok()));
    expect(await handlers["db-sources-list"]()).toEqual({ items: [] });
    const { handlers: second, root } = setup((args) => (args[1] === "ls" ? listing("placeholder", "erp") : ok()));
    fs.mkdirSync(path.join(root, "db"), { recursive: true });
    fs.writeFileSync(path.join(root, "db", "stack.yaml"), "{}");
    expect((await second["db-sources-list"]()).items.map((item) => item.name)).toEqual(["erp"]);
  });

  it("validates the request: names, ports, certificates and scopes", () => {
    expect(parseSuperOntologyRequest("db-source-add", request)).toMatchObject({ name: "erp" });
    for (const bad of [{ name: "a b" }, { port: 0 }, { caPem: "not a certificate" }, { include: [] }, { include: ["orders"] }, { kind: "oracle" }, { password: "" }]) {
      expect(() => parseSuperOntologyRequest("db-source-add", { ...request, ...bad })).toThrow();
    }
  });
});

describe("removing the last database", () => {
  it("puts the placeholder back first, so the stack stays valid, and deletes the trust anchor", async () => {
    const { root, calls, handlers } = setup((args) => (args[1] === "ls" ? listing("erp") : ok()));
    fs.mkdirSync(path.join(root, "db"), { recursive: true });
    fs.mkdirSync(path.join(root, "secrets"), { recursive: true });
    fs.writeFileSync(path.join(root, "db", "stack.yaml"), JSON.stringify({ version: "oo.stack/v1", sources: [{ id: "erp" }] }));
    fs.writeFileSync(path.join(root, "secrets", "db-erp-ca.pem"), "x");
    expect(await handlers["db-source-remove"]({ name: "erp" })).toEqual({ removed: true });
    const stack = JSON.parse(fs.readFileSync(path.join(root, "db", "stack.yaml"), "utf8"));
    expect(stack.sources.map((source) => source.id)).toEqual(["erp", "placeholder"]);
    expect(calls.some((call) => call.args[1] === "rm" && call.args[2] === "erp")).toBe(true);
    expect(fs.existsSync(path.join(root, "secrets", "db-erp-ca.pem"))).toBe(false);
  });
});
