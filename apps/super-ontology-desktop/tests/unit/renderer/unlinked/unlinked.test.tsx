// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { analyzeUnlinked, fileReason, formatOf, reasonForRow, type UnlinkedReport, type UnlinkedSource } from "../../../../src/renderer/model/unlinked";
import { applyUnlinked, UNLINKED_GROUP_ID, unlinkedCount } from "../../../../src/renderer/features/atlas/unlinked";
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
    for (const state of ["deleted", "superseded", "duplicate", "outdated"]) expect(fileReason(file({ state }), null)).toBeNull();
  });
  it("a ready file needs to have been read: no entities, waiting for review, or entities with no relations", () => {
    expect(fileReason(file({}), null)).toBeNull();
    expect(fileReason(file({ pending_review: 2 }), null)).toEqual({ kind: "review" });
    expect(fileReason(file({}), { entities: 0, connected: false })).toEqual({ kind: "no_entities" });
    expect(fileReason(file({ pending_review: 1 }), { entities: 0, connected: false })).toEqual({ kind: "review" });
    expect(fileReason(file({}), { entities: 3, connected: false })).toEqual({ kind: "no_relations" });
    expect(fileReason(file({}), { entities: 3, connected: true })).toBeNull();
  });
  it("reads the format from the extension, lower case", () => {
    expect(formatOf({ path: "a/B.HEIC" })).toBe("heic");
    expect(formatOf({ path: ".hidden" })).toBe("?");
  });
});

const ent = (key: string, type = "Person") => ({ key, name: key.toUpperCase(), type });
function source(files: Record<string, unknown>[], details: Record<string, { entities?: unknown[]; facts?: unknown[] }>, linked: string[] = []): UnlinkedSource {
  return {
    drives: async () => ({ ok: true, value: [{ id: "d1", name: "Docs" }] }),
    files: async () => ({ ok: true, value: { items: files as never, next_cursor: null } }),
    detail: async (id) => (details[id] ? { ok: true, value: { id, ...details[id] } as never } : { ok: false, error: { code: "X", retryable: false, details: {} } }),
    linkedFilePaths: async () => new Set(linked),
  };
}
const f = (id: string, over: Record<string, unknown> = {}) => ({ id, drive_id: "d1", path: `docs/${id}.md`, state: "published", pending_review: 0, ...over });

