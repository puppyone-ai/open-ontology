// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { analyzeUnlinked, fileReason, formatOf, reasonForRow, readUnlinked, resetUnlinked, UNLINKED_REFRESH_DEBOUNCE_MS, useUnlinked, type UnlinkedReport, type UnlinkedSource } from "../../../../src/renderer/model/unlinked";
import { applyUnlinked, GROUP_ENTRY_PREFIX, GROUP_ROWS_MAX, UNLINKED_GROUP_ID, unlinkedCount } from "../../../../src/renderer/features/atlas/unlinked";
import { AtlasList } from "../../../../src/renderer/features/atlas/AtlasList";
import type { AtlasData } from "../../../../src/renderer/features/atlas/model";
import { catalog } from "../../../support/locales";

const file = (over: Record<string, unknown>) => ({ id: "f1", drive_id: "d1", path: "docs/a.md", state: "published", pending_review: 0, ...over }) as never;

describe("why a file is not connected", () => {
  it("names every reason from the file's state", () => {
    expect(fileReason(file({ state: "unsupported", path: "x/model.psd" }), null)).toEqual({ kind: "unsupported", format: "psd" });
    expect(fileReason(file({ state: "unsupported", format: "dwg" }), null)).toEqual({ kind: "unsupported", format: "dwg" });
    expect(fileReason(file({ state: "unsupported", path: "noext" }), null)).toEqual({ kind: "unsupported", format: "?" });
    expect(fileReason(file({ state: "parse_failed", state_code: "PARSE_PDF_CORRUPT" }), null)).toEqual({ kind: "parse_failed", code: "PARSE_PDF_CORRUPT" });
    expect(fileReason(file({ state: "too_large" }), null)).toEqual({ kind: "too_large" });
    expect(fileReason(file({ state: "encrypted" }), null)).toEqual({ kind: "encrypted" });
    for (const state of ["stored", "queued", "parsing", "parsed", "extracting", "indexed"]) expect(fileReason(file({ state }), null)).toEqual({ kind: "processing" });
    // Indexed with facts waiting for a person is a review, not work in progress.
    expect(fileReason(file({ state: "indexed", pending_review: 3 }), null)).toEqual({ kind: "review" });
    expect(fileReason(file({ state: "extracting", pending_review: 3 }), null)).toEqual({ kind: "processing" });
    for (const state of ["deleted", "superseded", "duplicate", "outdated"]) expect(fileReason(file({ state }), null)).toBeNull();
  });
  it("a ready file needs to have been read: no entities, waiting for review, or entities none of which appear in another file", () => {
    expect(fileReason(file({}), null)).toBeNull();
    expect(fileReason(file({ pending_review: 2 }), null)).toEqual({ kind: "review" });
    expect(fileReason(file({}), { entities: 0, connected: false })).toEqual({ kind: "no_entities" });
    expect(fileReason(file({ pending_review: 1 }), { entities: 0, connected: false })).toEqual({ kind: "review" });
    expect(fileReason(file({}), { entities: 3, connected: false })).toEqual({ kind: "no_relations", count: 3 });
    expect(fileReason(file({}), { entities: 3, connected: true })).toBeNull();
  });
  it("reads the format from the extension, lower case", () => {
    expect(formatOf({ path: "a/B.HEIC" })).toBe("heic");
    expect(formatOf({ path: ".hidden" })).toBe("?");
  });
});

const ent = (key: string, type = "Person") => ({ key, name: key.toUpperCase(), type });
function source(files: Record<string, unknown>[], details: Record<string, { entities?: unknown[]; facts?: unknown[] }>, linked: string[] = [], speakers: Record<string, number> = {}): UnlinkedSource {
  return {
    speakerPending: async () => new Map(Object.entries(speakers)),
    drives: async () => ({ ok: true, value: [{ id: "d1", name: "Docs" }] }),
    files: async () => ({ ok: true, value: { items: files as never, next_cursor: null } }),
    detail: async (id) => (details[id] ? { ok: true, value: { id, ...details[id] } as never } : { ok: false, error: { code: "X", retryable: false, details: {} } }),
    linkedFilePaths: async () => new Set(linked),
  };
}
const f = (id: string, over: Record<string, unknown> = {}) => ({ id, drive_id: "d1", path: `docs/${id}.md`, state: "published", pending_review: 0, ...over });

