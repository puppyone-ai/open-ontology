import fs from "node:fs";
import http from "node:http";
import https from "node:https";
import net from "node:net";
import { pathToFileURL } from "node:url";

const DESTROYED_HOSTS = new Set(["oo-fe01-edge", "oo-fe01-proof", "oo-fe01-idp", "oo-fe01-pg", "oo-fe01-m7-edge", "oo-fe01-m7-proof", "oo-fe01-m7-idp", "oo-fe01-m7-pg", "oo-fe01-m9-edge", "oo-fe01-m9-proof", "oo-fe01-m9-idp", "oo-fe01-m9-pg", "oo-fe01-m12-edge", "oo-fe01-m12-proof", "oo-fe01-m12-idp", "oo-fe01-m12-pg", "oo-fe01-m13-edge", "oo-fe01-m13-proof", "oo-fe01-m13-idp", "oo-fe01-m13-pg", "oo-fe01-m13-b04"]);

export const ROUTE_POLICY = {
  selfbuild: [
    "/control/v1/builds",
    "/control/v1/builds/",
    "/control/v1/reviews",
    "/control/v1/reviews/",
    "/control/v1/classifications",
    "/control/v1/classifications/",
  ],
  identity: ["/control", "/control/"],
  api: ["/v1", "/v1/", "/api", "/api/"],
  oidc: ["/.well-known", "/.well-known/", "/oauth", "/oauth/", "/oidc", "/oidc/"],
  ws: ["/ws", "/ws/"],
  assets: ["/fonts/", "/assets/"],
  spaFallback: "page-deep-link-only",
};

const ASSET_FILE = /\.(?:js|mjs|css|map|woff2?|ttf|otf|png|jpe?g|gif|svg|ico|webp|webmanifest|txt|xml|html)$/i;

export function routeKind(urlPath, headers) {
  const path = (urlPath || "/").split("?")[0];
  const upgrade = headerValue(headers, "upgrade").toLowerCase();
  if (upgrade === "websocket" || matchesAny(path, ROUTE_POLICY.ws)) return "ws";
  if (matchesAny(path, ROUTE_POLICY.selfbuild)) return "selfbuild";
  if (matchesAny(path, ROUTE_POLICY.identity)) return "identity";
  if (matchesAny(path, ROUTE_POLICY.api)) return "api";
  if (matchesAny(path, ROUTE_POLICY.oidc)) return "oidc";
  if (matchesAny(path, ROUTE_POLICY.assets) || ASSET_FILE.test(path.slice(path.lastIndexOf("/") + 1))) return "asset";
  return "page";
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
  if (value.routing != null && JSON.stringify(value.routing) !== JSON.stringify(ROUTE_POLICY)) {
    throw new Error("routing policy is fixed");
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
  const gateway = optionalUpstream(value.gateway, "gateway");
  const selfbuild = optionalUpstream(value.selfbuild, "selfbuild");
  const ws = optionalUpstream(value.ws, "ws");
  const port = value.listen?.port;
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error("listen.port is required");
  }
  const host = value.listen?.host;
  if (typeof host !== "string" || host.length === 0) {
    throw new Error("listen.host is required");
  }
  return {
    listenHost: host,
    listenPort: port,
    certFile: value.tls?.certFile,
    keyFile: value.tls?.keyFile,
    pageOrigin: null,
    routing: ROUTE_POLICY,
    identity: {
      url: identityUrl,
      caFile: value.identity.caFile,
      servername: identityUrl.hostname,
    },
    static: { url: staticUrl },
    gateway,
    selfbuild,
    ws,
  };
}

