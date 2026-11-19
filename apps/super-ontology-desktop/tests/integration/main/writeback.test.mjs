import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSuggestHandlers } from "@so-main/suggest-handlers.mjs";
import { applyChanges, createWriteback, lineRangeOf } from "@so-main/writeback.mjs";

const SID = "00000000-0000-4000-8000-0000000000e1";
const SHARE = "00000000-0000-4000-8000-0000000000c1";
const DRIVE = "00000000-0000-4000-8000-000000000001";
let root;
let journal;
beforeEach(() => { root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "sd-wb-"))); journal = path.join(root, "state", "writeback", "journal.jsonl"); fs.mkdirSync(path.join(root, "drive")); });
afterEach(() => { fs.rmSync(root, { recursive: true, force: true }); });

const text = "# Decisions\n\nAdopt plan one (2026-08)\n\nNotes\n";
const file = () => path.join(root, "drive", "decisions.md");
const change = (extra = {}) => ({ file_id: "f1", path: "decisions.md", anchor: { kind: "text", line_start: 3, line_end: 3 }, block_id: "b1", source_b3: "b3:x", operation: "replace", after: "Adopt plan two (2026-11)", before_ref: "[src:decisions.md#L3-L3]", ...extra });
const diff = (changes = [change()], extra = {}) => ({ suggestion_id: SID, share_id: SHARE, kind: "supersede", rule: "supersede_in_use", role: "editor", writable: true, writer: "desktop", changes, ...extra });

describe("applying changes to text (F-S8-03)", () => {
  it("replaces anchored lines, inserts after them, keeps line endings and applies from the bottom", () => {
    expect(applyChanges(text, [change()]).text).toBe("# Decisions\n\nAdopt plan two (2026-11)\n\nNotes\n");
    expect(applyChanges(text, [change({ operation: "insert_after", after: "- cost" })]).text).toBe("# Decisions\n\nAdopt plan one (2026-08)\n- cost\n\nNotes\n");
    expect(applyChanges("a\r\nb\r\n", [change({ anchor: { line_start: 1, line_end: 1 }, after: "A" })]).text).toBe("A\r\nb\r\n");
    const two = applyChanges(text, [change({ anchor: { line_start: 1, line_end: 1 }, after: "# Plans" }), change()]);
    expect(two.text).toBe("# Plans\n\nAdopt plan two (2026-11)\n\nNotes\n");
    expect(two.touched.map((entry) => entry.first_line)).toEqual([3, 1]);
  });
  it("leaves annotations out, and refuses what it cannot place safely", () => {
    expect(applyChanges(text, [change({ operation: "annotate" })])).toMatchObject({ text, skipped: [{ reason: "annotate" }] });
    expect(() => applyChanges(text, [change({ anchor: { line_start: 99, line_end: 99 } })])).toThrow();
    expect(() => applyChanges(text, [change({ after: "two\nlines" })])).toThrow();
    expect(() => applyChanges(text, [change({ operation: "delete" })])).toThrow();
    expect(() => applyChanges(text, [change(), change({ after: "x" })])).toThrow();
  });
  it("reads the line range from the anchor or the fragment", () => {
    expect(lineRangeOf({ anchor: { line_start: 2, line_end: 4 } })).toEqual({ start: 2, end: 4 });
    expect(lineRangeOf({ frag: "#L7-L9" })).toEqual({ start: 7, end: 9 });
    expect(lineRangeOf({ frag: "#p3" })).toBeNull();
  });
});

