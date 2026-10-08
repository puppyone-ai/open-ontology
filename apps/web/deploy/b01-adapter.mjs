import fs from "node:fs";
import http from "node:http";
import https from "node:https";
import { pathToFileURL } from "node:url";

const DESTROYED_HOSTS = new Set(["oo-fe01-edge", "oo-fe01-proof", "oo-fe01-idp", "oo-fe01-pg"]);

export function routeKind(urlPath) {
  const path = (urlPath || "/").split("?")[0];
  return path.startsWith("/control/") ? "identity" : "static";
}

export function readAdapterConfig(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("config must be an object");
  }
  if (value.rejectUnauthorized === false) {
    throw new Error("certificate bypass is refused");
  }
  if (value.pageOrigin !== null) {
    throw new Error("pageOrigin stays null; the browser uses the page origin");
  }
  const identityUrl = httpsUrl(value.identity?.upstream, "identity.upstream");
  const staticUrl = httpUrl(value.static?.upstream, "static.upstream");
  refuseDestroyed(identityUrl.hostname);
  refuseDestroyed(staticUrl.hostname);
  if (typeof value.identity?.caFile !== "string" || value.identity.caFile.length === 0) {
    throw new Error("identity.caFile is required");
  }
  if (value.identity.servername != null && value.identity.servername !== identityUrl.hostname) {
    throw new Error("identity.servername must match the upstream host");
  }
  const port = value.listen?.port;
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error("listen.port is required");
  }
  return {
    listenHost: value.listen?.host ?? "127.0.0.1",
    listenPort: port,
    certFile: value.tls?.certFile,
    keyFile: value.tls?.keyFile,
    pageOrigin: null,
    identity: {
      url: identityUrl,
      caFile: value.identity.caFile,
      servername: identityUrl.hostname,
    },
    static: { url: staticUrl },
  };
}

export function startAdapter(config) {
  if (typeof config.certFile !== "string" || typeof config.keyFile !== "string") {
    throw new Error("tls certFile and keyFile are required");
  }
  const ca = fs.readFileSync(config.identity.caFile);
  const agent = new https.Agent({
    ca,
    keepAlive: true,
    servername: config.identity.servername,
    rejectUnauthorized: true,
  });
  const server = https.createServer(
    {
      cert: fs.readFileSync(config.certFile),
      key: fs.readFileSync(config.keyFile),
    },
    (request, response) => {
      const path = request.url || "/";
      const kind = routeKind(path);
      const target = kind === "identity" ? config.identity.url : config.static.url;
      const lib = target.protocol === "https:" ? https : http;
      const headers = { ...request.headers, host: target.host };
      const upstream = lib.request(
        {
          protocol: target.protocol,
          hostname: target.hostname,
          port: target.port,
          method: request.method,
          path,
          headers,
          agent: kind === "identity" ? agent : undefined,
          servername: kind === "identity" ? config.identity.servername : undefined,
          rejectUnauthorized: true,
        },
        (incoming) => {
          response.writeHead(incoming.statusCode || 502, incoming.headers);
          incoming.pipe(response);
        },
      );
      upstream.on("error", () => {
        if (!response.headersSent) {
          response.writeHead(502, { "content-type": "text/plain; charset=utf-8" });
        }
        response.end(kind === "identity" ? "identity_unreachable" : "static_unreachable");
      });
      request.pipe(upstream);
    },
  );
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(config.listenPort, config.listenHost, () => {
      const address = server.address();
      resolve({
        port: typeof address === "object" && address ? address.port : config.listenPort,
        close() {
          agent.destroy();
          return new Promise((done) => server.close(() => done()));
        },
      });
    });
  });
}

function httpsUrl(value, label) {
  const url = parseUrl(value, label);
  if (url.protocol !== "https:") throw new Error(`${label} must be https`);
  bareUrl(url, label);
  return url;
}

function httpUrl(value, label) {
  const url = parseUrl(value, label);
  if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error(`${label} must be an http URL`);
  bareUrl(url, label);
  return url;
}

function parseUrl(value, label) {
  if (typeof value !== "string" || value.length === 0) throw new Error(`${label} is required`);
  try {
    return new URL(value);
  } catch {
    throw new Error(`${label} must be a URL`);
  }
}

function bareUrl(url, label) {
  if (url.username || url.password || url.search || url.hash) {
    throw new Error(`${label} must not carry userinfo or a query`);
  }
}

function refuseDestroyed(hostname) {
  if (DESTROYED_HOSTS.has(hostname)) {
    throw new Error(`destroyed proof host ${hostname} is not a deploy default`);
  }
}

const invoked = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (invoked) {
  const file = process.env.OO_B01_ADAPTER_CONFIG;
  if (!file) throw new Error("OO_B01_ADAPTER_CONFIG is required");
  const config = readAdapterConfig(JSON.parse(fs.readFileSync(file, "utf8")));
  await startAdapter(config);
}
