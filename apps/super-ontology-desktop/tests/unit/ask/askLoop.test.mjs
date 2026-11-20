import { describe, expect, it } from "vitest";
import { equivalentCommands, MAX_CALLS, runAskLoop } from "../../../electron/main/super-ontology/ask/ask-loop.mjs";
import { anchorText, createAskTools, createSourceRegistry } from "../../../electron/main/super-ontology/ask/ask-tools.mjs";
import { ModelError } from "../../../electron/main/super-ontology/ask/model-client.mjs";

/** A model that plays a script: each entry is one reply. */
const scripted = (replies) => { let i = 0; const seen = []; return { seen, chat: async (input) => { seen.push(input); return replies[Math.min(i++, replies.length - 1)]; } }; };
const call = (name, args, id = `c${Math.random()}`) => ({ id, name, args });
const fakeTools = (outputs, registry) => ({
  definitions: [{ name: "search" }],
  run: async (name, args) => { const out = outputs[name] ?? "error: none"; return typeof out === "function" ? out(args, registry) : out; },
});

describe("the Ask loop", () => {
  it("calls tools, then answers with citations that map to sources the tools returned", async () => {
    const registry = createSourceRegistry();
    const model = scripted([
      { text: "", toolCalls: [call("search", { query: "Acme" })] },
      { text: "Acme renews every March [S1] and has 12 seats [S2, S9].", toolCalls: [] },
    ]);
    const tools = fakeTools({ search: (_a, r) => { r.add({ kind: "file", fileId: "f1", driveId: "d1", path: "contracts/acme.md", anchor: "L3" }); r.add({ kind: "file", fileId: "f2", driveId: "d1", path: "notes/acme.md", anchor: "" }); return "[S1] doc Organization Acme\n[S2] ..."; } }, registry);
    const events = [];
    const out = await runAskLoop({ question: "When does Acme renew?", model, tools, registry, emit: (e) => events.push(e) });
    expect(out.status).toBe("answered");
    expect(out.answer).toBe("Acme renews every March [S1] and has 12 seats [S2].");
    expect(out.sources.map((s) => s.path)).toEqual(["contracts/acme.md", "notes/acme.md"]);
    expect(out.invalidCitations).toBe(1);
    expect(out.uncited).toBe(false);
    expect(events.map((e) => e.state)).toEqual(["running", "done"]);
    expect(out.cli).toBe('oo ontology search "Acme"');
  });

  it("an invented citation is dropped and an answer with none is flagged uncited", async () => {
    const registry = createSourceRegistry();
    const model = scripted([{ text: "", toolCalls: [call("search", { query: "x" })] }, { text: "It is 42 [S7].", toolCalls: [] }]);
    const out = await runAskLoop({ question: "q", model, tools: fakeTools({ search: "no results" }, registry), registry });
    expect(out.status).toBe("answered");
    expect(out.sources).toEqual([]);
    expect(out.uncited).toBe(true);
    expect(out.answer).toBe("It is 42.");
  });

  it("an unanswerable question says so plainly and lists what was searched, and never offers a guess", async () => {
    const registry = createSourceRegistry();
    const model = scripted([
      { text: "", toolCalls: [call("search", { query: "salary" }), call("find_text", { query: "薪资" })] },
      { text: "NOT_IN_DATA: I searched for salary figures and found no document that states one.", toolCalls: [] },
    ]);
    const out = await runAskLoop({ question: "What is the CEO's salary?", model, tools: fakeTools({ search: "no results", find_text: "no passage matches" }, registry), registry });
    expect(out.status).toBe("unanswerable");
    expect(out.reason).toBe("I searched for salary figures and found no document that states one.");
    expect(out.searched).toEqual([{ tool: "search", arg: "salary" }, { tool: "find_text", arg: "薪资" }]);
    expect(out.answer).toBe("");
  });

  it("makes the model look before it answers, once", async () => {
    const registry = createSourceRegistry();
    const model = scripted([{ text: "Probably 42.", toolCalls: [] }, { text: "NOT_IN_DATA: nothing found.", toolCalls: [] }]);
    const out = await runAskLoop({ question: "q", model, tools: fakeTools({}, registry), registry });
    expect(out.status).toBe("unanswerable");
    expect(model.seen).toHaveLength(2);
  });

  it("stops at the call budget and asks for a final answer", async () => {
    const registry = createSourceRegistry();
    const always = { text: "", toolCalls: [call("search", { query: "again" })] };
    let turns = 0;
    const model = { chat: async ({ tools }) => { turns += 1; return tools.length === 0 ? { text: "NOT_IN_DATA: searched many times.", toolCalls: [] } : always; } };
    let ran = 0;
    const out = await runAskLoop({ question: "q", model, tools: { definitions: [{ name: "search" }], run: async () => { ran += 1; return "no results"; } }, registry });
    expect(ran).toBe(MAX_CALLS);
    expect(out.status).toBe("unanswerable");
    expect(turns).toBeLessThanOrEqual(MAX_CALLS + 2);
  });

  it("names a model failure instead of throwing, and cancels cleanly", async () => {
    const registry = createSourceRegistry();
    const timeout = { chat: async () => { throw new ModelError("timeout"); } };
    expect(await runAskLoop({ question: "q", model: timeout, tools: fakeTools({}, registry), registry })).toMatchObject({ status: "model_error", kind: "timeout" });
    const controller = new AbortController(); controller.abort();
    expect((await runAskLoop({ question: "q", model: scripted([]), tools: fakeTools({}, registry), registry, signal: controller.signal })).status).toBe("cancelled");
  });

  it("passes at most four earlier turns and the question to the model", async () => {
    const registry = createSourceRegistry();
    const model = scripted([{ text: "NOT_IN_DATA: x", toolCalls: [] }]);
    const history = Array.from({ length: 6 }, (_, i) => ({ question: `q${i}`, answer: `a${i}` }));
    await runAskLoop({ question: "now", history, model, tools: fakeTools({}, registry), registry });
    const sent = model.seen[0].messages;
    expect(sent).toHaveLength(9);
    expect(sent[0].text).toBe("q2");
    expect(sent.at(-1).text).toBe("now");
  });

  it("prints equivalent commands without repeats", () => {
    expect(equivalentCommands([{ tool: "search", arg: 'a "b"' }, { tool: "search", arg: 'a "b"' }, { tool: "schema", arg: "" }, { tool: "sql", arg: "SELECT 1" }])).toBe('oo ontology search "a \\"b\\""\noo ontology schema\noo ontology query "SELECT 1"');
  });
});

