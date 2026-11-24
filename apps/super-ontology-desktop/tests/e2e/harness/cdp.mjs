// A small Chrome DevTools Protocol client over Node's built-in WebSocket, for the packaged-app scenarios (60 §3.2, 20-frontend §6.6).
// It only reads: `evaluate` runs an expression in the page and returns its JSON value. Operating the app is Peekaboo's job. No
// dependency is added: the alternative named in §6.6 (playwright-core connectOverCDP) would need a package.json change.
import fs from "node:fs/promises";
import path from "node:path";

const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

/** `--remote-debugging-port=0` makes Chromium pick a port and write it (first line) to `<user data dir>/DevToolsActivePort`. */
export async function readDevToolsPort(profileDirectory, { attempts = 200, interval = 100 } = {}) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const [port] = (await fs.readFile(path.join(profileDirectory, "DevToolsActivePort"), "utf8")).split("\n");
      if (/^\d+$/u.test(port)) return Number(port);
    } catch { /* Not written yet. */ }
    await sleep(interval);
  }
  throw new Error("DevToolsActivePort did not appear");
}

export async function connectToPage(port, { urlPattern = /./u } = {}) {
  let target = null;
  for (let attempt = 0; attempt < 100 && !target; attempt += 1) {
    const targets = await fetch(`http://127.0.0.1:${port}/json`).then((response) => response.json(), () => []);
    target = targets.find((entry) => entry.type === "page" && urlPattern.test(entry.url ?? "")) ?? null;
    if (!target) await sleep(200);
  }
  if (!target) throw new Error("no page target to attach to");
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once: true }); socket.addEventListener("error", reject, { once: true }); });
  let nextId = 0;
  const pending = new Map();
  const console_ = [];
  socket.addEventListener("message", (message) => {
    const data = JSON.parse(String(message.data));
    if (data.id !== undefined && pending.has(data.id)) {
      const { resolve, reject } = pending.get(data.id);
      pending.delete(data.id);
      if (data.error) reject(new Error(data.error.message)); else resolve(data.result);
    } else if (data.method === "Runtime.consoleAPICalled") {
      console_.push({ type: data.params.type, text: data.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" ") });
    }
  });
  // A closed socket must settle every request still waiting: otherwise a caller awaiting one (the harness's `Browser.close`) never
  // resumes and the process silently ends with exit code 0 before it wrote its report.
  socket.addEventListener("close", () => { for (const { reject } of pending.values()) reject(new Error("the page connection closed")); pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { nextId += 1; pending.set(nextId, { resolve, reject }); socket.send(JSON.stringify({ id: nextId, method, params })); });
  await send("Runtime.enable");
  return {
    send,
    console: () => [...console_],
    async evaluate(expression) {
      const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? "evaluation failed");
      return result.result.value;
    },
    async until(expression, { timeoutMs = 30_000, interval = 200, description = expression } = {}) {
      const started = Date.now();
      while (Date.now() - started < timeoutMs) {
        if (await this.evaluate(expression).catch(() => false)) return;
        await sleep(interval);
      }
      throw new Error(`Timed out waiting for ${description}`);
    },
    close: () => socket.close(),
  };
}