describe("reading the files", () => {
  it("finds files with nothing, files whose entities relate to nothing, isolated entities, and leaves connected ones out", async () => {
    const report = await analyzeUnlinked(source(
      [f("a"), f("b"), f("c"), f("d", { state: "unsupported", format: "psd" }), f("e", { state: "parsing" }), f("g")],
      {
        a: { entities: [ent("ann"), ent("proj", "Project")], facts: [{ subject: ent("ann"), object: ent("proj", "Project"), status: "published" }] },
        b: { entities: [ent("solo")], facts: [{ subject: ent("solo"), object: null, status: "published" }] },
        c: { entities: [], facts: [] },
        g: { entities: [ent("lonely")], facts: [] },
      },
    ));
    const byId = Object.fromEntries(report.files.map((x) => [x.fileId, x.reason.kind]));
    expect(byId).toEqual({ b: "no_relations", c: "no_entities", d: "unsupported", e: "processing", g: "no_relations" });
    expect(report.entities.map((x) => x.key)).toEqual(["lonely", "solo"].sort((x, y) => x.toUpperCase().localeCompare(y.toUpperCase())));
    expect(report.entities.find((x) => x.key === "solo")?.fileIds).toEqual(["b"]);
    expect(report.complete).toBe(true);
    expect(report.read).toContain("a");
    expect(reasonForRow({ ...f("a") } as never, report)).toBeNull();
    expect(reasonForRow({ ...f("c") } as never, report)).toEqual({ kind: "no_entities" });
  });

  it("the Document entity the engine names after every file does not count as an entity found", async () => {
    const doc = (name: string) => ({ key: name, name, type: "Document" });
    const report = await analyzeUnlinked(source([f("a"), f("b", { state: "unsupported", format: "unknown", path: "x/plan.dwg" })], { a: { entities: [doc("docs/a.md")], facts: [] } }));
    expect(report.files.map((x) => [x.fileId, x.reason])).toEqual([["a", { kind: "no_entities" }], ["b", { kind: "unsupported", format: "dwg" }]]);
    expect(report.entities).toEqual([]);
  });

  it("an entity another file's fact points to is connected; a file that links to a table row is connected", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b"), f("c")], {
      a: { entities: [ent("x")], facts: [{ subject: ent("x"), object: ent("y"), status: "published" }] },
      b: { entities: [ent("y")], facts: [] },
      c: { entities: [ent("z")], facts: [] },
    }, ["d1|docs/c.md"]));
    expect(report.files).toEqual([]);
    expect(report.entities).toEqual([]);
  });

  it("a retracted fact does not connect, and a file that failed to read is never called unconnected", async () => {
    const report = await analyzeUnlinked(source([f("a"), f("b")], { a: { entities: [ent("x")], facts: [{ subject: ent("x"), object: ent("y"), status: "retracted" }] } }));
    expect(report.files.map((x) => x.fileId)).toEqual(["a"]);
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
      entities: [{ key: "solo", name: "Solo", type: "Topic", fileIds: [] }],
      files: [
        { fileId: "f1", driveId: "d", path: "a/none.md", name: "none.md", reason: { kind: "no_entities" } },
        { fileId: "f2", driveId: "d", path: "a/x.psd", name: "x.psd", reason: { kind: "unsupported", format: "psd" } },
        { fileId: "f3", driveId: "d", path: "a/p.pdf", name: "p.pdf", reason: { kind: "processing" } },
        { fileId: "f4", driveId: "d", path: "a/r.docx", name: "r.docx", reason: { kind: "review" } },
        { fileId: "f5", driveId: "d", path: "a/bad.xlsx", name: "bad.xlsx", reason: { kind: "parse_failed", code: "PARSE_XLSX_CORRUPT" } },
        { fileId: "f6", driveId: "d", path: "a/lone.md", name: "lone.md", reason: { kind: "no_relations" } },
      ],
    })),
  });
  it("shows every reason in plain words next to each row, in a group alone when filtered", () => {
    render(<TestLocalizationProvider messages={messages}><AtlasList data={data()} query="" onOpen={() => undefined} only={UNLINKED_GROUP_ID} /></TestLocalizationProvider>);
    expect(screen.getAllByTestId("atlas-list-group")).toHaveLength(1);
    expect(screen.getByTestId("atlas-unlinked-caption")).toBeInTheDocument();
    const rows = screen.getAllByTestId("atlas-list-member");
    const reasons = Object.fromEntries(rows.map((row) => [row.getAttribute("data-name"), within(row).getByTestId("atlas-reason").textContent]));
    expect(reasons).toEqual({
      "Solo": "No relation to anything else",
      "none.md": "No entities found in this file",
      "x.psd": "Format not supported: psd",
      "p.pdf": "Still processing",
      "r.docx": "Waiting for your review",
      "bad.xlsx": "Could not be read (PARSE_XLSX_CORRUPT)",
      "lone.md": "Entities found, but none relates to anything else",
    });
  });
  it("is a group of the full list otherwise, named in the language of the app by the screen", () => {
    render(<TestLocalizationProvider messages={messages}><AtlasList data={data()} query="" onOpen={() => undefined} /></TestLocalizationProvider>);
    const head = screen.getAllByTestId("atlas-list-group").find((h) => h.getAttribute("data-group") === UNLINKED_GROUP_ID)!;
    expect(head).toHaveTextContent("7 items");
    expect(screen.getAllByTestId("atlas-reason").length).toBe(7);
    fireEvent.click(head);
    expect(screen.queryAllByTestId("atlas-reason").length).toBe(0);
  });
});