describe("the Ask tools", () => {
  const http = (routes) => async (method, route) => { const hit = routes.find(([m, prefix]) => m === method && route.startsWith(prefix)); return hit ? { status: 200, json: hit[2] } : { status: 404, json: { error: { code: "NOT_FOUND", message: "no" } } }; };

  it("search tags each source and says when nothing matched", async () => {
    const registry = createSourceRegistry();
    const tools = createAskTools({ registry, http: http([["GET", "/v1/ontology/search?q=Acme", { items: [{ urn: "oo:doc/d1/org:acme", side: "doc", type: "Organization", name: "Acme", provenance: { kind: "file", drive_id: "d1", path: "c/acme.md", anchor: { line_start: 3, line_end: 4 } } }], sides: {} }], ["GET", "/v1/ontology/search?q=zzz", { items: [], sides: {}, warnings: [] }]]) });
    const hit = await tools.run("search", { query: "Acme" });
    expect(hit).toContain("[S1] doc Organization Acme");
    expect(registry.get("S1")).toMatchObject({ kind: "file", path: "c/acme.md", anchor: "L3-L4", driveId: "d1" });
    expect(await tools.run("search", { query: "zzz" })).toContain('no results for "zzz"');
  });

  it("find_text carries the file id so a chip can jump; sql records the query", async () => {
    const registry = createSourceRegistry();
    const tools = createAskTools({ registry, http: async (m, route, body) => (route.startsWith("/v1/docs/search")
      ? { status: 200, json: { items: [{ kind: "block", id: "f#b", text: "Renews in March", score: 1, evidence: [{ file_id: "f1", drive_id: "d1", path: "a.md", block_id: "b", anchor: { line_start: 2 } }] }], truncated: false } }
      : { status: 200, json: { columns: ["n"], rows: [[3]], row_count: 1, provenance: { kind: "gateway_query", query_id: "q1" }, echo: body } }) });
    expect(await tools.run("find_text", { query: "March" })).toContain("file_id f1");
    expect(registry.get("S1")).toMatchObject({ kind: "file", fileId: "f1", anchor: "L2" });
    const rows = await tools.run("sql", { sql: "SELECT count(*) n FROM orders" });
    expect(rows).toContain("[S2]");
    expect(registry.get("S2")).toMatchObject({ kind: "query", sql: "SELECT count(*) n FROM orders", queryId: "q1" });
  });

  it("turns an engine refusal into text the model can read, and rejects unknown tools and empty arguments", async () => {
    const registry = createSourceRegistry();
    const tools = createAskTools({ registry, http: async () => ({ status: 403, json: { error: { code: "ONTOLOGY_SCOPE_DENIED", message: "db:read needed" } } }) });
    expect(await tools.run("sql", { sql: "SELECT 1" })).toBe("error 403 ONTOLOGY_SCOPE_DENIED: db:read needed");
    expect(await tools.run("rm", {})).toContain("unknown tool");
    expect(await tools.run("search", { query: " " })).toBe("error: query is required");
  });

  it("reads anchors the way the engine writes them", () => {
    expect(anchorText({ heading: "Terms" })).toBe("Terms");
    expect(anchorText({ page: 4 })).toBe("p.4");
    expect(anchorText({ sheet: "Q1", cell: "B2" })).toBe("Q1!B2");
    expect(anchorText(null)).toBe("");
  });
});
