import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import https from "node:https";
import tls from "node:tls";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ROUTE_POLICY, readAdapterConfig, routeKind, startAdapter } from "./b01-adapter.mjs";

const example = JSON.parse(readFileSync(new URL("./b01-adapter.example.json", import.meta.url), "utf8"));
const exampleText = readFileSync(new URL("./b01-adapter.example.json", import.meta.url), "utf8");

test("splits selfbuild, identity, api, oidc, ws, assets, and pages", () => {
  assert.equal(routeKind("/control/v1/builds"), "selfbuild");
  assert.equal(routeKind("/control/v1/builds?project_id=alpha"), "selfbuild");
  assert.equal(routeKind("/control/v1/builds/b_0123456789abcdef0123456789abcdef"), "selfbuild");
  assert.equal(routeKind("/control/v1/reviews"), "selfbuild");
  assert.equal(routeKind("/control/v1/reviews/item-1"), "selfbuild");
  assert.equal(routeKind("/control/v1/classifications/preview"), "selfbuild");
  assert.equal(routeKind("/control/v1/identity/current"), "identity");
  assert.equal(routeKind("/control/v1/auth/login?return_to=%2Faccount"), "identity");
  assert.equal(routeKind("/control"), "identity");
  assert.equal(routeKind("/controller"), "page");
  assert.equal(routeKind("/v1/pack/metadata"), "api");
  assert.equal(routeKind("/api/health"), "api");
  assert.equal(routeKind("/.well-known/openid-configuration"), "oidc");
  assert.equal(routeKind("/oauth/authorize"), "oidc");
  assert.equal(routeKind("/ws"), "ws");
  assert.equal(routeKind("/login", { upgrade: "websocket" }), "ws");
  assert.equal(routeKind("/admin"), "page");
  assert.equal(routeKind("/admin/sources"), "page");
  assert.equal(routeKind("/fonts/NotoSansSC-400-subset.woff2"), "asset");
  assert.equal(routeKind("/assets/index.js"), "asset");
  assert.deepEqual(example.routing, ROUTE_POLICY);
});