describe("reading the files", () => {
  it("a recording with unconfirmed speakers says so, whatever else it produced", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b"), f("r")], {
      a: { entities: [ent("x")], facts: [] }, b: { entities: [ent("x")], facts: [] }, r: { entities: [ent("x")], facts: [] },
    }, [], { r: 2 }));
    expect(report.files).toEqual([expect.objectContaining({ fileId: "r", reason: { kind: "speaker", count: 2 } })]);
    expect(fileReason(file({ state: "indexed", pending_review: 2 }), null, 2)).toEqual({ kind: "speaker", count: 2 });
    expect(fileReason(file({ state: "unsupported" }), null, 2)).toMatchObject({ kind: "unsupported" });
  });

  it("judges connectivity across files: a file whose own entities relate only to each other is not connected", async () => {
    const report = await analyzeUnlinked(source(
      [f("a"), f("b"), f("c"), f("d", { state: "unsupported", format: "psd" }), f("e", { state: "parsing" }), f("g"), f("h")],
      {
        a: { entities: [ent("ann"), ent("proj", "Project")], facts: [{ subject: ent("ann"), object: ent("proj", "Project"), status: "published" }] },
        b: { entities: [ent("ann")], facts: [] },
        c: { entities: [], facts: [] },
        g: { entities: [ent("lonely")], facts: [] },
        h: { entities: [ent("flour"), ent("tart")], facts: [{ subject: ent("tart"), object: ent("flour"), status: "published" }] },
      },
    ));
    const byId = Object.fromEntries(report.files.map((x) => [x.fileId, x.reason]));
    expect(byId).toEqual({ c: { kind: "no_entities" }, d: { kind: "unsupported", format: "psd" }, e: { kind: "processing" }, g: { kind: "no_relations", count: 1 }, h: { kind: "no_relations", count: 2 } });
    // Islands that touch one file only: the lone entity and the recipe pair (with the file named); ann and proj reach two files.
    expect(report.entities.map((x) => x.key).sort()).toEqual(["FLOUR", "LONELY", "TART"].map((k) => k.toLowerCase()));
    expect(report.entities.find((x) => x.key === "lonely")).toMatchObject({ fileIds: ["g"], fileNames: ["g.md"] });
    expect(report.complete).toBe(true);
    expect(report.read).toContain("a");
    expect(reasonForRow({ ...f("a") } as never, report)).toBeNull();
    expect(reasonForRow({ ...f("h") } as never, report)).toEqual({ kind: "no_relations", count: 2 });
  });

  it("a thing in two files connects both; a relation reaching a thing another file holds connects too", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b"), f("c")], {
      a: { entities: [ent("x")], facts: [{ subject: ent("x"), object: ent("y"), status: "published" }] },
      b: { entities: [ent("y")], facts: [] },
      c: { entities: [ent("z")], facts: [] },
    }));
    expect(report.files.map((x) => x.fileId)).toEqual(["c"]);
    expect(report.entities.map((x) => x.key)).toEqual(["z"]);
  });

  it("with a single file there is nothing to compare, so nothing is called unconnected", async () => {
    const report = await analyzeUnlinked(source([f("a")], { a: { entities: [ent("x")], facts: [] } }));
    expect(report.files).toEqual([]);
    expect(report.entities).toEqual([]);
  });

  it("the Document entity the engine names after every file does not count as an entity found", async () => {
    const doc = (name: string) => ({ key: name, name, type: "Document" });
    const report = await analyzeUnlinked(source([f("a"), f("b", { state: "unsupported", format: "unknown", path: "x/plan.dwg" })], { a: { entities: [doc("docs/a.md")], facts: [] } }));
    expect(report.files.map((x) => [x.fileId, x.reason])).toEqual([["a", { kind: "no_entities" }], ["b", { kind: "unsupported", format: "dwg" }]]);
    expect(report.entities).toEqual([]);
  });

  it("a file that links to a table row is connected", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b"), f("c")], {
      a: { entities: [ent("x")], facts: [{ subject: ent("x"), object: ent("y"), status: "published" }] },
      b: { entities: [ent("y")], facts: [] },
      c: { entities: [ent("z")], facts: [] },
    }, ["d1|docs/c.md"]));
    expect(report.files).toEqual([]);
    expect(report.entities).toEqual([]);
  });

  it("a retracted fact does not connect, and a file that failed to read is never called unconnected", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b"), f("c")], {
      a: { entities: [ent("x")], facts: [{ subject: ent("x"), object: ent("y"), status: "retracted" }] },
      c: { entities: [ent("y")], facts: [] },
    }));
    expect(report.files.map((x) => x.fileId)).toEqual(["a", "c"]);
    expect(report.complete).toBe(false);
    expect(report.files.find((x) => x.fileId === "b")).toBeUndefined();
  });

  it("stops at the budget and says so, without calling unread files unconnected", async () => {
    const files = Array.from({ length: 5 }, (_, i) => f(`n${i}`));
    const details = Object.fromEntries(files.map((x) => [x.id, { entities: [], facts: [] }]));
    const report = await analyzeUnlinked(source(files, details), { maxFiles: 2 });
    expect(report.limited).toBe(true);
    expect(report.checkedFiles).toBe(2);
    expect(report.files).toHaveLength(2);
  });
});

