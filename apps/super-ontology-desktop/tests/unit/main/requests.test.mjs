import { ONTOLOGY_CHANNELS } from "@so-shared/ontology-requests.mjs";
import { describe, expect, it } from "vitest";
import { SO_ERROR_CODES, SuperOntologyError, toErrorResult } from "@so-shared/errors.mjs";
import {
  AGENT_CHANNELS,
  ENTERPRISE_CHANNELS,
  SUPER_ONTOLOGY_CHANNELS,
  parseSuperOntologyRequest,
} from "@so-shared/requests.mjs";

describe("parseSuperOntologyRequest", () => {
  it("accepts an empty engine-read request in any empty shape", () => {
    expect(parseSuperOntologyRequest("engine-read", undefined)).toEqual({});
    expect(parseSuperOntologyRequest("engine-read", null)).toEqual({});
    expect(parseSuperOntologyRequest("engine-read", {})).toEqual({});
  });

  it("accepts a well-formed operationId and returns a frozen copy", () => {
    const parsed = parseSuperOntologyRequest("engine-read", { operationId: "op_1234567890abcdef" });
    expect(parsed).toEqual({ operationId: "op_1234567890abcdef" });
    expect(Object.isFrozen(parsed)).toBe(true);
  });

  it.each([
    ["unknown channel", "shares-delete-all", {}],
    ["non-string channel", 7, {}],
    ["prototype channel", "toString", {}],
    ["array body", "engine-read", []],
    ["string body", "engine-read", "x"],
    ["unknown field", "engine-read", { path: "../x" }],
    ["short operationId", "engine-read", { operationId: "short" }],
    ["operationId with slash", "engine-read", { operationId: "../../etc/passwd/aaaa" }],
    ["overlong operationId", "engine-read", { operationId: "a".repeat(129) }],
    ["non-string operationId", "engine-read", { operationId: 12345678901234567 }],
  ])("rejects %s with SO_INVALID_REQUEST", (_name, channel, body) => {
    expect(() => parseSuperOntologyRequest(channel, body)).toThrow(
      expect.objectContaining({ code: "SO_INVALID_REQUEST" }),
    );
  });

  it("validates field values per channel", () => {
    expect(parseSuperOntologyRequest("pick-path", { purpose: "oo-executable" })).toEqual({ purpose: "oo-executable" });
    expect(parseSuperOntologyRequest("engine-set-executable", { grantId: "0b4f8d5e-1111-4222-8333-444455556666" })).toEqual({
      grantId: "0b4f8d5e-1111-4222-8333-444455556666",
    });
    for (const [channel, body] of [
      ["pick-path", {}],
      ["pick-path", { purpose: "anywhere" }],
      ["pick-path", { purpose: "drive-root", path: "/etc" }],
      ["engine-set-executable", {}],
      ["engine-set-executable", { grantId: "../../x" }],
      ["engine-set-executable", { grantId: 7 }],
    ]) {
      expect(() => parseSuperOntologyRequest(channel, body)).toThrow(expect.objectContaining({ code: "SO_INVALID_REQUEST" }));
    }
  });

  describe("control API channels", () => {
    const DRIVE = "00000000-0000-4000-8000-000000000001";
    const OP = "op_0123456789abcdef";

    it("accepts well-formed requests and normalizes them", () => {
      expect(parseSuperOntologyRequest("drive-create", { operationId: OP, name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001", exclude: ["*.bak"], llmDailyCalls: 5 }))
        .toEqual({ operationId: OP, name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001", exclude: ["*.bak"], llmDailyCalls: 5 });
      expect(parseSuperOntologyRequest("drive-update", { driveId: DRIVE, revision: 3, patch: { paused: true, review: { auto_publish_bp: 8000 } } }).patch)
        .toEqual({ paused: true, review: { auto_publish_bp: 8000 } });
      expect(parseSuperOntologyRequest("drive-tree", { driveId: DRIVE, path: "" })).toEqual({ driveId: DRIVE, path: "" });
      expect(parseSuperOntologyRequest("drive-tree", { driveId: DRIVE, path: "Projects/a b/中文" }).path).toBe("Projects/a b/中文");
      expect(parseSuperOntologyRequest("files-list", { driveId: DRIVE, states: ["queued"], sort: "path", limit: 500, cursor: "abc_-=" }))
        .toEqual({ driveId: DRIVE, states: ["queued"], sort: "path", limit: 500, cursor: "abc_-=" });
    });

    it.each([
      ["drive-get", { driveId: "../x" }],
      ["drive-get", { driveId: "0000000A-0000-4000-8000-00000000000A" }],
      ["drive-create", { name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001" }],
      ["drive-create", { operationId: OP, name: "", backend: "fs", rootGrantId: "grant-0001-0001" }],
      ["drive-create", { operationId: OP, name: "x", backend: "s3", rootGrantId: "grant-0001-0001" }],
      ["drive-create", { operationId: OP, name: "x", backend: "fs", rootGrantId: "grant-0001-0001", root: "/etc" }],
      ["drive-create", { operationId: OP, name: "x", backend: "fs", rootGrantId: "grant-0001-0001", llmDailyCalls: -1 }],
      ["drive-update", { driveId: DRIVE, revision: -1, patch: { paused: true } }],
      ["drive-update", { driveId: DRIVE, revision: 1, patch: {} }],
      ["drive-update", { driveId: DRIVE, revision: 1, patch: { backend: "s3" } }],
      ["drive-update", { driveId: DRIVE, revision: 1, patch: { review: { auto_publish_bp: 100 } } }],
      ["drive-update", { driveId: DRIVE, revision: 1, patch: { exclude: new Array(65).fill("x") } }],
      ["drive-sync", { driveId: DRIVE }],
      ["drive-sync", { operationId: OP, driveId: DRIVE, confirmMassDelete: "3" }],
      ["drive-tree", { driveId: DRIVE, path: "../x" }],
      ["drive-tree", { driveId: DRIVE, path: "/abs" }],
      ["drive-tree", { driveId: DRIVE, path: "a//b" }],
      ["drive-tree", { driveId: DRIVE, path: "a/./b" }],
      ["drive-tree", { driveId: DRIVE, path: "a\u0000b" }],
      ["drive-tree", { driveId: DRIVE, path: "x".repeat(1025) }],
      ["files-list", { driveId: DRIVE, limit: 501 }],
      ["files-list", { driveId: DRIVE, sort: "name" }],
      ["files-list", { driveId: DRIVE, states: ["Queued"] }],
      ["files-list", { driveId: DRIVE, states: [] }],
      ["files-list", { driveId: DRIVE, cursor: "a b" }],
      ["files-list", { driveId: DRIVE, q: "x".repeat(257) }],
    ])("rejects %s %j", (channel, body) => {
      expect(() => parseSuperOntologyRequest(channel, body)).toThrow(expect.objectContaining({ code: "SO_INVALID_REQUEST" }));
    });
  });

  it("validates the event channels", () => {
    expect(parseSuperOntologyRequest("events-subscribe", {})).toEqual({});
    expect(parseSuperOntologyRequest("events-subscribe", { sinceSeq: 12 })).toEqual({ sinceSeq: 12 });
    expect(parseSuperOntologyRequest("events-since", { instanceId: "42:2026", seq: 0 })).toEqual({ instanceId: "42:2026", seq: 0 });
    for (const [channel, body] of [
      ["events-subscribe", { sinceSeq: -1 }], ["events-subscribe", { sinceSeq: 1.5 }], ["events-since", { seq: 1 }],
      ["events-since", { instanceId: "", seq: 1 }], ["events-since", { instanceId: "x", seq: "1" }], ["events-unsubscribe", { x: 1 }],
    ]) {
      expect(() => parseSuperOntologyRequest(channel, body)).toThrow(expect.objectContaining({ code: "SO_INVALID_REQUEST" }));
    }
  });

  it("covers exactly the implemented channels", () => {
    expect([...SUPER_ONTOLOGY_CHANNELS].slice(0, 33)).toEqual([
      "engine-read", "engine-preflight", "engine-start", "engine-stop", "engine-restart", "engine-set-keep",
      "engine-reveal-logs", "pick-path", "engine-set-executable",
      "drives-list", "drive-get", "drive-create", "my-files-ensure", "drive-update", "drive-delete", "drive-sync", "drive-tree", "files-list",
      "events-subscribe", "events-since", "events-unsubscribe", "upload-start", "upload-cancel",
      "files-retry", "files-retry-matching", "file-detail", "file-blocks", "file-preview-url",
      "suggestions-list", "suggestions-digest", "suggestion-diff", "suggestion-decide", "suggestions-decide-source",
    ]);
    // The L4 and L6 channels follow, exactly as ontology-requests.mjs lists them.
    expect([...SUPER_ONTOLOGY_CHANNELS].slice(33, 33 + ONTOLOGY_CHANNELS.length)).toEqual([...ONTOLOGY_CHANNELS]);
    // The L7 and L8 channels (Cloud mode, audit, runtime) come last.
    expect([...SUPER_ONTOLOGY_CHANNELS].slice(33 + ONTOLOGY_CHANNELS.length)).toEqual([...ENTERPRISE_CHANNELS, ...AGENT_CHANNELS]);
  });

  it("validates the upload channels", () => {
    const batch = "00000000-0000-4000-8000-0000000000b1";
    const DRIVE = "00000000-0000-4000-8000-000000000001";
    expect(parseSuperOntologyRequest("upload-start", { driveId: DRIVE, folder: "a/b", paths: ["/x/y.md"] })).toEqual({ driveId: DRIVE, folder: "a/b", paths: ["/x/y.md"] });
    expect(parseSuperOntologyRequest("upload-cancel", { batchId: batch })).toEqual({ batchId: batch });
    for (const bad of [
      ["upload-start", { driveId: DRIVE, folder: "../up", paths: [] }],
      ["upload-start", { driveId: DRIVE, folder: "", paths: "x" }],
      ["upload-start", { driveId: DRIVE, folder: "", paths: ["a\u0000b"] }],
      ["upload-start", { driveId: DRIVE, folder: "", paths: ["/x"], overwrite: true }],
      ["upload-cancel", { batchId: "nope" }],
    ]) expect(() => parseSuperOntologyRequest(bad[0], bad[1])).toThrow();
  });
});

describe("toErrorResult", () => {
  it("keeps known SO_* codes and drops everything else", () => {
    const result = toErrorResult(new SuperOntologyError("SO_TIMEOUT", { retryable: true, details: { token: "x" }, httpStatus: 504 }));
    expect(result).toEqual({ ok: false, error: { code: "SO_TIMEOUT", retryable: true, details: {}, httpStatus: 504 } });
  });

  it("never leaks a native error message", () => {
    const result = toErrorResult(new Error("Authorization: Bearer oodt_secret"));
    expect(result).toEqual({ ok: false, error: { code: "SO_INTERNAL", retryable: false, details: {} } });
    expect(JSON.stringify(result)).not.toContain("oodt_");
  });

  it("treats an unlisted code as internal", () => {
    expect(toErrorResult(new SuperOntologyError("NOT_A_CODE")).error.code).toBe("SO_INTERNAL");
    expect(SO_ERROR_CODES).toContain("SO_INTERNAL");
  });
});

describe("drive-create for an object-store Drive (F-S4-01)", () => {
  const base = { operationId: "op_0123456789abcdef", name: "Team", backend: "s3", endpoint: "https://s3.example.com", bucket: "team-docs", accessKeyId: "AKIAEXAMPLE", secretAccessKey: "secret-value" };

  it("takes the object-store fields and no grant", () => {
    expect(parseSuperOntologyRequest("drive-create", { ...base, region: "eu-west-1", prefix: "shared/", pathStyle: true, sessionToken: "t", exclude: ["*.bak"] })).toMatchObject({ backend: "s3", bucket: "team-docs", pathStyle: true });
  });

  it("refuses a grant on an object-store Drive and object-store fields on a folder Drive", () => {
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, rootGrantId: "grant-0001-0001" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { operationId: base.operationId, name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001", bucket: "x-bucket" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { operationId: base.operationId, name: "Docs", backend: "fs" })).toThrow();
  });

  it("refuses an unknown backend, a short bucket name and control characters in a key", () => {
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, backend: "gcs" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, bucket: "ab" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, secretAccessKey: "a\nb" })).toThrow();
  });
});

describe("drive-create for oss, cos and r2 and the access policy (F-S11-01)", () => {
  const base = { operationId: "op_0123456789abcdef", name: "Team", bucket: "team-docs", accessKeyId: "AKIAEXAMPLE", secretAccessKey: "secret-value" };
  const ACCOUNT = "0123456789abcdef0123456789abcdef";

  it("takes oss and cos with a region and no endpoint, and the in-cloud switch", () => {
    for (const backend of ["oss", "cos"]) {
      expect(parseSuperOntologyRequest("drive-create", { ...base, backend, region: "cn-hangzhou", internal: true })).toMatchObject({ backend, region: "cn-hangzhou", internal: true });
    }
  });

  it("takes r2 with a 32 character account id and refuses another length", () => {
    expect(parseSuperOntologyRequest("drive-create", { ...base, backend: "r2", accountId: ACCOUNT })).toMatchObject({ backend: "r2", accountId: ACCOUNT });
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, backend: "r2", accountId: "short" })).toThrow();
  });

  it("refuses a grant on any of the three and object-store fields on a folder Drive", () => {
    for (const backend of ["oss", "cos", "r2"]) expect(() => parseSuperOntologyRequest("drive-create", { ...base, backend, rootGrantId: "grant-0001-0001" })).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { operationId: base.operationId, name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001", accountId: ACCOUNT })).toThrow();
  });

  it("takes allow and deny prefixes on every backend, drops a trailing slash and refuses empty or escaping entries", () => {
    expect(parseSuperOntologyRequest("drive-create", { ...base, backend: "oss", region: "cn-hangzhou", allowPrefixes: ["Projects/", "People"], denyPrefixes: ["Projects/Payroll"] }))
      .toMatchObject({ allowPrefixes: ["Projects", "People"], denyPrefixes: ["Projects/Payroll"] });
    expect(parseSuperOntologyRequest("drive-create", { operationId: base.operationId, name: "Docs", backend: "fs", rootGrantId: "grant-0001-0001", denyPrefixes: ["Private"] })).toMatchObject({ denyPrefixes: ["Private"] });
    for (const bad of [[""], ["/"], ["../x"], ["a//b"], [1]]) expect(() => parseSuperOntologyRequest("drive-create", { ...base, backend: "cos", region: "ap-beijing", denyPrefixes: bad }), JSON.stringify(bad)).toThrow();
    expect(() => parseSuperOntologyRequest("drive-create", { ...base, backend: "cos", region: "ap-beijing", allowPrefixes: Array.from({ length: 33 }, (_, index) => `p${index}`) })).toThrow();
  });
});

describe("suggest channels (F-S7-01, F-S8-01)", () => {
  const SHARE = "00000000-0000-4000-8000-0000000000c1";
  const ID = "00000000-0000-4000-8000-0000000000e1";
  it("parses the list, digest, diff and decide requests", () => {
    expect(parseSuperOntologyRequest("suggestions-list", { status: "pending", shareId: SHARE, day: "2026-11-18", limit: 200 })).toMatchObject({ status: "pending", shareId: SHARE });
    expect(parseSuperOntologyRequest("suggestions-digest", {})).toEqual({});
    expect(parseSuperOntologyRequest("suggestions-digest", { day: "2026-11-18" })).toEqual({ day: "2026-11-18" });
    expect(parseSuperOntologyRequest("suggestion-diff", { id: ID })).toEqual({ id: ID });
    expect(parseSuperOntologyRequest("suggestion-decide", { id: ID, decision: "undo", revision: 3, note: "wrong" })).toMatchObject({ decision: "undo", revision: 3 });
    expect(parseSuperOntologyRequest("suggestions-decide-source", { shareId: SHARE, decision: "accept" })).toEqual({ shareId: SHARE, decision: "accept" });
  });
  it("refuses unknown values, bad ids and a bulk undo", () => {
    expect(() => parseSuperOntologyRequest("suggestions-list", { status: "maybe" })).toThrow();
    expect(() => parseSuperOntologyRequest("suggestions-digest", { day: "today" })).toThrow();
    expect(() => parseSuperOntologyRequest("suggestion-diff", { id: "1" })).toThrow();
    expect(() => parseSuperOntologyRequest("suggestion-decide", { id: ID, decision: "maybe" })).toThrow();
    expect(() => parseSuperOntologyRequest("suggestions-decide-source", { shareId: SHARE, decision: "undo" })).toThrow();
    expect(() => parseSuperOntologyRequest("suggestion-decide", { id: ID, decision: "accept", extra: 1 })).toThrow();
  });
});