function setup({ role = "editor", fileB3 = "b3:x", modified } = {}) {
  fs.writeFileSync(file(), text);
  const stat = fs.statSync(file());
  const calls = [];
  const client = {
    request: vi.fn(async (name, options) => {
      calls.push([name, options]);
      switch (name) {
        case "driveGet": return { value: { id: DRIVE, backend: "fs", config: { root: path.join(root, "drive") } } };
        case "fileDetail": return { value: { id: "f1", b3: fileB3, modified_at: modified ?? new Date(stat.mtimeMs).toISOString() } };
        case "suggestionGet": return { value: { id: SID, share_id: SHARE, drive_id: DRIVE, kind: "supersede", status: "pending", revision: 1, credential_id: "c1", submitted_by: "credential" } };
        case "suggestionDiff": return { value: diff([change()], { role, writable: role === "editor" }) };
        case "suggestionDecide": return { value: { id: SID, share_id: SHARE, kind: "supersede", status: options.body.decision === "accept" ? "accepted" : "undone", revision: 2 } };
        case "driveSync": return { value: {} };
        default: throw new Error(name);
      }
    }),
  };
  const onWritten = vi.fn(async () => undefined);
  const writeback = createWriteback({ client, journalFile: journal, onWritten, logger: { debug() {} } });
  return { handlers: createSuggestHandlers({ client, writeback }), calls, onWritten };
}

