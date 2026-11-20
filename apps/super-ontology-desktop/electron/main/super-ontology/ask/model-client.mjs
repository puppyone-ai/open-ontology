import { readFile } from "node:fs/promises";

/**
 * One chat turn against the model configured in Settings, for the Ask loop. Messages are kept in a neutral shape
 * (`user`, `assistant` with `toolCalls`, `tool` with `results`) and converted here to the provider's own. The key is read from the
 * `file:` reference the engine holds, in this process only; it never reaches a result, an error or the renderer.
 */
const RETRY_STATUS = new Set([429, 500, 502, 503, 504]);

export class ModelError extends Error {
  constructor(kind, { httpStatus } = {}) {
    super(`model ${kind}`);
    this.kind = kind;
    this.httpStatus = httpStatus;
  }
}

/** `file:/path` is the reference the app and `oo docs model set` write (for the key, and for the address when it is kept in a file); a bare absolute path is read the same way. */
export async function readKeyRef(keyRef, { read = readFile } = {}) {
  const file = typeof keyRef === "string" ? (keyRef.startsWith("file:") ? keyRef.slice(5) : keyRef) : "";
  if (!file.startsWith("/")) throw new ModelError("key_unreadable");
  try {
    const text = (await read(file, "utf8")).trim();
    if (!text) throw new ModelError("key_unreadable");
    return text;
  } catch (error) {
    throw error instanceof ModelError ? error : new ModelError("key_unreadable");
  }
}

function openaiMessages(system, messages) {
  const out = [{ role: "system", content: system }];
  for (const m of messages) {
    if (m.role === "user") out.push({ role: "user", content: m.text });
    else if (m.role === "assistant") {
      out.push({
        role: "assistant", content: m.text || null,
        ...(m.toolCalls?.length ? { tool_calls: m.toolCalls.map((c) => ({ id: c.id, type: "function", function: { name: c.name, arguments: JSON.stringify(c.args ?? {}) } })) } : {}),
      });
    } else for (const r of m.results) out.push({ role: "tool", tool_call_id: r.id, content: r.content });
  }
  return out;
}

function anthropicMessages(messages) {
  const out = [];
  for (const m of messages) {
    if (m.role === "user") out.push({ role: "user", content: m.text });
    else if (m.role === "assistant") {
      const content = [];
      if (m.text) content.push({ type: "text", text: m.text });
      for (const c of m.toolCalls ?? []) content.push({ type: "tool_use", id: c.id, name: c.name, input: c.args ?? {} });
      out.push({ role: "assistant", content });
    } else out.push({ role: "user", content: m.results.map((r) => ({ type: "tool_result", tool_use_id: r.id, content: r.content })) });
  }
  return out;
}

const parseArgs = (raw) => {
  if (raw && typeof raw === "object") return raw;
  try { const parsed = JSON.parse(raw || "{}"); return parsed && typeof parsed === "object" ? parsed : {}; } catch { return {}; }
};

export function createModelClient({ fetchImpl = globalThis.fetch, timeoutMs = 60_000, sleep = (ms) => new Promise((r) => setTimeout(r, ms)) } = {}) {
  /** @returns {Promise<{ text: string, toolCalls: { id: string, name: string, args: object }[] }>} */
  async function chat({ config, key, system, messages, tools, signal }) {
    const anthropic = config.provider === "anthropic";
    const base = String(config.base_url ?? "").replace(/\/+$/u, "");
    const url = anthropic ? (base.endsWith("/messages") ? base : `${base}/v1/messages`) : (base.endsWith("/chat/completions") ? base : `${base}/chat/completions`);
    const body = anthropic
      ? { model: config.model, max_tokens: 2048, temperature: 0, system, messages: anthropicMessages(messages), ...(tools?.length ? { tools: tools.map((t) => ({ name: t.name, description: t.description, input_schema: t.parameters })) } : {}) }
      : { model: config.model, temperature: 0, messages: openaiMessages(system, messages), ...(tools?.length ? { tools: tools.map((t) => ({ type: "function", function: { name: t.name, description: t.description, parameters: t.parameters } })) } : {}) };
    const headers = anthropic
      ? { "x-api-key": key, "anthropic-version": "2023-06-01", "Content-Type": "application/json" }
      : { Authorization: `Bearer ${key}`, "Content-Type": "application/json" };
    let status = 0;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (signal?.aborted) throw new ModelError("cancelled");
      const limit = AbortSignal.timeout(timeoutMs);
      let response;
      try {
        response = await fetchImpl(url, { method: "POST", headers, body: JSON.stringify(body), signal: signal ? AbortSignal.any([signal, limit]) : limit, redirect: "error" });
      } catch (error) {
        if (signal?.aborted) throw new ModelError("cancelled");
        if (error?.name === "TimeoutError" || error?.name === "AbortError") throw new ModelError("timeout");
        throw new ModelError("unreachable");
      }
      status = response.status;
      if (RETRY_STATUS.has(status) && attempt < 2) { await response.arrayBuffer().catch(() => {}); await sleep(500 * 2 ** attempt); continue; }
      if (!response.ok) { await response.arrayBuffer().catch(() => {}); throw new ModelError(status === 401 || status === 403 ? "rejected" : "failed", { httpStatus: status }); }
      let json;
      try { json = await response.json(); } catch { throw new ModelError("failed", { httpStatus: status }); }
      if (anthropic) {
        const blocks = Array.isArray(json?.content) ? json.content : [];
        return {
          text: blocks.filter((b) => b.type === "text").map((b) => b.text).join("\n"),
          toolCalls: blocks.filter((b) => b.type === "tool_use").map((b) => ({ id: String(b.id), name: String(b.name), args: parseArgs(b.input) })),
        };
      }
      const message = json?.choices?.[0]?.message;
      if (!message) throw new ModelError("failed", { httpStatus: status });
      return {
        text: typeof message.content === "string" ? message.content : "",
        toolCalls: (message.tool_calls ?? []).map((c, i) => ({ id: String(c.id ?? `call_${i}`), name: String(c.function?.name ?? ""), args: parseArgs(c.function?.arguments) })),
      };
    }
    throw new ModelError("failed", { httpStatus: status });
  }
  return { chat };
}
