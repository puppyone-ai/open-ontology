import { describe, expect, it } from "vitest";
import { connectSnippet, createAgentHandlers } from "../../electron/main/super-ontology/agent-handlers.mjs";
import { parseSuperOntologyRequest } from "../../src/shared/super-ontology/requests.mjs";

describe("agent connect snippets", () => {
  it("match the shapes `oo mcp-config` prints and carry no credential", () => {
    expect(JSON.parse(connectSnippet({ agent: "claude", profile: "demo" }).text)).toEqual({ mcpServers: { "open-ontology": { command: "oo", args: ["mcp", "--stdio", "--profile", "demo"] } } });
    expect(connectSnippet({ agent: "cursor", profile: "demo" }).format).toBe("json");
    const codex = connectSnippet({ agent: "codex", profile: "demo" });
    expect(codex.text).toBe('[mcp_servers.open-ontology]\ncommand = "oo"\nargs = ["mcp", "--stdio", "--profile", "demo"]\n');
    expect(codex.cli).toBe("oo mcp-config --agent codex --profile demo");
    expect(connectSnippet({ agent: "generic", profile: "p" }).text).not.toMatch(/ooa_/u);
  });
});

describe("agent token requests", () => {
  const uuid = "00000000-0000-4000-8000-0000000000d1";
  it("validates names, scopes and drives", () => {
    expect(parseSuperOntologyRequest("agent-token-create", { name: "sales-bot", scopes: ["docs:read"], driveIds: [uuid] })).toMatchObject({ name: "sales-bot" });
    expect(() => parseSuperOntologyRequest("agent-token-create", { name: "bad name", scopes: ["docs:read"] })).toThrow();
    expect(() => parseSuperOntologyRequest("agent-token-create", { name: "a", scopes: ["docs:admin"] })).toThrow();
    expect(() => parseSuperOntologyRequest("agent-token-create", { name: "a", scopes: ["docs:read", "docs:read"] })).toThrow();
    expect(() => parseSuperOntologyRequest("agent-connect-snippet", { agent: "vim", profile: "p" })).toThrow();
  });
  it("sends the engine body and refuses a docs scope without a drive choice", async () => {
    const calls = [];
    const client = { request: async (name, options) => { calls.push([name, options]); return { value: { id: "x", token: `ooa_${"a".repeat(8)}` } }; } };
    const handlers = createAgentHandlers({ client });
    await handlers["agent-token-create"]({ name: "n", scopes: ["docs:read"], allDrives: true, expiresInSeconds: 3600 });
    expect(calls[0]).toEqual(["agentTokenCreate", { body: { name: "n", scopes: ["docs:read"], all_drives: true, expires_in_seconds: 3600 } }]);
    await expect(handlers["agent-token-create"]({ name: "n", scopes: ["docs:read"] })).rejects.toMatchObject({ code: "SO_INVALID_REQUEST" });
    await expect(handlers["agent-token-create"]({ name: "n", scopes: ["docs:read"], allDrives: true, driveIds: [uuid] })).rejects.toBeDefined();
  });
  it("lists revoked and expired tokens too, so a revoked row stays and shows its state", async () => {
    const calls = [];
    const client = { request: async (name, options) => { calls.push([name, options]); return { value: { items: [{ id: "x", state: "revoked" }] } }; } };
    const handlers = createAgentHandlers({ client });
    await expect(handlers["agent-token-list"]({})).resolves.toEqual({ items: [{ id: "x", state: "revoked" }] });
    expect(calls[0]).toEqual(["agentTokenList", { query: { state: "all" } }]);
  });
});