const report = (over: Partial<UnlinkedReport> = {}): UnlinkedReport => ({ files: [], entities: [], totalFiles: 0, checkedFiles: 0, read: [], limited: false, complete: true, ...over });
const base = (): AtlasData => ({
  groups: [{ id: "g:type:Person", kind: "type", label: "Person", tone: "ink" }, { id: "g:type:Topic", kind: "type", label: "Topic", tone: "mist" }],
  members: [
    { id: "e:ann", group: "g:type:Person", label: "Ann", ref: { kind: "entity", key: "ann" }, weight: 1, born: null },
    { id: "e:solo", group: "g:type:Topic", label: "Solo", ref: { kind: "entity", key: "solo" }, weight: 0, born: null },
  ],
  links: [], versions: [], partial: [],
});

describe("the not-connected group of the atlas", () => {
  it("moves isolated entities and unlinked files into one side group and drops the group it emptied", () => {
    const data = applyUnlinked(base(), report({
      entities: [{ key: "solo", name: "Solo", type: "Topic", fileIds: ["f1"] }],
      files: [{ fileId: "f9", driveId: "d1", path: "docs/scan.psd", name: "scan.psd", reason: { kind: "unsupported", format: "psd" } }],
    }));
    expect(data.groups.map((g) => g.id)).toEqual(["g:type:Person", UNLINKED_GROUP_ID]);
    expect(data.groups.at(-1)).toMatchObject({ kind: "unlinked", tone: "stone" });
    const side = data.members.filter((m) => m.group === UNLINKED_GROUP_ID);
    expect(side.map((m) => [m.label, m.reason?.kind])).toEqual([["Solo", "entity"], ["scan.psd", "unsupported"]]);
    expect(side[1]!.ref).toEqual({ kind: "file", fileId: "f9", path: "docs/scan.psd" });
    expect(unlinkedCount(data)).toBe(2);
  });
  it("shows the files of a code folder as one entry per reason, with a count and the biggest formats, not one row each", () => {
    const formats = ["ts", "ts", "ts", "py", "py", "rs"];
    const unsupported = Array.from({ length: 28_000 }, (_, index) => ({ fileId: `u${index}`, driveId: "d1", path: `src/f${index}.${formats[index % formats.length]}`, name: `f${index}.${formats[index % formats.length]}`, reason: { kind: "unsupported" as const, format: formats[index % formats.length]! } }));
    const processing = Array.from({ length: 20_000 }, (_, index) => ({ fileId: `p${index}`, driveId: "d1", path: `src/p${index}.md`, name: `p${index}.md`, reason: { kind: "processing" as const } }));
    const odd = { fileId: "m1", driveId: "d1", path: "docs/none.md", name: "none.md", reason: { kind: "no_entities" as const } };
    const out = applyUnlinked(base(), report({ files: [...unsupported, ...processing, odd] }));
    const side = out.members.filter((m) => m.group === UNLINKED_GROUP_ID);
    expect(side).toHaveLength(3);
    expect(side.find((m) => m.id === `${GROUP_ENTRY_PREFIX}unsupported`)).toMatchObject({ reason: { kind: "group", of: "unsupported", count: 28_000, detail: "ts ×14001, py ×9333, rs ×4666" } });
    expect(side.find((m) => m.id === `${GROUP_ENTRY_PREFIX}processing`)).toMatchObject({ reason: { kind: "group", of: "processing", count: 20_000 } });
    expect(side.find((m) => m.id === "f:m1")?.reason).toEqual({ kind: "no_entities" });
    expect(unlinkedCount(out)).toBe(48_001);
  });
  it("keeps a few files of one reason as rows", () => {
    const few = Array.from({ length: GROUP_ROWS_MAX }, (_, index) => ({ fileId: `u${index}`, driveId: "d1", path: `a/f${index}.psd`, name: `f${index}.psd`, reason: { kind: "unsupported" as const, format: "psd" } }));
    const out = applyUnlinked(base(), report({ files: few }));
    expect(out.members.filter((m) => m.group === UNLINKED_GROUP_ID)).toHaveLength(GROUP_ROWS_MAX);
    expect(out.members.some((m) => m.id.startsWith(GROUP_ENTRY_PREFIX))).toBe(false);
  });
  it("leaves the data alone when everything is connected", () => {
    const data = base();
    expect(applyUnlinked(data, report())).toBe(data);
    expect(applyUnlinked(data, null)).toBe(data);
  });
  it("in the source view a listed file is moved, not duplicated", () => {
    const source: AtlasData = { groups: [{ id: "g:drive:d1", kind: "drive", label: "Docs", tone: "ink", source: "folder" }], members: [{ id: "f:f9", group: "g:drive:d1", label: "scan.psd", ref: { kind: "file", fileId: "f9", path: "docs/scan.psd" }, weight: 0, born: null }], links: [], versions: [], partial: [] };
    const out = applyUnlinked(source, report({ files: [{ fileId: "f9", driveId: "d1", path: "docs/scan.psd", name: "scan.psd", reason: { kind: "no_entities" } }] }));
    expect(out.members).toHaveLength(1);
    expect(out.members[0]!.group).toBe(UNLINKED_GROUP_ID);
  });
});

