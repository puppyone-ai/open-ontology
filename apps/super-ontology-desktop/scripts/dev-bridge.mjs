#!/usr/bin/env node
// Development stand-in for the Electron main process: runs the real Super Ontology service (engine lifecycle, control client,
// event bridge) in this Node process and exposes the preload bridge to a browser tab over loopback HTTP:
//   POST /bridge/invoke  {channel, request}  ->  the envelope the preload bridge would return
//   GET  /bridge/push                          ->  Server-Sent Events carrying the push channels
// It binds to 127.0.0.1 only. `OO_BIN` names the oo executable to use; state lives under `--state-dir`.
import http from "node:http";
import os from "node:os";
import path from "node:path";
import fs from "node:fs/promises";
import { createSuperOntologyService } from "../electron/main/super-ontology/service.mjs";
import { unwrapHandlerResult } from "../electron/main/super-ontology/handler-result.mjs";
import { toErrorResult } from "../src/shared/super-ontology/errors.mjs";
import { parseSuperOntologyRequest, SUPER_ONTOLOGY_CHANNELS } from "../src/shared/super-ontology/requests.mjs";

const args = process.argv.slice(2);
const option = (name, fallback) => { const i = args.indexOf(name); return i >= 0 && args[i + 1] ? args[i + 1] : fallback; };
const port = Number(option("--port", "5274"));
const stateDir = path.resolve(option("--state-dir", path.join(os.tmpdir(), "so-dev-bridge")));
const pickQueue = [];

const clients = new Set();
const broadcast = (channel, payload) => {
  const frame = `data: ${JSON.stringify({ channel, payload })}\n\n`;
  for (const response of clients) response.write(frame);
};

await fs.mkdir(path.join(stateDir, "state"), { recursive: true, mode: 0o700 });
await fs.chmod(path.join(stateDir, "state"), 0o700);
const service = createSuperOntologyService({
  paths: { stateDir: path.join(stateDir, "state"), logsDir: path.join(stateDir, "logs"), settingsFile: path.join(stateDir, "settings.json") },
  broadcast,
  platformHost: { platform: process.platform === "darwin" ? "macos" : process.platform, executableDiscovery: { captureEnvironment: async () => ({ environment: { ...process.env, ...(process.env.OO_BIN ? { PATH: `${path.dirname(process.env.OO_BIN)}:${process.env.PATH}` } : {}) } }) } },
  pickPath: async () => pickQueue.shift() ?? null,
});

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://127.0.0.1");
  if (url.pathname === "/bridge/push") {
    response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", Connection: "keep-alive" });
    response.write(": ok\n\n");
    clients.add(response);
    request.on("close", () => clients.delete(response));
    return;
  }
  if (url.pathname === "/bridge/pick" && request.method === "POST") {
    // Development only: queue the path the next native-picker call should return.
    const body = JSON.parse(await readBody(request));
    pickQueue.push(body.path);
    response.writeHead(204).end();
    return;
  }
  if (url.pathname === "/bridge/invoke" && request.method === "POST") {
    let result;
    try {
      const { channel, request: raw } = JSON.parse(await readBody(request));
      if (!SUPER_ONTOLOGY_CHANNELS.includes(channel)) throw Object.assign(new Error("bad channel"), { code: "SO_INVALID_REQUEST" });
      const parsed = parseSuperOntologyRequest(channel, raw);
      const { value, replayed } = unwrapHandlerResult(await service.handlers[channel](parsed, { sender: devSender }));
      result = replayed ? { ok: true, value, replayed: true } : { ok: true, value };
    } catch (error) {
      result = toErrorResult(error);
    }
    response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(result));
    return;
  }
  response.writeHead(404).end();
});
const devSender = {};
const readBody = (request) => new Promise((resolve, reject) => { const chunks = []; request.on("data", (c) => chunks.push(c)); request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8"))); request.on("error", reject); });
server.listen(port, "127.0.0.1", () => console.log(`dev bridge on http://127.0.0.1:${port} (state ${stateDir})`));
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, async () => { await service.shutdown(); server.close(); process.exit(0); });
