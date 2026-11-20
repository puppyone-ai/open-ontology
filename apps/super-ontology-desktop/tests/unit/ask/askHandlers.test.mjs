import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createAskHandlers } from "../../../electron/main/super-ontology/ask/ask-handlers.mjs";
import { createModelClient, readKeyRef } from "../../../electron/main/super-ontology/ask/model-client.mjs";
import { parseSuperOntologyRequest } from "../../../src/shared/super-ontology/requests.mjs";

const dirs = [];
const tmp = async () => { const d = await fs.mkdtemp(path.join(os.tmpdir(), "ask-")); dirs.push(d); return d; };
afterEach(async () => { await Promise.all(dirs.splice(0).map((d) => fs.rm(d, { recursive: true, force: true }))); });

const json = (status, body) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const TOKEN = { id: "t1", token: `ooa_${"a".repeat(30)}`, gw_profile: null, expires_at: new Date(Date.now() + 30 * 86400_000).toISOString() };

function setup({ config = { configured: true, provider: "openai-compatible", base_url: "https://model.invalid/v1", model: "m", key_ref: "file:/k" }, model, engine = true, fetchImpl } = {}) {
  const calls = [];
  const client = { request: async (name, options) => {
    calls.push([name, options]);
    if (name === "modelConfigGet") return { value: config };
    if (name === "agentTokenList") return { value: { items: [{ id: "old", name: "super-ontology-app" }] } };
    if (name === "agentTokenCreate") return { value: TOKEN };
    if (name === "filesList") return { value: { items: [{ id: "f-1", path: "contracts/acme.md" }] } };
    return { value: {} };
  } };
  return { calls, client, build: async () => {
    const secrets = await tmp();
    const pushes = [];
    const handlers = createAskHandlers({
      client, credentials: () => (engine ? { baseUrl: "http://127.0.0.1:9", token: "oodt_x" } : null), secretsDirectory: secrets, fsPromises: fs,
      broadcast: (channel, payload) => pushes.push([channel, payload]), model, readKey: async () => "sk-secret", fetchImpl,
    });
    return { handlers, secrets, pushes };
  } };
}

describe("ask-run", () => {
  it("answers with a cited file chip that has a file id, using a private agent token and never leaking the key", async () => {
    let step = 0;
    const model = { chat: async () => (step++ === 0 ? { text: "", toolCalls: [{ id: "1", name: "search", args: { query: "Acme" } }] } : { text: "Acme renews in March [S1].", toolCalls: [] }) };
    const seenAuth = [];
    const fetchImpl = async (url, init) => { seenAuth.push(init.headers.Authorization); return json(200, { items: [{ urn: "oo:doc/d1/org:acme", side: "doc", type: "Organization", name: "Acme", provenance: { kind: "file", drive_id: "d1", path: "contracts/acme.md", anchor: { heading: "Renewal" } } }], sides: {} }); };
    const { calls, build } = setup({ model, fetchImpl });
    const { handlers, secrets, pushes } = await build();
    const out = await handlers["ask-run"]({ askId: "ask-0001", question: "When does Acme renew?" });
    expect(out.status).toBe("answered");
    expect(out.sources).toEqual([expect.objectContaining({ id: "S1", kind: "file", path: "contracts/acme.md", fileId: "f-1", anchor: "Renewal" })]);
    expect(seenAuth).toEqual([`Bearer ${TOKEN.token}`]);
    expect(pushes.map(([c, p]) => [c, p.state])).toEqual([["ask-progress", "running"], ["ask-progress", "done"]]);
    expect(JSON.stringify(out)).not.toMatch(/sk-secret|ooa_|oodt_/u);
    // The old token of this app was retired, the new one is kept in a 0600 file.
    expect(calls.some(([n, o]) => n === "agentTokenRevoke" && o.params.id === "old")).toBe(true);
    const file = path.join(secrets, "ask-agent.json");
    expect((await fs.stat(file)).mode & 0o777).toBe(0o600);
  });

  it("reuses the stored token on the next question", async () => {
    const model = { chat: async ({ messages }) => (messages.some((m) => m.role === "tool") ? { text: "NOT_IN_DATA: none.", toolCalls: [] } : { text: "", toolCalls: [{ id: "1", name: "search", args: { query: "x" } }] }) };
    const { calls, build } = setup({ model, fetchImpl: async () => json(200, { items: [] }) });
    const { handlers } = await build();
    await handlers["ask-run"]({ askId: "ask-0002", question: "x" });
    await handlers["ask-run"]({ askId: "ask-0002", question: "y" });
    expect(calls.filter(([n]) => n === "agentTokenCreate")).toHaveLength(1);
  });

  it("says plainly when no model is configured, the key cannot be read, or the engine is down", async () => {
    const model = { chat: async () => ({ text: "x", toolCalls: [] }) };
    const unreadable = createAskHandlers({ client: { request: async () => ({ value: { configured: true, provider: "openai-compatible", base_url: "https://m.invalid", model: "m", key_ref: "file:/nope" } }) }, credentials: () => ({ baseUrl: "http://127.0.0.1:9" }), secretsDirectory: await tmp(), fsPromises: fs, model });
    expect((await unreadable["ask-run"]({ askId: "ask-0006", question: "q" })).status).toBe("key_unreadable");
    expect((await (await setup({ model, config: { configured: false } }).build()).handlers["ask-run"]({ askId: "ask-0003", question: "q" })).status).toBe("no_model");
    expect((await (await setup({ model, engine: false }).build()).handlers["ask-run"]({ askId: "ask-0004", question: "q" })).status).toBe("engine_down");
  });

  it("maps model timeouts and rejections to calm states", async () => {
    const { ModelError } = await import("../../../electron/main/super-ontology/ask/model-client.mjs");
    for (const [kind, status] of [["timeout", "model_timeout"], ["rejected", "model_rejected"], ["failed", "model_failed"], ["unreachable", "model_failed"]]) {
      const model = { chat: async () => { throw new ModelError(kind, { httpStatus: 401 }); } };
      const { handlers } = await setup({ model, fetchImpl: async () => json(200, {}) }).build();
      expect((await handlers["ask-run"]({ askId: "ask-0005", question: "q" })).status).toBe(status);
    }
  });

  it("reads a base address kept in a file: reference, without sending the reference to the model", async () => {
    const seen = [];
    const modelClient = { chat: async (input) => { seen.push(input.config.base_url); return { text: "NOT_IN_DATA: none.", toolCalls: [] }; } };
    const dir = await tmp();
    const handlers = createAskHandlers({
      client: { request: async () => ({ value: { configured: true, provider: "openai-compatible", base_url: "file:/addr", model: "m", key_ref: "file:/key" } }) },
      credentials: () => ({ baseUrl: "http://127.0.0.1:9" }), secretsDirectory: dir, fsPromises: fs, model: modelClient, readKey: async (ref) => (ref === "file:/addr" ? "https://model.invalid/v1" : "sk"),
    });
    const out = await handlers["ask-run"]({ askId: "ask-0007", question: "q" });
    expect(out.status).toBe("unanswerable");
    expect(seen).toEqual(["https://model.invalid/v1"]);
  });

  it("validates the request", () => {
    expect(parseSuperOntologyRequest("ask-run", { askId: "ask-00001", question: "hi", history: [{ question: "a", answer: "b" }] })).toMatchObject({ question: "hi" });
    expect(() => parseSuperOntologyRequest("ask-run", { askId: "x", question: "hi" })).toThrow();
    expect(() => parseSuperOntologyRequest("ask-run", { askId: "ask-00001", question: "" })).toThrow();
    expect(() => parseSuperOntologyRequest("ask-run", { askId: "ask-00001", question: "q", apiKey: "k" })).toThrow();
  });
});