describe("accepting an editor suggestion writes the file (F-S8-03)", () => {
  it("checks, accepts at the engine, writes atomically, syncs the path, journals suggestion, Share and evidence, and nudges Git", async () => {
    const { handlers, calls, onWritten } = setup();
    const result = await handlers["suggestion-decide"]({ id: SID, decision: "accept", revision: 1 });
    expect(fs.readFileSync(file(), "utf8")).toBe("# Decisions\n\nAdopt plan two (2026-11)\n\nNotes\n");
    expect(result.writeback.written).toEqual([{ path: "decisions.md", operations: [{ block_id: "b1", operation: "replace", first_line: 3 }], skipped: [] }]);
    expect(calls.map(([name]) => name)).toEqual(["suggestionGet", "suggestionDiff", "driveGet", "fileDetail", "suggestionDecide", "driveSync"]);
    expect(calls.at(-1)[1].body).toEqual({ paths: ["decisions.md"] });
    const [entry] = fs.readFileSync(journal, "utf8").trim().split("\n").map((line) => JSON.parse(line));
    expect(entry).toMatchObject({ event: "written", suggestion_id: SID, share_id: SHARE, path: "decisions.md", credential_id: "c1", role: "editor", evidence: ["[src:decisions.md#L3-L3]"] });
    expect(onWritten).toHaveBeenCalledWith(expect.objectContaining({ relativePath: "decisions.md", suggestionId: SID }));
    expect(fs.readdirSync(path.join(root, "drive"))).toEqual(["decisions.md"]);
  });

  it("a file the engine has not read in its current form is a conflict: nothing written, nothing accepted", async () => {
    const stale = setup({ fileB3: "b3:other" });
    await expect(stale.handlers["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_WRITEBACK_CONFLICT" });
    expect(stale.calls.some(([name, options]) => name === "suggestionDecide" && options.body.decision === "accept")).toBe(false);
    expect(fs.readFileSync(file(), "utf8")).toBe(text);
    const edited = setup({ modified: "2020-01-01T00:00:00Z" });
    await expect(edited.handlers["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_WRITEBACK_CONFLICT" });
    expect(fs.readFileSync(file(), "utf8")).toBe(text);
  });

  it("a viewer or suggester suggestion is only accepted at the engine; reject and undo never touch files", async () => {
    const suggester = setup({ role: "suggester" });
    const result = await suggester.handlers["suggestion-decide"]({ id: SID, decision: "accept" });
    expect(result.writeback).toBeNull();
    expect(fs.readFileSync(file(), "utf8")).toBe(text);
    const editor = setup();
    await editor.handlers["suggestion-decide"]({ id: SID, decision: "reject" });
    await editor.handlers["suggestion-decide"]({ id: SID, decision: "undo" });
    expect(fs.readFileSync(file(), "utf8")).toBe(text);
    expect(editor.calls.every(([name]) => name === "suggestionDecide")).toBe(true);
  });

  it("an object-storage Drive, a non-text file and a path outside the folder are refused before anything happens", async () => {
    const s3 = { request: vi.fn(async (name) => (name === "driveGet" ? { value: { id: DRIVE, backend: "s3", config: {} } } : name === "suggestionGet" ? { value: { id: SID, share_id: SHARE, drive_id: DRIVE, kind: "supersede", status: "pending", revision: 1 } } : { value: diff() })) };
    const wb = createWriteback({ client: s3, logger: { debug() {} } });
    await expect(createSuggestHandlers({ client: s3, writeback: wb })["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_WRITEBACK_UNSUPPORTED" });
    const pdf = { request: vi.fn(async (name) => (name === "driveGet" ? { value: { id: DRIVE, backend: "fs", config: { root: path.join(root, "drive") } } } : name === "suggestionGet" ? { value: { id: SID, share_id: SHARE, drive_id: DRIVE, kind: "supersede", status: "pending", revision: 1 } } : { value: diff([change({ path: "x.pdf" })]) })) };
    await expect(createSuggestHandlers({ client: pdf, writeback: createWriteback({ client: pdf, logger: { debug() {} } }) })["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_WRITEBACK_UNSUPPORTED" });
    const escape = { request: vi.fn(async (name) => (name === "driveGet" ? { value: { id: DRIVE, backend: "fs", config: { root: path.join(root, "drive") } } } : name === "suggestionGet" ? { value: { id: SID, share_id: SHARE, drive_id: DRIVE, kind: "supersede", status: "pending", revision: 1 } } : { value: diff([change({ path: "../outside.md" })]) })) };
    await expect(createSuggestHandlers({ client: escape, writeback: createWriteback({ client: escape, logger: { debug() {} } }) })["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_INVALID_REQUEST" });
  });

  it("a failed write puts the file back and takes the acceptance back", async () => {
    const { handlers, calls } = setup();
    const spy = vi.spyOn(fs.promises, "rename").mockImplementation(async () => { throw Object.assign(new Error("denied"), { code: "EACCES" }); });
    await expect(handlers["suggestion-decide"]({ id: SID, decision: "accept" })).rejects.toMatchObject({ code: "SO_UPLOAD_PERMISSION_DENIED" });
    spy.mockRestore();
    expect(fs.readFileSync(file(), "utf8")).toBe(text);
    expect(calls.filter(([name]) => name === "suggestionDecide").map(([, options]) => options.body.decision)).toEqual(["accept", "undo"]);
    expect(fs.readdirSync(path.join(root, "drive"))).toEqual(["decisions.md"]);
  });

  it("accept-all from a source decides each pending suggestion and reports files written and failures", async () => {
    const stub = createSuggestHandlers({ client: { request: async (name, options) => {
      if (name === "suggestionsList") return { value: { items: [{ id: SID, share_id: SHARE, kind: "supersede", status: "pending", revision: 1 }, { id: "00000000-0000-4000-8000-0000000000e2", share_id: SHARE, kind: "supersede", status: "pending", revision: 1 }] } };
      if (name === "suggestionGet") return { value: { id: options.params.id, share_id: SHARE, drive_id: DRIVE, kind: "supersede", status: "pending", revision: 1 } };
      if (name === "suggestionDiff") return { value: diff([change()], { writable: false, role: "suggester" }) };
      if (options.params.id === SID) return { value: { id: SID, share_id: SHARE, kind: "supersede", status: "accepted", revision: 2 } };
      throw Object.assign(new Error("x"), { code: "DOCS_SUGGEST_NOT_PENDING" });
    } }, writeback: null });
    const result = await stub["suggestions-decide-source"]({ shareId: SHARE, decision: "accept" });
    expect(result).toMatchObject({ decided: 1, failed: 1 });
    expect(result.results.map((entry) => entry.status)).toEqual(["accepted", "failed"]);
  });
});
