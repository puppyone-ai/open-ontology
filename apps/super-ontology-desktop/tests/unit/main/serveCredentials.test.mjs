import { chmod, mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readServeCredentials } from "@so-main/serve-credentials.mjs";

const TOKEN = `oodt_${"A".repeat(43)}`;

describe("readServeCredentials", () => {
  let dir;
  beforeEach(async () => {
    dir = await mkdtemp(path.join(os.tmpdir(), "so-serve-"));
    await chmod(dir, 0o700);
  });
  afterEach(async () => { await rm(dir, { recursive: true, force: true }); });

  async function write({ document = {}, token = `${TOKEN}\n`, jsonMode = 0o600, tokenMode = 0o600 } = {}) {
    const tokenFile = path.join(dir, "serve.token");
    await writeFile(tokenFile, token, { mode: tokenMode });
    await chmod(tokenFile, tokenMode);
    await writeFile(path.join(dir, "serve.json"), JSON.stringify({
      protocol: "oo.docs.serve/v1", pid: 4242, port: 51234, api_version: "1.0", token_file: tokenFile,
      state_dir: dir, started_at: "2026-11-17T00:00:00Z", ...document,
    }), { mode: jsonMode });
    await chmod(path.join(dir, "serve.json"), jsonMode);
  }
  const invalid = { code: "SO_SERVE_CREDENTIALS_INVALID" };

  it("returns null while no instance has written serve.json", async () => {
    expect(await readServeCredentials({ stateDir: dir })).toBeNull();
  });

  it("reads port, versions and the token from a private, well-formed pair", async () => {
    await write();
    expect(await readServeCredentials({ stateDir: dir })).toEqual({
      info: { pid: 4242, port: 51234, apiMajor: 1, apiVersion: "1.0", startedAt: "2026-11-17T00:00:00Z" },
      token: TOKEN,
    });
  });

  it("rejects a relative state directory", async () => {
    await expect(readServeCredentials({ stateDir: "state" })).rejects.toMatchObject(invalid);
  });

  it("rejects loose permissions on serve.json and on the token file", async () => {
    await write({ jsonMode: 0o644 });
    await expect(readServeCredentials({ stateDir: dir })).rejects.toMatchObject(invalid);
    await write({ tokenMode: 0o640 });
    await expect(readServeCredentials({ stateDir: dir })).rejects.toMatchObject(invalid);
  });

  it("rejects a symlinked serve.json and a symlinked token file", async () => {
    await write();
    await rm(path.join(dir, "serve.json"));
    await writeFile(path.join(dir, "real.json"), "{}", { mode: 0o600 });
    await symlink(path.join(dir, "real.json"), path.join(dir, "serve.json"));
    await expect(readServeCredentials({ stateDir: dir })).rejects.toMatchObject(invalid);

    await rm(path.join(dir, "serve.json"));
    await write();
    await rm(path.join(dir, "serve.token"));
    await writeFile(path.join(dir, "elsewhere.token"), TOKEN, { mode: 0o600 });
    await symlink(path.join(dir, "elsewhere.token"), path.join(dir, "serve.token"));
    await expect(readServeCredentials({ stateDir: dir })).rejects.toMatchObject(invalid);
  });

  it("rejects a file owned by someone else", async () => {
    await write();
    await expect(readServeCredentials({ stateDir: dir, getUid: () => 0x7ffffff0 })).rejects.toMatchObject(invalid);
  });

  it.each([
    ["foreign protocol", { document: { protocol: "other/v1" } }],
    ["api major 2 still parses but a missing version does not", { document: { api_version: "x" } }],
    ["port 0", { document: { port: 0 } }],
    ["port too large", { document: { port: 70000 } }],
    ["non-integer port", { document: { port: "80" } }],
    ["relative token path", { document: { token_file: "serve.token" } }],
    ["token outside the state directory", { document: { token_file: path.join(os.tmpdir(), "x.token") } }],
    ["malformed token", { token: "oodt_short" }],
    ["wrong token prefix", { token: `oodr_${"A".repeat(43)}` }],
  ])("rejects %s", async (_name, options) => {
    await write(options);
    await expect(readServeCredentials({ stateDir: dir })).rejects.toMatchObject(invalid);
  });

  it("reports the major api version so the caller can refuse an incompatible engine", async () => {
    await write({ document: { api_version: "2.1" } });
    expect((await readServeCredentials({ stateDir: dir })).info.apiMajor).toBe(2);
  });

  it("never puts the token into the error", async () => {
    await write({ jsonMode: 0o644 });
    const error = await readServeCredentials({ stateDir: dir }).catch((caught) => caught);
    expect(JSON.stringify(error)).not.toContain(TOKEN);
    expect(String(error.message)).not.toContain(TOKEN);
  });
});

describe("serve log rotation", () => {
  it("rotates at the size limit and keeps three files", async () => {
    const { createServeLog } = await import("@so-main/serve-log.mjs");
    const directory = await mkdtemp(path.join(os.tmpdir(), "so-log-"));
    try {
      const log = createServeLog({ directory, maxBytes: 10, keep: 3 });
      for (let round = 1; round <= 5; round += 1) {
        const handle = await log.open();
        await handle.writeFile(`${round}`.repeat(12));
        await handle.close();
      }
      const { readdir, readFile } = await import("node:fs/promises");
      expect((await readdir(directory)).sort()).toEqual([
        "super-ontology-serve.log", "super-ontology-serve.log.1", "super-ontology-serve.log.2", "super-ontology-serve.log.3",
      ]);
      expect(await readFile(path.join(directory, "super-ontology-serve.log.1"), "utf8")).toBe("4".repeat(12));
      expect(await readFile(path.join(directory, "super-ontology-serve.log.3"), "utf8")).toBe("2".repeat(12));
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