describe("the model client", () => {
  it("speaks the OpenAI shape with tools and parses tool calls", async () => {
    let seen;
    const fetchImpl = async (url, init) => { seen = { url, body: JSON.parse(init.body), auth: init.headers.Authorization }; return json(200, { choices: [{ message: { content: "", tool_calls: [{ id: "c1", function: { name: "search", arguments: "{\"query\":\"a\"}" } }] } }] }); };
    const reply = await createModelClient({ fetchImpl }).chat({ config: { provider: "openai-compatible", base_url: "https://m.invalid/v1/", model: "g" }, key: "k", system: "s", messages: [{ role: "user", text: "hi" }], tools: [{ name: "search", description: "d", parameters: { type: "object" } }] });
    expect(seen.url).toBe("https://m.invalid/v1/chat/completions");
    expect(seen.body.tools[0].function.name).toBe("search");
    expect(reply.toolCalls).toEqual([{ id: "c1", name: "search", args: { query: "a" } }]);
  });
  it("speaks the Anthropic shape", async () => {
    let seen;
    const fetchImpl = async (url, init) => { seen = { url, body: JSON.parse(init.body), key: init.headers["x-api-key"] }; return json(200, { content: [{ type: "text", text: "ok" }] }); };
    const reply = await createModelClient({ fetchImpl }).chat({ config: { provider: "anthropic", base_url: "https://a.invalid", model: "c" }, key: "k", system: "s", messages: [{ role: "user", text: "hi" }, { role: "assistant", text: "", toolCalls: [{ id: "t", name: "search", args: {} }] }, { role: "tool", results: [{ id: "t", name: "search", content: "r" }] }], tools: [] });
    expect(seen.url).toBe("https://a.invalid/v1/messages");
    expect(seen.body.messages.at(-1).content[0]).toMatchObject({ type: "tool_result", tool_use_id: "t" });
    expect(reply.text).toBe("ok");
  });
  it("retries a 503 and gives up with a named failure", async () => {
    let n = 0;
    const fetchImpl = async () => { n += 1; return json(503, {}); };
    await expect(createModelClient({ fetchImpl, sleep: async () => {} }).chat({ config: { provider: "openai-compatible", base_url: "https://m.invalid", model: "g" }, key: "k", system: "s", messages: [], tools: [] })).rejects.toMatchObject({ kind: "failed", httpStatus: 503 });
    expect(n).toBe(3);
  });
  it("reads only a file: reference for the key", async () => {
    const dir = await tmp();
    await fs.writeFile(path.join(dir, "k"), "sk-1\n");
    expect(await readKeyRef(`file:${path.join(dir, "k")}`)).toBe("sk-1");
    await expect(readKeyRef("env:OPENAI")).rejects.toMatchObject({ kind: "key_unreadable" });
    await expect(readKeyRef("file:/nonexistent/key")).rejects.toMatchObject({ kind: "key_unreadable" });
  });
});