const messages = mergeCatalogNamespaces({ app: catalog("en", "app"), "super-drive": catalog("en", "super-drive") });
afterEach(cleanup);

describe("the not-connected list", () => {
  const data = (): AtlasData => ({
    ...applyUnlinked(base(), report({
      entities: [{ key: "solo", name: "Solo", type: "Topic", fileIds: ["f1"], fileNames: ["notes.md"] }],
      files: [
        { fileId: "f1", driveId: "d", path: "a/none.md", name: "none.md", reason: { kind: "no_entities" } },
        { fileId: "f2", driveId: "d", path: "a/x.psd", name: "x.psd", reason: { kind: "unsupported", format: "psd" } },
        { fileId: "f3", driveId: "d", path: "a/p.pdf", name: "p.pdf", reason: { kind: "processing" } },
        { fileId: "f4", driveId: "d", path: "a/r.docx", name: "r.docx", reason: { kind: "review" } },
        { fileId: "f5", driveId: "d", path: "a/bad.xlsx", name: "bad.xlsx", reason: { kind: "parse_failed", code: "PARSE_XLSX_CORRUPT" } },
        { fileId: "f7", driveId: "d", path: "a/call.m4a", name: "call.m4a", reason: { kind: "speaker", count: 2 } },
        { fileId: "f6", driveId: "d", path: "a/lone.md", name: "lone.md", reason: { kind: "no_relations", count: 6 } },
      ],
    })),
  });
  const onReview = vi.fn();
  it("names a grouped entry with its count and the reason, in plain words", () => {
    const grouped = applyUnlinked(base(), report({ files: Array.from({ length: 30 }, (_, index) => ({ fileId: `u${index}`, driveId: "d", path: `a/f${index}.ts`, name: `f${index}.ts`, reason: { kind: "unsupported" as const, format: "ts" } })) }));
    render(<TestLocalizationProvider messages={messages}><AtlasList data={grouped} query="" onOpen={() => undefined} only={UNLINKED_GROUP_ID} /></TestLocalizationProvider>);
    const rows = screen.getAllByTestId("atlas-list-member");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("30 files");
    expect(within(rows[0]!).getByTestId("atlas-reason")).toHaveTextContent("In formats the engine doesn’t read: ts ×30");
    expect(screen.getByTestId("atlas-list-group")).toHaveTextContent("30");
  });
  it("shows every reason in plain words next to each row, in a group alone when filtered", () => {
    render(<TestLocalizationProvider messages={messages}><AtlasList data={data()} query="" onOpen={() => undefined} only={UNLINKED_GROUP_ID} onReviewSpeakers={onReview} /></TestLocalizationProvider>);
    expect(screen.getAllByTestId("atlas-list-group")).toHaveLength(1);
    expect(screen.getByTestId("atlas-unlinked-caption")).toBeInTheDocument();
    const rows = screen.getAllByTestId("atlas-list-member");
    const reasons = Object.fromEntries(rows.map((row) => [row.getAttribute("data-name"), within(row).getByTestId("atlas-reason").textContent]));
    expect(reasons).toEqual({
      "Solo": "Only appears in notes.md",
      "none.md": "No entities found in this file",
      "x.psd": "Format not supported: psd",
      "p.pdf": "Still processing",
      "r.docx": "Waiting for your review",
      "bad.xlsx": "Could not be read (PARSE_XLSX_CORRUPT)",
      "lone.md": "Found 6 things, none appear elsewhere",
      "call.m4a": "Speakers not named yet (2)",
    });
    fireEvent.click(screen.getByTestId("atlas-speaker-review"));
    expect(onReview).toHaveBeenCalledTimes(1);
  });
  it("is a group of the full list otherwise, named in the language of the app by the screen", () => {
    render(<TestLocalizationProvider messages={messages}><AtlasList data={data()} query="" onOpen={() => undefined} /></TestLocalizationProvider>);
    const head = screen.getAllByTestId("atlas-list-group").find((h) => h.getAttribute("data-group") === UNLINKED_GROUP_ID)!;
    expect(head).toHaveTextContent("8 items");
    expect(screen.getAllByTestId("atlas-reason").length).toBe(8);
    fireEvent.click(head);
    expect(screen.queryAllByTestId("atlas-reason").length).toBe(0);
  });
});