export function startAdapter(config) {
  if (typeof config.certFile !== "string" || typeof config.keyFile !== "string") {
    throw new Error("tls certFile and keyFile are required");
  }
  const identityAgent = tlsAgent(config.identity);
  const gatewayAgent = config.gateway?.url.protocol === "https:" ? tlsAgent(config.gateway) : null;
  const selfbuildAgent = config.selfbuild?.url.protocol === "https:" ? tlsAgent(config.selfbuild) : null;
  const wsAgent = config.ws?.url.protocol === "https:" ? tlsAgent(config.ws) : null;
  const server = https.createServer(
    {
      cert: fs.readFileSync(config.certFile),
      key: fs.readFileSync(config.keyFile),
    },
    (request, response) => {
      try {
        dispatch(request, response, config, { identityAgent, gatewayAgent, selfbuildAgent });
      } catch {
        sendPlain(request, response, 502, "adapter_error");
      }
    },
  );
  server.on("upgrade", (request, socket) => {
    if (!config.ws) {
      refuseUpgrade(socket);
      return;
    }
    proxyUpgrade(request, socket, config.ws, wsAgent);
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(config.listenPort, config.listenHost, () => {
      const address = server.address();
      resolve({
        port: typeof address === "object" && address ? address.port : config.listenPort,
        close() {
          identityAgent.destroy();
          gatewayAgent?.destroy();
          selfbuildAgent?.destroy();
          wsAgent?.destroy();
          server.closeAllConnections?.();
          return new Promise((done) => server.close(() => done()));
        },
      });
    });
  });
}

function dispatch(request, response, config, agents) {
  const path = request.url || "/";
  const kind = routeKind(path, request.headers);
  if (kind === "selfbuild") {
    if (!config.selfbuild) return sendPlain(request, response, 502, "selfbuild_unbound");
    return proxy(request, response, config.selfbuild, agents.selfbuildAgent, "selfbuild_unreachable");
  }
  if (kind === "api") {
    if (!config.gateway) return sendPlain(request, response, 502, "api_unbound");
    return proxy(request, response, config.gateway, agents.gatewayAgent, "api_unreachable");
  }
  if (kind === "oidc") return sendPlain(request, response, 502, "oidc_unbound");
  if (kind === "ws") {
    if (!config.ws) return sendPlain(request, response, 502, "ws_unbound");
    return proxy(request, response, config.ws, agents.wsAgent, "ws_unreachable");
  }
  if (kind === "identity") return proxy(request, response, config.identity, agents.identityAgent, "identity_unreachable");
  return proxy(request, response, config.static, undefined, "static_unreachable", kind === "page");
}

function proxy(request, response, target, agent, errorBody, spaFallback) {
  const upstream = openUpstream(request, target, target.url.protocol === "https:" ? agent : undefined, request.url || "/");
  upstream.on("response", (incoming) => {
    if (spaFallback && incoming.statusCode === 404 && isDocument(request)) {
      incoming.resume();
      const index = openUpstream(request, target, target.url.protocol === "https:" ? agent : undefined, "/index.html", request.method || "GET");
      index.on("response", (page) => {
        const headers = { ...page.headers, "cache-control": "no-store" };
        response.writeHead(page.statusCode || 502, headers);
        page.pipe(response);
      });
      index.on("error", () => sendPlain(request, response, 502, errorBody));
      index.end();
      return;
    }
    response.writeHead(incoming.statusCode || 502, incoming.headers);
    incoming.pipe(response);
  });
  upstream.on("error", () => sendPlain(request, response, 502, errorBody));
  request.pipe(upstream);
}

function openUpstream(request, target, agent, path, method) {
  const headers = { ...request.headers, host: target.url.host };
  if (path === "/index.html") {
    delete headers["content-length"];
  }
  const lib = target.url.protocol === "https:" ? https : http;
  const servername = target.url.protocol === "https:" ? sniName(target.servername) : undefined;
  return lib.request({
    protocol: target.url.protocol,
    hostname: target.url.hostname,
    port: target.url.port,
    method: method || request.method,
    path,
    headers,
    agent,
    servername,
    rejectUnauthorized: true,
  });
}

