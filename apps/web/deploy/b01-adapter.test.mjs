import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import https from "node:https";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { readAdapterConfig, routeKind, startAdapter } from "./b01-adapter.mjs";

const example = JSON.parse(readFileSync(new URL("./b01-adapter.example.json", import.meta.url), "utf8"));
const exampleText = readFileSync(new URL("./b01-adapter.example.json", import.meta.url), "utf8");

test("routes only the control prefix to identity", () => {
  assert.equal(routeKind("/control/v1/identity/current"), "identity");
  assert.equal(routeKind("/control/v1/auth/login?return_to=%2Faccount"), "identity");
  assert.equal(routeKind("/control"), "static");
  assert.equal(routeKind("/admin"), "static");
  assert.equal(routeKind("/fonts/NotoSansSC-400-subset.woff2"), "static");
});

test("the committed example has no origin and no destroyed proof host", () => {
  assert.equal(example.pageOrigin, null);
  assert.equal(example.identity.upstream, null);
  assert.equal(example.static.upstream, null);
  for (const host of ["oo-fe01-edge", "oo-fe01-proof", "oo-fe01-idp", "oo-fe01-pg"]) {
    assert.equal(exampleText.includes(host), false);
  }
  assert.throws(() => readAdapterConfig(example), /identity\.upstream is required/);
});

test("refuses plaintext identity, certificate bypass, and destroyed hosts", () => {
  const base = {
    pageOrigin: null,
    listen: { host: "127.0.0.1", port: 0 },
    identity: { upstream: "https://identity.internal:7443", caFile: "/secrets/ca.crt", servername: "identity.internal" },
    static: { upstream: "http://127.0.0.1:4173" },
  };
  assert.equal(readAdapterConfig(base).pageOrigin, null);
  assert.throws(() => readAdapterConfig({ ...base, identity: { ...base.identity, upstream: "http://identity.internal:7443" } }), /https/);
  assert.throws(() => readAdapterConfig({ ...base, rejectUnauthorized: false }), /bypass/);
  assert.throws(() => readAdapterConfig({ ...base, pageOrigin: "https://oo-fe01-edge:8443" }), /pageOrigin/);
  assert.throws(
    () => readAdapterConfig({ ...base, identity: { ...base.identity, upstream: "https://oo-fe01-proof:7443" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, identity: { ...base.identity, servername: "other.internal" } }),
    /servername/,
  );
});

test("forwards /control/ to identity and does not fall through to the static page", async () => {
  const dir = mkdtempSync(join(tmpdir(), "oo-b01-adapter-"));
  writeCerts(dir);
  const identity = await listenHttps(dir, (request, response) => {
    response.writeHead(200, { "content-type": "text/plain" });
    response.end(`identity ${request.url}`);
  });
  const pages = await listenPlain((request, response) => {
    response.writeHead(200, { "content-type": "text/html" });
    response.end("static-index");
  });
  try {
    const adapter = await startAdapter(readAdapterConfig({
      pageOrigin: null,
      listen: { host: "127.0.0.1", port: 0 },
      tls: { certFile: join(dir, "adapter.crt"), keyFile: join(dir, "adapter.key") },
      identity: {
        upstream: `https://127.0.0.1:${identity.port}`,
        caFile: join(dir, "ca.crt"),
        servername: "127.0.0.1",
      },
      static: { upstream: `http://127.0.0.1:${pages.port}` },
    }));
    try {
      const ca = readFileSync(join(dir, "ca.crt"));
      const current = await getText(`https://127.0.0.1:${adapter.port}/control/v1/identity/current`, ca);
      assert.equal(current.status, 200);
      assert.equal(current.body, "identity /control/v1/identity/current");
      const page = await getText(`https://127.0.0.1:${adapter.port}/admin`, ca);
      assert.equal(page.status, 200);
      assert.equal(page.body, "static-index");
      await identity.close();
      const down = await getText(`https://127.0.0.1:${adapter.port}/control/v1/auth/login`, ca);
      assert.equal(down.status, 502);
      assert.equal(down.body, "identity_unreachable");
      assert.equal(down.body.includes("static-index"), false);
    } finally {
      await adapter.close();
    }
  } finally {
    await pages.close();
    await identity.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

function writeCerts(dir) {
  const run = (args) => {
    const result = spawnSync("openssl", args, { stdio: "pipe", cwd: dir });
    if (result.status !== 0) {
      throw new Error(result.stderr.toString() || "openssl failed");
    }
  };
  run(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", join(dir, "ca.key"), "-out", join(dir, "ca.crt"), "-days", "2", "-subj", "/CN=oo-b01-adapter-test", "-addext", "basicConstraints=critical,CA:TRUE"]);
  for (const name of ["adapter", "identity"]) {
    const ext = join(dir, `${name}.ext`);
    writeFileSync(ext, "subjectAltName=IP:127.0.0.1\n");
    run(["req", "-newkey", "rsa:2048", "-nodes", "-keyout", join(dir, `${name}.key`), "-out", join(dir, `${name}.csr`), "-subj", `/CN=${name}`]);
    run(["x509", "-req", "-in", `${name}.csr`, "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial", "-out", `${name}.crt`, "-days", "2", "-extfile", `${name}.ext`]);
  }
}

function listenHttps(dir, handler) {
  const server = https.createServer(
    {
      cert: readFileSync(join(dir, "identity.crt")),
      key: readFileSync(join(dir, "identity.key")),
    },
    handler,
  );
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({
        port: server.address().port,
        close: () => new Promise((done) => server.close(() => done())),
      });
    });
  });
}

function listenPlain(handler) {
  const server = http.createServer(handler);
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({
        port: server.address().port,
        close: () => new Promise((done) => server.close(() => done())),
      });
    });
  });
}

function getText(url, ca) {
  return new Promise((resolve, reject) => {
    const request = https.get(url, { ca, rejectUnauthorized: true }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({ status: response.statusCode, body: Buffer.concat(chunks).toString("utf8") }));
    });
    request.on("error", reject);
  });
}
