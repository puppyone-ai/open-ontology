import http from "node:http";

/**
 * A scripted `GET /v1/docs/events` endpoint on loopback. Each accepted
 * connection becomes a `stream` the test can write to (as events, comments
 * or raw bytes) and close. The real engine sends `retry: 3000`; tests shorten it with `retryMs`.
 */
export async function startFakeEventStream({ token = `oodt_${"E".repeat(43)}`, retryMs = 10 } = {}) {
  const connections = [];
  let nextResponse = null;
  const waiters = [];
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, "http://127.0.0.1");
    if (url.pathname !== "/v1/docs/events") { response.writeHead(404).end(); return; }
    const record = { since: url.searchParams.get("since"), headers: request.headers, response };
    connections.push(record);
    for (const waiter of waiters.splice(0)) waiter();
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401, { "content-type": "application/json" }).end(JSON.stringify({ code: "DOCS_ENGINE_UNAUTHENTICATED", message: "x", retryable: false, details: {} }));
      return;
    }
    if (nextResponse) {
      const plan = nextResponse;
      nextResponse = null;
      response.writeHead(plan.status, { "content-type": "application/json", "OO-Docs-Api": "1.0" });
      response.end(JSON.stringify(plan.body ?? {}));
      return;
    }
    response.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store", "OO-Docs-Api": "1.0" });
    response.write(`retry: ${retryMs}\n\n`);
  });
  await new Promise((resolve) => { server.listen(0, "127.0.0.1", resolve); });
  const { port } = server.address();
  const live = () => connections.filter((entry) => !entry.response.writableEnded && !entry.response.destroyed);
  return {
    port, token, connections,
    baseUrl: `http://127.0.0.1:${port}`,
    /** The next connection answers with this status and JSON body instead of a stream. */
    respondOnce: (status, body) => { nextResponse = { status, body }; },
    current: () => live().at(-1),
    send(event, { id = event.seq } = {}) {
      const target = live().at(-1)?.response;
      target?.write(`${id === undefined ? "" : `id: ${id}\n`}event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
    },
    raw: (text) => live().at(-1)?.response.write(text),
    comment: (text) => live().at(-1)?.response.write(`: ${text}\n\n`),
    drop: () => { for (const entry of live()) entry.response.destroy(); },
    end: () => { for (const entry of live()) entry.response.end(); },
    /** Resolves when at least `count` connections have been attempted. */
    async attempts(count, timeout = 5_000) {
      const deadline = Date.now() + timeout;
      while (connections.length < count) {
        if (Date.now() > deadline) throw new Error(`Only ${connections.length} connection attempts, wanted ${count}`);
        await new Promise((resolve) => { waiters.push(resolve); setTimeout(resolve, 20); });
      }
    },
    close: () => new Promise((resolve) => { server.closeAllConnections?.(); server.close(resolve); }),
  };
}
