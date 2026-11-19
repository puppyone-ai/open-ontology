import http from "node:http";
import { createContractMock } from "./contract-mock.mjs";

export const CONTRACT_TOKEN = `oodt_${"B".repeat(43)}`;

/**
 * The contract mock behind a real loopback HTTP server with Bearer auth.
 * `script(request, index)` may return `{ status, headers, body, raw, delayMs }`
 * to override a response; otherwise the schema-derived mock answers.
 * `requests` records what the client sent (headers included).
 */
export async function startContractServer({ token = CONTRACT_TOKEN, script, mock = createContractMock() } = {}) {
  const requests = [];
  let currentToken = token;
  const server = http.createServer((request, response) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", async () => {
      const text = Buffer.concat(chunks).toString("utf8");
      const record = { method: request.method, path: request.url, headers: request.headers, body: text ? JSON.parse(text) : undefined };
      requests.push(record);
      const send = async ({ status = 200, headers = {}, body, raw, delayMs = 0 }) => {
        if (delayMs) await new Promise((resolve) => { setTimeout(resolve, delayMs); });
        response.writeHead(status, { "content-type": "application/json", "OO-Docs-Api": "1.0", ...headers });
        response.end(raw ?? (body === undefined ? undefined : JSON.stringify(body)));
      };
      if (request.url === "/livez") { response.writeHead(204).end(); return; }
      if (request.headers.authorization !== `Bearer ${currentToken}`) {
        await send({ status: 401, body: { code: "DOCS_ENGINE_UNAUTHENTICATED", message: "no", retryable: false, details: {} } });
        return;
      }
      const scripted = script?.(record, requests.length);
      if (scripted) { await send(scripted); return; }
      const answer = mock.handle({ method: record.method, path: record.path, headers: record.headers, body: record.body });
      await send({ status: answer.status, headers: answer.headers, body: answer.body });
    });
  });
  await new Promise((resolve) => { server.listen(0, "127.0.0.1", resolve); });
  const { port } = server.address();
  return {
    port,
    baseUrl: `http://127.0.0.1:${port}`,
    token,
    requests,
    mock,
    /** Rotates the token the server accepts (an engine restart). */
    rotateToken(next) { currentToken = next; },
    close: () => new Promise((resolve) => { server.closeAllConnections?.(); server.close(resolve); }),
  };
}
