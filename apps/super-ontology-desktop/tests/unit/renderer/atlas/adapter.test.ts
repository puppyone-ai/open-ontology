import { describe, expect, it } from "vitest";
import { diffAtlas, driveSource, loadAtlas, pool } from "../../../../src/renderer/features/atlas/adapter";
import { ENTITY_TYPE_VALUES, fakeAtlasSource, syntheticAtlas } from "../../../../src/renderer/features/atlas/lab/fixtures";
import { parseForeignKeys, parseTables } from "../../../../src/renderer/features/atlas/atlasApi";

const corpus = () => syntheticAtlas({ groups: 6, members: 80, linkRatio: 1, versions: 5, seed: 11, dbShare: 0.34, names: ENTITY_TYPE_VALUES });
const ids = (list: { id: string }[]) => list.map((x) => x.id).sort();

describe("atlas data adapter", () => {
  it("type mode groups by entity type only: no source, table or database appears", async () => {
    const data = corpus();
    const source = fakeAtlasSource(data);
    const { data: loaded, truncated } = await loadAtlas(source, { mode: "type" });
    expect(truncated).toBe(false);
    expect(loaded.groups.every((g) => g.kind === "type")).toBe(true);
    expect(ids(loaded.members)).toEqual(ids(data.members.filter((m) => m.ref.kind === "entity")));
    expect(loaded.links.every((l) => l.kind === "fact")).toBe(true);
    expect(loaded.links.length).toBe(data.links.filter((l) => l.kind === "fact").length);
    expect(source.calls.catalog).toBeUndefined();
    expect(loaded.versions.map((v) => v.n)).toEqual(data.versions.map((v) => v.n));
    expect(loaded.partial).toEqual([]);
    const entity = loaded.members[0]!;
    expect(entity.born).toBe(data.members.find((m) => m.id === entity.id)!.born);
  });

  it("source mode groups by source only (Drives and database schemas) and links files to tables and tables to tables", async () => {
    const data = corpus();
    const { data: loaded } = await loadAtlas(fakeAtlasSource(data), { mode: "source" });
    expect(loaded.groups.every((g) => g.kind === "drive" || g.kind === "db")).toBe(true);
    expect(loaded.groups.some((g) => g.kind === "drive")).toBe(true);
    expect(ids(loaded.groups.filter((g) => g.kind === "db"))).toEqual(ids(data.groups.filter((g) => g.kind === "db")));
    expect(ids(loaded.members.filter((m) => m.ref.kind === "table"))).toEqual(ids(data.members.filter((m) => m.ref.kind === "table")));
    expect(loaded.members.some((m) => m.ref.kind === "entity")).toBe(false);
    expect(loaded.links.filter((l) => l.kind === "foreign_key").length).toBe(data.links.filter((l) => l.kind === "foreign_key").length);
    expect(loaded.links.filter((l) => l.kind === "link").length).toBe(data.links.filter((l) => l.kind === "link").length);
  });

  it("pages the entity list until it ends and stops at the cap, saying so", async () => {
    const data = syntheticAtlas({ groups: 3, members: 450, linkRatio: 0, seed: 2, dbShare: 0, names: ENTITY_TYPE_VALUES });
    const source = fakeAtlasSource(data);
    const all = await loadAtlas(source, { mode: "type" });
    expect(all.data.members.length).toBe(data.members.length);
    expect(source.calls.entities).toBe(3);
    const capped = await loadAtlas(fakeAtlasSource(data), { mode: "type", maxEntities: 250 });
    expect(capped.truncated).toBe(true);
    expect(capped.data.members.length).toBe(250);
  });

  it("reads relations only for the most connected entities, a few at a time", async () => {
    const data = corpus();
    const source = fakeAtlasSource(data);
    let live = 0, peak = 0;
    const entity = source.entity;
    source.entity = async (key) => { live += 1; peak = Math.max(peak, live); await new Promise((r) => setTimeout(r, 1)); const result = await entity(key); live -= 1; return result; };
    await loadAtlas(source, { mode: "type", relationBudget: 10, concurrency: 3 });
    expect(source.calls.entity).toBeLessThanOrEqual(10);
    expect(peak).toBeLessThanOrEqual(3);
  });

  it("names the part that failed and keeps the rest", async () => {
    const data = corpus();
    const noDb = await loadAtlas(fakeAtlasSource(data, { failCatalog: true }), { mode: "source" });
    expect(noDb.data.partial).toContain("tables");
    expect(noDb.data.members.some((m) => m.ref.kind === "file")).toBe(true);
    expect(noDb.data.groups.some((g) => g.kind === "db")).toBe(false);
    const noLinks = await loadAtlas(fakeAtlasSource(data, { failLinks: true }), { mode: "source" });
    expect(noLinks.data.partial).toContain("links");
    expect(noLinks.data.links.some((l) => l.kind === "link")).toBe(false);
    expect(noLinks.data.links.some((l) => l.kind === "foreign_key")).toBe(true);
  });

  it("source mode groups files by Drive and skips deleted and superseded ones", async () => {
    const drive = { id: "00000000-0000-4000-8000-000000000001", name: "Docs", backend: "fs", config: {}, status: "active", revision: 1, counts: {}, created_at: "x" };
    const file = (n: number, state: string) => ({ id: `f${n}`, drive_id: drive.id, path: `a/b/doc-${n}.md`, state, state_since: "x", retryable: false, pending_review: 0 });
    const source = fakeAtlasSource(syntheticAtlas({ groups: 2, members: 5, dbShare: 0.5, seed: 1 }), { drives: [drive as never], files: [file(1, "published"), file(2, "deleted"), file(3, "superseded"), file(4, "queued")] as never });
    const { data } = await loadAtlas(source, { mode: "source" });
    const docs = data.members.filter((m) => m.ref.kind === "file");
    expect(docs.map((m) => m.label)).toEqual(["doc-1.md", "doc-4.md"]);
    expect(data.groups.find((g) => g.kind === "drive")?.label).toBe("Docs");
    expect(source.calls.entities).toBeUndefined();
  });

  it("tells the kinds of source apart: My files, a folder, a bucket, a database, each with its own tint", async () => {
    expect([["My files", "fs"], ["Notes", "fs"], ["Backups", "s3"], ["Lake", "r2"]].map(([name, backend]) => driveSource({ name: name!, backend: backend! }))).toEqual(["files", "folder", "bucket", "bucket"]);
    const { data } = await loadAtlas(fakeAtlasSource(corpus()), { mode: "source" });
    const byKind = new Map(data.groups.map((g) => [g.source, g.tone]));
    expect([...byKind.keys()].sort()).toEqual(["bucket", "database", "files", "folder"]);
    expect(new Set(byKind.values()).size).toBe(4);
  });

  it("a link to a table without a schema resolves by table name when it is unambiguous", async () => {
    const data = corpus();
    const source = fakeAtlasSource(data);
    const links = source.links;
    source.links = async () => { const r = await links(); if (!r.ok) return r; return { ok: true, value: { ...r.value, items: r.value.items.map((i) => ({ ...i, db_table: i.db_table.slice(i.db_table.indexOf(".") + 1) })) } }; };
    const { data: loaded } = await loadAtlas(source, { mode: "source" });
    expect(loaded.links.filter((l) => l.kind === "link").length).toBeGreaterThan(0);
  });

  it("runs a pool with bounded width and stops when aborted", async () => {
    let live = 0, peak = 0, done = 0;
    const signal = { aborted: false };
    await pool(Array.from({ length: 20 }, (_, i) => i), 4, async (i) => { live += 1; peak = Math.max(peak, live); await new Promise((r) => setTimeout(r, 1)); live -= 1; done += 1; if (i === 8) signal.aborted = true; }, signal);
    expect(peak).toBeLessThanOrEqual(4);
    expect(done).toBeLessThan(20);
  });

  it("turns the difference between two loads into budding, retracting and pulsing", () => {
    const before = corpus();
    const after = structuredClone(before);
    const gone = after.members.pop()!;
    const added = { ...before.members[0]!, id: "e:brand-new", label: "Brand new" };
    after.members.push(added);
    after.members[1]!.weight += 3;
    after.versions.push({ n: 99, asOf: "2026-12-01T00:00:00Z" });
    const ops = diffAtlas(before, after);
    expect(ops).toContainEqual({ type: "bud", member: added, group: undefined });
    expect(ops).toContainEqual({ type: "retract", id: gone.id });
    expect(ops).toContainEqual({ type: "pulse", id: after.members[1]!.id });
    expect(ops).toContainEqual({ type: "breathe" });
    expect(diffAtlas(before, before)).toEqual([]);
  });

  it("carries a new group with the first member that needs it", () => {
    const before = corpus();
    const after = structuredClone(before);
    after.groups.push({ id: "g:type:Fresh", kind: "type", label: "Fresh", tone: "dusk" });
    after.members.push({ id: "e:fresh-1", group: "g:type:Fresh", label: "Fresh one", ref: { kind: "entity", key: "fresh-1" }, weight: 1, born: null });
    const bud = diffAtlas(before, after).find((op) => op.type === "bud");
    expect(bud).toMatchObject({ type: "bud", group: { id: "g:type:Fresh" } });
  });

  it("reads database rows defensively", () => {
    expect(parseTables([["public", "orders"], ["public"], [1, 2], ["", "x"]])).toEqual([{ schema: "public", table: "orders" }]);
    expect(parseForeignKeys([["a", "b", "c", "d"], ["a", "b", "c"]])).toEqual([{ from: { schema: "a", table: "b" }, to: { schema: "c", table: "d" } }]);
  });
});