describe("reading what is not connected while a big folder is ingested", () => {
  afterEach(() => { vi.useRealTimers(); resetUnlinked(); });
  const slowSource = (log: string[], gate: { release: () => void }) => {
    const base = source([f("a")], { a: { entities: [], facts: [] } });
    return { ...base, files: async (input: { driveId: string; cursor?: string }) => { log.push("files"); await new Promise<void>((resolve) => { gate.release = resolve; }); return base.files(input); } } as UnlinkedSource;
  };
  it("lets a reading for a newer key wait for the one on its way instead of running beside it", async () => {
    const log: string[] = [];
    const gate = { release: () => undefined };
    const slow = slowSource(log, gate);
    const first = readUnlinked("v1|10", slow);
    const second = readUnlinked("v2|10", slow);
    await Promise.resolve();
    expect(log).toEqual(["files"]);
    gate.release();
    await first;
    await vi.waitFor(() => expect(log).toEqual(["files", "files"]));
    gate.release();
    await second;
  });

  it("shows a report that arrives after its key went stale, and reads the newer key only once the key has stood still", async () => {
    vi.useFakeTimers();
    const reads: string[] = [];
    const src: UnlinkedSource = { ...source([f("a")], { a: { entities: [], facts: [] } }), files: vi.fn(async () => { reads.push("read"); return { ok: true as const, value: { items: [f("a")] as never, next_cursor: null } }; }) };
    const { result, rerender } = renderHook(({ key }: { key: string }) => useUnlinked(key, { source: src }), { initialProps: { key: "v1|10" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(result.current.status).toBe("ready");
    expect(reads).toHaveLength(1);
    // The ontology moves on: nothing is read again until the key has stood still, however often it changes.
    for (const key of ["v2|10", "v3|10", "v4|10"]) { rerender({ key }); await act(async () => { await vi.advanceTimersByTimeAsync(UNLINKED_REFRESH_DEBOUNCE_MS - 1_000); }); }
    expect(reads).toHaveLength(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(reads).toHaveLength(2);
    expect(result.current.report).not.toBeNull();
  });
});