function proxyUpgrade(request, socket, target, agent) {
  const headers = { ...request.headers, host: target.url.host };
  const lib = target.url.protocol === "https:" ? https : http;
  const servername = target.url.protocol === "https:" ? sniName(target.servername) : undefined;
  const upstream = lib.request({
    protocol: target.url.protocol,
    hostname: target.url.hostname,
    port: target.url.port,
    method: request.method || "GET",
    path: request.url || "/",
    headers,
    agent,
    servername,
    rejectUnauthorized: true,
  });
  upstream.on("upgrade", (incoming, upSocket, head) => {
    const lines = [`HTTP/1.1 ${incoming.statusCode || 101} ${incoming.statusMessage || "Switching Protocols"}`];
    for (const [name, value] of Object.entries(incoming.headers)) {
      const values = Array.isArray(value) ? value : [value];
      for (const item of values) {
        if (item != null) lines.push(`${name}: ${item}`);
      }
    }
    socket.write(`${lines.join("\r\n")}\r\n\r\n`);
    if (head.length) socket.write(head);
    upSocket.pipe(socket);
    socket.pipe(upSocket);
  });
  upstream.on("response", (incoming) => {
    const type = headerValue(incoming.headers, "content-type");
    incoming.resume();
    if (type.includes("text/html")) {
      refuseUpgrade(socket);
      return;
    }
    refuseUpgrade(socket, "ws_unreachable");
  });
  upstream.on("error", () => refuseUpgrade(socket));
  upstream.end();
}

function tlsAgent(target) {
  const servername = sniName(target?.servername);
  return new https.Agent({
    ca: target?.caFile ? fs.readFileSync(target.caFile) : undefined,
    keepAlive: true,
    servername,
    rejectUnauthorized: true,
  });
}

function sniName(value) {
  if (typeof value !== "string" || value.length === 0 || net.isIP(value)) return undefined;
  return value;
}

function optionalUpstream(section, label) {
  if (section == null || section.upstream == null) return null;
  const url = httpUrl(section.upstream, `${label}.upstream`);
  refuseDestroyed(url.hostname);
  if (url.protocol === "https:") {
    if (typeof section.caFile !== "string" || section.caFile.length === 0) {
      throw new Error(`${label}.caFile is required`);
    }
    if (section.servername != null && section.servername !== url.hostname) {
      throw new Error(`${label}.servername must match the upstream host`);
    }
  }
  return {
    url,
    caFile: url.protocol === "https:" ? section.caFile : null,
    servername: url.hostname,
  };
}

function sendPlain(request, response, status, body) {
  request.resume();
  if (response.headersSent) {
    response.end();
    return;
  }
  const payload = Buffer.from(body);
  response.writeHead(status, {
    "content-type": "text/plain; charset=utf-8",
    "content-length": String(payload.length),
    "cache-control": "no-store",
  });
  response.end(payload);
}

function refuseUpgrade(socket, body = "ws_unbound") {
  if (socket.destroyed || socket.writableEnded) return;
  const payload = Buffer.from(body);
  socket.write(
    `HTTP/1.1 502 Bad Gateway\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: ${payload.length}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n`,
  );
  socket.end(payload);
}

function isDocument(request) {
  if (request.method !== "GET" && request.method !== "HEAD") return false;
  const accept = headerValue(request.headers, "accept");
  if (accept === "") return true;
  return accept.includes("text/html") || accept.includes("*/*");
}

function headerValue(headers, name) {
  if (!headers) return "";
  const value = headers[name] ?? headers[name.toLowerCase()];
  if (Array.isArray(value)) return String(value[0] ?? "");
  return value == null ? "" : String(value);
}

function matchesAny(path, prefixes) {
  return prefixes.some((prefix) => {
    if (prefix.endsWith("/")) {
      const bare = prefix.slice(0, -1);
      return path === bare || path.startsWith(prefix);
    }
    return path === prefix || path.startsWith(`${prefix}/`);
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