test("the committed example has no origin and no destroyed proof host", () => {
  assert.equal(example.pageOrigin, null);
  assert.equal(example.listen.host, null);
  assert.equal(example.listen.port, null);
  assert.equal(example.identity.upstream, null);
  assert.equal(example.static.upstream, null);
  assert.equal(example.gateway.upstream, null);
  assert.equal(example.selfbuild.upstream, null);
  assert.equal(example.ws.upstream, null);
  for (const host of ["oo-fe01-edge", "oo-fe01-proof", "oo-fe01-idp", "oo-fe01-pg", "oo-fe01-m7-edge", "oo-fe01-m7-proof", "oo-fe01-m7-idp", "oo-fe01-m7-pg", "oo-fe01-m9-edge", "oo-fe01-m9-proof", "oo-fe01-m9-idp", "oo-fe01-m9-pg", "oo-fe01-m12-edge", "oo-fe01-m12-proof", "oo-fe01-m12-idp", "oo-fe01-m12-pg", "oo-fe01-m13-edge", "oo-fe01-m13-proof", "oo-fe01-m13-idp", "oo-fe01-m13-pg", "oo-fe01-m13-b04", "oo-fe01-m14-edge", "oo-fe01-m14-proof", "oo-fe01-m14-idp", "oo-fe01-m14-pg", "oo-fe01-m14-b04"]) {
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
  assert.equal(readAdapterConfig(base).listenHost, "127.0.0.1");
  assert.throws(() => readAdapterConfig({ ...base, listen: { port: 0 } }), /listen\.host is required/);
  assert.throws(() => readAdapterConfig({ ...base, identity: { ...base.identity, upstream: "http://identity.internal:7443" } }), /https/);
  assert.throws(() => readAdapterConfig({ ...base, rejectUnauthorized: false }), /bypass/);
  assert.throws(() => readAdapterConfig({ ...base, pageOrigin: "https://oo-fe01-edge:8443" }), /pageOrigin/);
  assert.throws(
    () => readAdapterConfig({ ...base, identity: { ...base.identity, upstream: "https://oo-fe01-proof:7443" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, identity: { ...base.identity, upstream: "https://oo-fe01-m9-proof:7443" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, identity: { ...base.identity, servername: "other.internal" } }),
    /servername/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, routing: { ...ROUTE_POLICY, spaFallback: "all" } }),
    /routing policy is fixed/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, gateway: { upstream: "https://oo-fe01-edge:7443", caFile: "/secrets/ca.crt" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, gateway: { upstream: "https://oo-fe01-m9-edge:7443", caFile: "/secrets/ca.crt" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, selfbuild: { upstream: "https://oo-fe01-m13-b04:7444", caFile: "/secrets/ca.crt" } }),
    /destroyed proof host/,
  );
  assert.throws(
    () => readAdapterConfig({ ...base, selfbuild: { upstream: "https://oo-fe01-m14-b04:7444", caFile: "/secrets/ca.crt" } }),
    /destroyed proof host/,
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
      const api = await getText(`https://127.0.0.1:${adapter.port}/v1/pack/metadata`, ca);
      assert.equal(api.status, 502);
      assert.equal(api.body, "api_unbound");
      const builds = await getText(`https://127.0.0.1:${adapter.port}/control/v1/builds?project_id=alpha`, ca);
      assert.equal(builds.status, 502);
      assert.equal(builds.body, "selfbuild_unbound");
      assert.equal(builds.body.includes("identity"), false);
      assert.equal(api.body.includes("static-index"), false);
      const oidc = await getText(`https://127.0.0.1:${adapter.port}/.well-known/openid-configuration`, ca);
      assert.equal(oidc.status, 502);
      assert.equal(oidc.body, "oidc_unbound");
      const login = await getText(`https://127.0.0.1:${adapter.port}/control/v1/auth/login`, ca);
      assert.equal(login.status, 200);
      assert.equal(login.body, "identity /control/v1/auth/login");
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

test("falls back to index.html only for page deep links", async () => {
  const dir = mkdtempSync(join(tmpdir(), "oo-b01-adapter-"));
  writeCerts(dir);
  const identity = await listenHttps(dir, (request, response) => {
    if (request.url === "/control/v1/auth/login") {
      response.writeHead(302, { location: "https://idp.example/auth" });
      response.end("go");
      return;
    }
    response.writeHead(200, { "content-type": "text/plain" });
    response.end("identity");
  });
  const pages = await listenPlain((request, response) => {
    if (request.url === "/index.html") {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end("spa-shell");
      return;
    }
    if (request.url.startsWith("/fonts/") || request.url.startsWith("/assets/")) {
      response.writeHead(404, { "content-type": "text/plain" });
      response.end("missing-asset");
      return;
    }
    response.writeHead(404, { "content-type": "text/plain" });
    response.end("missing-page");
  });
  const gateway = await listenPlain((request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(`gateway ${request.url}`);
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
      gateway: { upstream: `http://127.0.0.1:${gateway.port}` },
      routing: ROUTE_POLICY,
    }));
    try {
      const ca = readFileSync(join(dir, "ca.crt"));
      const admin = await getText(`https://127.0.0.1:${adapter.port}/admin/sources`, ca, { accept: "text/html" });
      assert.equal(admin.status, 200);
      assert.equal(admin.body, "spa-shell");
      assert.equal(admin.headers["cache-control"], "no-store");
      const raw = await getText(`https://127.0.0.1:${adapter.port}/admin`, ca, { accept: "application/json" });
      assert.equal(raw.status, 404);
      assert.equal(raw.body, "missing-page");
      const posted = await getText(`https://127.0.0.1:${adapter.port}/admin`, ca, { method: "POST", accept: "text/html" });
      assert.equal(posted.status, 404);
      assert.equal(posted.body, "missing-page");
      const font = await getText(`https://127.0.0.1:${adapter.port}/fonts/missing.woff2`, ca, { accept: "text/html" });
      assert.equal(font.status, 404);
      assert.equal(font.body, "missing-asset");
      const metadata = await getText(`https://127.0.0.1:${adapter.port}/v1/pack/metadata`, ca, { accept: "text/html" });
      assert.equal(metadata.status, 200);
      assert.equal(metadata.body, "gateway /v1/pack/metadata");
      assert.equal(metadata.body.includes("spa-shell"), false);
      const oidc = await getText(`https://127.0.0.1:${adapter.port}/.well-known/openid-configuration`, ca, { accept: "text/html" });
      assert.equal(oidc.status, 502);
      assert.equal(oidc.body, "oidc_unbound");
      const login = await getText(`https://127.0.0.1:${adapter.port}/control/v1/auth/login`, ca);
      assert.equal(login.status, 302);
      assert.equal(login.headers.location, "https://idp.example/auth");
      assert.equal(login.body, "go");
      const upgraded = await websocketText(adapter.port, ca);
      assert.match(upgraded, /^HTTP\/1\.1 502 /);
      assert.match(upgraded, /ws_unbound/);
      assert.equal(upgraded.includes("spa-shell"), false);
      assert.equal(upgraded.includes("<html"), false);
    } finally {
      await adapter.close();
    }
  } finally {
    await pages.close();
    await identity.close();
    await gateway.close();
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

function getText(url, ca, options = {}) {
  return new Promise((resolve, reject) => {
    const request = https.request(url, {
      ca,
      rejectUnauthorized: true,
      method: options.method || "GET",
      headers: options.accept ? { accept: options.accept } : undefined,
    }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({
        status: response.statusCode,
        headers: response.headers,
        body: Buffer.concat(chunks).toString("utf8"),
      }));
    });
    request.on("error", reject);
    request.end();
  });
}

function websocketText(port, ca) {
  return new Promise((resolve, reject) => {
    const socket = tls.connect({ port, host: "127.0.0.1", ca, rejectUnauthorized: true }, () => {
      socket.write("GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n");
    });
    const chunks = [];
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    socket.on("error", reject);
  });
}
