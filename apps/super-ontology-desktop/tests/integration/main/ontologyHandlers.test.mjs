import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { createOntologyHandlers, DECIDE_BATCH_SIZE } from "@so-main/ontology-handlers.mjs";
import { createGrantStore } from "@so-main/grants.mjs";
import { ROUTES } from "@so-main/control-client.mjs";
import { ONTOLOGY_CHANNELS } from "@so-shared/ontology-requests.mjs";
import { SUPER_ONTOLOGY_CHANNELS, parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const OP = "op_0123456789abcdef";
const DRIVE = "00000000-0000-4000-8000-000000000001";
const id = (n) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const SHARE = { id: id(50), state: "active", pack_id: id(51), drive_id: DRIVE, mount: { target_dir: "/tmp/mount-a", present: true } };

function make({ responses = {}, secretsDirectory = null, fsPromises = fs, hostMount = null } = {}) {
  const calls = [];
  const client = {
    request: vi.fn(async (name, options) => {
      calls.push([name, options]);
      const response = responses[name];
      if (typeof response === "function") return response(options);
      if (response !== undefined) return response;
      throw new Error(`unexpected ${name}`);
    }),
  };
  const grants = createGrantStore();
  const revealPath = vi.fn();
  return { handlers: createOntologyHandlers({ client, grants, revealPath, secretsDirectory, fsPromises, hostMount }), calls, grants, revealPath };
}

describe("L4 and L6 channel contract", () => {
  it("registers every handler channel and its route", () => {
    const { handlers } = make();
    const first = SUPER_ONTOLOGY_CHANNELS.indexOf("entities-search");
    const ontology = SUPER_ONTOLOGY_CHANNELS.slice(first, first + ONTOLOGY_CHANNELS.length);
    expect(Object.keys(handlers).sort()).toEqual([...ontology].sort());
    for (const name of ["entitiesList", "reviewDecide", "sharesCreate", "factCorrect", "entitiesMerge", "entitySplit", "modelConfigPut", "shareUpdates", "shareGroupRefresh", "constraintsList"]) {
      expect(ROUTES[name], name).toBeTruthy();
    }
    expect(ROUTES.factCorrect.ifMatch).toBe(true);
    expect(ROUTES.modelConfigPut.method).toBe("PUT");
  });

  it("requires an operation id for every request with a side effect", () => {
    for (const channel of ["review-decide", "pack-compile", "share-mount", "share-refresh", "share-revoke", "share-group-refresh", "fact-correct", "entity-merge", "entity-split"]) {
      expect(() => parseSuperOntologyRequest(channel, {}), channel).toThrow();
    }
  });

  it("rejects raw host paths, unknown fields and oversize decision lists", () => {
    expect(() => parseSuperOntologyRequest("share-mount", { operationId: OP, compile: { driveId: DRIVE }, targetGrantId: "grant-0001", expiresInSeconds: 3600, targetDir: "/etc" })).toThrow();
    expect(() => parseSuperOntologyRequest("review-decide", { operationId: OP, items: Array.from({ length: 5001 }, (_, n) => ({ id: id(n), revision: 1, decision: "accept" })) })).toThrow();
    expect(() => parseSuperOntologyRequest("pack-compile", { operationId: OP, compile: { driveId: DRIVE, budget: 100 } })).toThrow();
    expect(() => parseSuperOntologyRequest("share-mount", { operationId: OP, compile: { driveId: DRIVE }, targetGrantId: "grant-0001", expiresInSeconds: 60 })).toThrow();
    const parsed = parseSuperOntologyRequest("review-decide", { operationId: OP, bySource: { fileId: id(1), decision: "accept", asOfSeq: 9 } });
    expect(parsed.bySource).toEqual({ fileId: id(1), decision: "accept", asOfSeq: 9 });
  });
});

describe("review-decide", () => {
  it("sends a by-source decision as one request with the as_of_seq of the loaded group", async () => {
    const { handlers, calls } = make({ responses: { reviewDecide: { value: { results: [], accepted: 100, rejected: 0, publish_pending: true }, replayed: false } } });
    const result = await handlers["review-decide"]({ operationId: OP, bySource: { fileId: id(1), decision: "accept", kinds: ["low_confidence_fact"], asOfSeq: 41 } });
    expect(calls).toHaveLength(1);
    expect(calls[0]).toEqual(["reviewDecide", { body: { by_source: { file_id: id(1), decision: "accept", kinds: ["low_confidence_fact"], as_of_seq: 41 } }, operationId: OP }]);
    expect(result.value.accepted).toBe(100);
  });

  it("cuts items into batches of 500 with their own keys and merges the per-item results", async () => {
    const { handlers, calls } = make({
      responses: {
        reviewDecide: (options) => ({
          value: { results: options.body.items.map((item) => ({ id: item.id, status: item.decision === "reject" ? "already_decided" : "ok" })), accepted: options.body.items.length, rejected: 0, publish_pending: calls.length === 2 },
          replayed: false,
        }),
      },
    });
    const items = Array.from({ length: DECIDE_BATCH_SIZE + 5 }, (_, n) => ({ id: id(n), revision: 2, decision: n === 3 ? "reject" : "accept" }));
    const { value } = await handlers["review-decide"]({ operationId: OP, items });
    expect(calls.map(([, options]) => [options.body.items.length, options.operationId])).toEqual([[500, `${OP}-0`], [5, `${OP}-1`]]);
    expect(value.results).toHaveLength(505);
    expect(value.results[3].status).toBe("already_decided");
    expect(value.accepted).toBe(505);
    expect(value.publish_pending).toBe(true);
  });

  it("keeps the operation id as is for a single batch", async () => {
    const { handlers, calls } = make({ responses: { reviewDecide: { value: { results: [] }, replayed: true } } });
    const result = await handlers["review-decide"]({ operationId: OP, items: [{ id: id(1), revision: 1, decision: "edit", edit: { name: "A" } }] });
    expect(calls[0][1].operationId).toBe(OP);
    expect(calls[0][1].body.items[0]).toEqual({ id: id(1), revision: 1, decision: "edit", edit: { name: "A" } });
    expect(result.replayed).toBe(true);
  });
});

describe("share-mount and share-reveal", () => {
  it("turns the grant into the target directory and maps the compile request to the contract", async () => {
    const { handlers, calls, grants } = make({ responses: { sharesCreate: { value: SHARE, replayed: false } } });
    const sender = {};
    const { grantId } = grants.issue(sender, "mount-target", "/tmp/mount-a");
    const compile = { driveId: DRIVE, scope: ["Projects/a"], task: "t", budget: 800, role: "suggester", requireComplete: true };
    const result = await handlers["share-mount"]({ operationId: OP, compile, targetGrantId: grantId, expiresInSeconds: 604800, group: "T-1", refreshPolicy: "manual" }, { sender });
    expect(calls[0]).toEqual(["sharesCreate", {
      body: {
        compile: { drive_id: DRIVE, scope: ["Projects/a"], task: "t", budget: 800, role: "suggester", require_complete: true },
        mount: { target_dir: "/tmp/mount-a" }, expires_in_seconds: 604800, group: "T-1", refresh_policy: "manual",
      },
      operationId: OP,
    }]);
    expect(result.value.id).toBe(SHARE.id);
  });

  it("refuses a grant of another window", async () => {
    const { handlers, grants } = make({ responses: { sharesCreate: { value: SHARE } } });
    const { grantId } = grants.issue({}, "mount-target", "/tmp/mount-a");
    await expect(handlers["share-mount"]({ operationId: OP, compile: { driveId: DRIVE }, targetGrantId: grantId, expiresInSeconds: 3600 }, { sender: {} })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
  });

  it("reveals only the mount folder the engine recorded and only while it exists", async () => {
    const present = make({ responses: { shareGet: { value: SHARE } } });
    expect(await present.handlers["share-reveal"]({ shareId: SHARE.id })).toEqual({ revealed: true });
    expect(present.revealPath).toHaveBeenCalledWith("/tmp/mount-a");
    const missing = make({ responses: { shareGet: { value: { ...SHARE, mount: { target_dir: "/tmp/mount-a", present: false } } } } });
    expect(await missing.handlers["share-reveal"]({ shareId: SHARE.id })).toEqual({ revealed: false });
    expect(missing.revealPath).not.toHaveBeenCalled();
  });
});

describe("the host writes the mounted folder", () => {
  const fakeMount = (overrides = {}) => ({ preflight: vi.fn(async (to) => `${to}/real`), install: vi.fn(async () => "/x/.context"), replace: vi.fn(async () => "/x/.context"), remove: vi.fn(async () => true), annotate: vi.fn(async (shares) => shares.map((share) => ({ ...share, mount: { target_dir: "/tmp/mount-a/real", present: true } }))), ...overrides });
  const request = (grants, sender) => ({ operationId: OP, compile: { driveId: DRIVE }, targetGrantId: grants.issue(sender, "mount-target", "/tmp/mount-a").grantId, expiresInSeconds: 3600 });

  it("checks the target first, sends the checked path to the engine and installs the Share it returns", async () => {
    const hostMount = fakeMount();
    const { handlers, calls, grants } = make({ hostMount, responses: { sharesCreate: { value: SHARE, replayed: false } } });
    const sender = {};
    await handlers["share-mount"](request(grants, sender), { sender });
    expect(calls[0][1].body.mount.target_dir).toBe("/tmp/mount-a/real");
    expect(hostMount.install).toHaveBeenCalledWith(SHARE, "/tmp/mount-a/real");
  });

  it("accepts the share lists the engine really sends, which carry no as_of_seq", async () => {
    const { handlers } = make({ responses: { sharesList: { value: { items: [SHARE], next_cursor: null } }, shareUpdates: { value: { items: [], next_cursor: null } } } });
    expect((await handlers["shares-list"]({ driveId: DRIVE })).items).toHaveLength(1);
    expect((await handlers["share-updates"]({ shareId: SHARE.id })).items).toEqual([]);
  });

  it("fills `mount` of every Share the engine reports from the host's own record", async () => {
    const hostMount = fakeMount();
    const { handlers } = make({ hostMount, responses: { shareGet: { value: { ...SHARE, mount: { present: false } } }, sharesList: { value: { items: [{ ...SHARE, mount: { present: false } }], as_of_seq: 1, next_cursor: null } } } });
    expect((await handlers["share-get"]({ shareId: SHARE.id })).mount).toEqual({ target_dir: "/tmp/mount-a/real", present: true });
    expect((await handlers["shares-list"]({ driveId: DRIVE })).items[0].mount.present).toBe(true);
  });

  it("revokes the Share when the folder could not be written, and says why", async () => {
    const hostMount = fakeMount({ install: vi.fn(async () => { throw Object.assign(new Error("x"), { code: "DOCS_SHARE_TARGET_WRITE_FAILED" }); }) });
    const { handlers, calls, grants } = make({ hostMount, responses: { sharesCreate: { value: SHARE, replayed: false }, shareRevoke: { value: {}, replayed: false } } });
    const sender = {};
    await expect(handlers["share-mount"](request(grants, sender), { sender })).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_WRITE_FAILED" });
    expect(calls.map(([name]) => name)).toEqual(["sharesCreate", "shareRevoke"]);
    expect(calls[1][1].params.sid).toBe(SHARE.id);
  });

  it("creates nothing when the target is refused", async () => {
    const hostMount = fakeMount({ preflight: vi.fn(async () => { throw Object.assign(new Error("x"), { code: "DOCS_SHARE_TARGET_OCCUPIED" }); }) });
    const { handlers, calls, grants } = make({ hostMount });
    const sender = {};
    await expect(handlers["share-mount"](request(grants, sender), { sender })).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_OCCUPIED" });
    expect(calls).toEqual([]);
  });

  it("remounts a refreshed Share over the old folder (single and group) and removes the folder of a revoked one", async () => {
    const hostMount = fakeMount();
    const fresh = { ...SHARE, id: id(60) };
    const { handlers } = make({ hostMount, responses: {
      shareRefresh: { value: fresh, replayed: false },
      shareGroupRefresh: { value: { results: [{ id: SHARE.id }], refreshed: [fresh] }, replayed: false },
      shareRevoke: { value: { share: { state: "revoked" } }, replayed: false },
    } });
    await handlers["share-refresh"]({ operationId: OP, shareId: SHARE.id });
    expect(hostMount.replace).toHaveBeenCalledWith(SHARE.id, fresh);
    await handlers["share-group-refresh"]({ operationId: OP, groupId: id(70) });
    expect(hostMount.replace).toHaveBeenCalledTimes(2);
    await handlers["share-revoke"]({ operationId: OP, shareId: SHARE.id });
    expect(hostMount.remove).toHaveBeenCalledWith(SHARE.id);
  });
});

describe("model-config-save", () => {
  it("writes the key to a 0600 file in the state directory and sends only a file reference", async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "sd-model-"));
    const secretsDirectory = path.join(root, "secrets");
    try {
      const { handlers, calls } = make({ secretsDirectory, responses: { modelConfigPut: { value: { configured: true, revision: 3 } } } });
      await handlers["model-config-save"]({ provider: "openai-compatible", baseUrl: "https://models.example.test/v1", model: "m", apiKey: "k-secret", revision: 2 });
      const file = path.join(secretsDirectory, "model-key");
      expect((await fs.stat(file)).mode & 0o777).toBe(0o600);
      expect(await fs.readFile(file, "utf8")).toBe("k-secret");
      expect(calls[0]).toEqual(["modelConfigPut", { body: { provider: "openai-compatible", base_url: "https://models.example.test/v1", model: "m", key_ref: `file:${file}` }, revision: 2 }]);
      expect(JSON.stringify(calls)).not.toContain("k-secret");
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it("uses a file: path in the key field as the reference and copies nothing", async () => {
    const { handlers, calls } = make({ secretsDirectory: "/unused-secrets", responses: { modelConfigPut: { value: { configured: true, revision: 3 } } } });
    await handlers["model-config-save"]({ provider: "openai-compatible", baseUrl: "file:/home/someone/endpoint", model: "m", apiKey: "file:/home/someone/key", revision: 2 });
    expect(calls).toEqual([["modelConfigPut", { body: { provider: "openai-compatible", base_url: "file:/home/someone/endpoint", model: "m", key_ref: "file:/home/someone/key" }, revision: 2 }]]);
  });

  it("keeps the existing key reference when no new key is given", async () => {
    const { handlers, calls } = make({ secretsDirectory: "/unused", responses: { modelConfigGet: { value: { configured: true, key_ref: "env:MODEL_KEY", revision: 1 } }, modelConfigPut: { value: { configured: true, revision: 2 } } } });
    await handlers["model-config-save"]({ provider: "anthropic", baseUrl: "https://models.example.test", model: "m", revision: 1 });
    expect(calls[1][1].body.key_ref).toBe("env:MODEL_KEY");
  });
});

describe("corrections", () => {
  it("sends If-Match revisions and idempotency keys for correct, merge and split", async () => {
    const { handlers, calls } = make({
      responses: {
        factCorrect: { value: { id: id(7), predicate: "owns" } }, entitiesMerge: { value: { id: id(8), name: "A" } }, entitySplit: { value: { results: [] } },
      },
    });
    await handlers["fact-correct"]({ operationId: OP, factId: id(7), revision: 4, action: "mark_wrong", reason: "wrong" });
    await handlers["entity-merge"]({ operationId: OP, into: "person:a", from: ["person:b"], revision: 2 });
    await handlers["entity-split"]({ operationId: OP, entityId: "person:a", revision: 2, groups: [{}, { aliases: ["x"] }] });
    expect(calls).toEqual([
      ["factCorrect", { params: { id: id(7) }, body: { action: "mark_wrong", reason: "wrong" }, operationId: OP, revision: 4 }],
      ["entitiesMerge", { body: { into: "person:a", from: ["person:b"] }, operationId: OP, revision: 2 }],
      ["entitySplit", { params: { eid: "person:a" }, body: { groups: [{}, { aliases: ["x"] }] }, operationId: OP, revision: 2 }],
    ]);
  });
});

describe("lists", () => {
  it("passes the review filters through and defaults to pending", async () => {
    const { handlers, calls } = make({ responses: { reviewList: { value: { items: [], next_cursor: null, as_of_seq: 3 } } } });
    await handlers["review-list"]({ driveId: DRIVE, groupBy: "source" });
    expect(calls[0][1].query).toEqual({ drive_id: DRIVE, kind: undefined, source: undefined, status: "pending", group_by: "source", limit: undefined, cursor: undefined });
  });

  it("rejects a malformed engine answer", async () => {
    const { handlers } = make({ responses: { shareGet: { value: { nope: true } } } });
    await expect(handlers["share-get"]({ shareId: SHARE.id })).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });
});

describe("atlas channels (D88)", () => {
  it("reads the document to table links and the read-only database schema, nothing else", async () => {
    const schema = { protocol: "oo.ontology.schema/v1", tables: [], relationships: [], doc_links: [], next_cursor: null };
    const { handlers, calls } = make({ responses: { linksList: { value: { items: [], agent_db: "bound" } }, ontologySchema: { value: schema } } });
    expect(await handlers["atlas-links"]({})).toEqual({ items: [], agent_db: "bound" });
    expect(await handlers["ontology-schema"]({ limit: 5, cursor: "t5" })).toEqual(schema);
    expect(calls.find(([name]) => name === "ontologySchema")[1].query).toEqual({ limit: 5, cursor: "t5" });
    expect(ROUTES.ontologySchema.method).toBe("GET");
    expect(ROUTES.linksList.method).toBe("GET");
    expect(handlers["atlas-sql"]).toBeUndefined();
  });
  it("rejects an answer that is not a schema", async () => {
    const { handlers } = make({ responses: { ontologySchema: { value: { columns: [] } } } });
    await expect(handlers["ontology-schema"]({})).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });
  it("validates the request", () => {
    expect(() => parseSuperOntologyRequest("ontology-schema", { limit: 0 })).toThrow();
    expect(() => parseSuperOntologyRequest("ontology-schema", { limit: 500 })).toThrow();
    expect(() => parseSuperOntologyRequest("ontology-schema", { sql: "SELECT 1" })).toThrow();
    expect(() => parseSuperOntologyRequest("atlas-sql", {})).toThrow();
    expect(parseSuperOntologyRequest("ontology-schema", {})).toEqual({});
    expect(parseSuperOntologyRequest("ontology-schema", { limit: 100, cursor: "t100" })).toEqual({ limit: 100, cursor: "t100" });
  });
});

describe("file class overrides (D94)", () => {
  const drive = "00000000-0000-4000-8000-0000000000d1";
  it("lists a Drive's overrides and sets one with the engine's body shape", async () => {
    const { handlers, calls } = make({ responses: { profileOverridesList: { value: { items: [{ scope: "dir", path: "src", profile: "code", class: "code" }] } }, profileOverridesPut: { value: { affected: 4 } } } });
    expect((await handlers["profile-overrides-list"]({ driveId: drive })).items).toHaveLength(1);
    expect(await handlers["profile-override-set"]({ driveId: drive, scope: "dir", path: "src", profile: "code" })).toEqual({ affected: 4 });
    expect(calls.find(([name]) => name === "profileOverridesPut")[1]).toMatchObject({ params: { id: drive }, body: { scope: "dir", path: "src", profile: "code" } });
    expect(ROUTES.profileOverridesList.method).toBe("GET");
    expect(ROUTES.profileOverridesPut.method).toBe("PUT");
  });
  it("rejects an answer without a count", async () => {
    const { handlers } = make({ responses: { profileOverridesPut: { value: {} } } });
    await expect(handlers["profile-override-set"]({ driveId: drive, scope: "file", path: "a.md", profile: null })).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });
  it("validates the request: a known profile or null, a scope, a Drive-relative path", () => {
    expect(parseSuperOntologyRequest("profile-override-set", { driveId: drive, scope: "file", path: "a/b.md", profile: null })).toEqual({ driveId: drive, scope: "file", path: "a/b.md", profile: null });
    expect(parseSuperOntologyRequest("profile-override-set", { driveId: drive, scope: "dir", path: "src", profile: "reference" }).profile).toBe("reference");
    for (const bad of [{ profile: "paper" }, { profile: undefined }, { scope: "drive" }, { path: "../x" }, { path: "/abs" }, { extra: 1 }]) expect(() => parseSuperOntologyRequest("profile-override-set", { driveId: drive, scope: "file", path: "a.md", profile: "code", ...bad })).toThrow();
    expect(() => parseSuperOntologyRequest("profile-overrides-list", {})).toThrow();
    expect(parseSuperOntologyRequest("profile-overrides-list", { driveId: drive })).toEqual({ driveId: drive });
  });
});

describe("the structure layer (D93)", () => {
  const drive = "00000000-0000-4000-8000-0000000000d2";
  const schema = { protocol: "oo.docs.structure/v1", version: 2, classes: [{ class_key: "meeting_minutes" }], coverage: { record_files: 10, in_class: 9, confirmed: 0, share_in_class: 0.9, share_confirmed: 0 } };
  it("reads the schema, induces it and decides with the engine's body shape", async () => {
    const { handlers, calls } = make({ responses: { structureGet: { value: schema }, structureInduce: { value: schema }, structureDecide: { value: schema }, structureYaml: { value: { text: "protocol: x\n" } } } });
    expect((await handlers["structure-read"]({ driveId: drive })).version).toBe(2);
    expect((await handlers["structure-induce"]({ driveId: drive })).classes).toHaveLength(1);
    await handlers["structure-decide"]({ driveId: drive, op: "rename", classKey: "meeting_minutes", property: "attendees", name: "Who came" });
    expect(calls.find(([name]) => name === "structureDecide")[1]).toMatchObject({ params: { id: drive }, body: { op: "rename", class_key: "meeting_minutes", property: "attendees", name: "Who came" } });
    await handlers["structure-decide"]({ driveId: drive, op: "accept", classKey: "meeting_minutes" });
    expect(calls.filter(([name]) => name === "structureDecide")[1][1].body).toEqual({ op: "accept", class_key: "meeting_minutes", property: null });
    expect((await handlers["structure-yaml"]({ driveId: drive, database: false })).text).toBe("protocol: x\n");
    expect(calls.find(([name]) => name === "structureYaml")[1]).toMatchObject({ query: { database: "0" } });
    expect(ROUTES.structureGet.method).toBe("GET");
    expect(ROUTES.structureDecide.method).toBe("POST");
    expect(ROUTES.structureInduce.method).toBe("POST");
  });
  it("rejects an answer that is not a schema", async () => {
    const { handlers } = make({ responses: { structureGet: { value: { classes: [] } } } });
    await expect(handlers["structure-read"]({ driveId: drive })).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });
  it("validates the request: a Drive, a known operation, bounded names", () => {
    expect(parseSuperOntologyRequest("structure-read", { driveId: drive })).toEqual({ driveId: drive });
    expect(() => parseSuperOntologyRequest("structure-read", {})).toThrow();
    expect(parseSuperOntologyRequest("structure-decide", { driveId: drive, op: "merge", classKey: "c", property: "a", into: "b" }).into).toBe("b");
    expect(parseSuperOntologyRequest("structure-decide", { driveId: drive, op: "accept", classKey: "c", property: null }).property).toBeNull();
    for (const bad of [{ op: "delete" }, { classKey: "x".repeat(65) }, { name: "y".repeat(81) }, { extra: 1 }]) expect(() => parseSuperOntologyRequest("structure-decide", { driveId: drive, op: "rename", classKey: "c", name: "n", ...bad })).toThrow();
  });
});
