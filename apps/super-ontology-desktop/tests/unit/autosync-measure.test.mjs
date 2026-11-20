import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { createAutosyncFixtures, BASE_FILE_COUNT } from "../e2e/harness/fixtures.mjs";
import { BUDGET, judge, measureChange, percentile, Trace } from "../e2e/harness/autosync-measure.mjs";

/** A scripted world: `script(t)` answers the probe at fake time t. */
function world(script, step = 100) {
  let t = 0;
  return { now: () => t, sleep: async (ms) => { t += ms; }, probe: async () => script(t), step, advance: (ms) => { t += ms; } };
}
const sample = (t, files, freshness, extra = {}) => ({ files: new Map(Object.entries(files)), freshness, uiFreshness: freshness, uiRows: null, lastSync: null, ...extra });

describe("Trace", () => {
  it("keeps only changes and knows when it left up_to_date", () => {
    const trace = new Trace();
    for (const [ms, state] of [[0, "up_to_date"], [100, "up_to_date"], [200, "processing"], [300, "processing"], [400, "up_to_date"], [500, null]]) trace.add(ms, state);
    expect(trace.states()).toEqual(["up_to_date", "processing", "up_to_date"]);
    expect(trace.left).toBe(true);
    expect(trace.last).toBe("up_to_date");
  });
});

describe("measureChange", () => {
  it("times the row, the published state and the return to up_to_date", async () => {
    const w = world((t) => sample(t, t >= 800 ? { "a.md": { state: "published" } } : t >= 400 ? { "a.md": { state: "parsing" } } : {}, t >= 300 && t < 1100 ? "processing" : "up_to_date"));
    const m = await measureChange({ expect: [{ path: "a.md", want: "published" }], probe: w.probe, now: w.now, sleep: w.sleep });
    const [file] = m.files;
    expect(file.rowMs).toBe(400);
    expect(file.settledMs).toBe(800);
    expect(m.freshness.api.map((entry) => entry.state)).toEqual(["up_to_date", "processing", "up_to_date"]);
    expect(judge(m, { notPoll: false })).toEqual([]);
  });

  it("an edit is seen only once the b3 differs", async () => {
    const w = world((t) => sample(t, { "a.md": { state: "published", b3: t >= 1000 ? "new" : "old" } }, t >= 500 && t < 1200 ? "processing" : "up_to_date"));
    const m = await measureChange({ expect: [{ path: "a.md", want: "published", changedFrom: "old" }], probe: w.probe, now: w.now, sleep: w.sleep });
    expect(m.files[0].rowMs).toBe(1000);
    expect(m.files[0].settledMs).toBe(1000);
  });

  it("a deleted file is settled when it is absent or marked deleted", async () => {
    const w = world((t) => sample(t, t < 600 ? { "a.md": { state: "published" } } : t < 900 ? { "a.md": { state: "deleted" } } : {}, "up_to_date"));
    const m = await measureChange({ expect: [{ path: "a.md", want: "gone" }], probe: w.probe, now: w.now, sleep: w.sleep });
    expect(m.files[0].settledMs).toBe(600);
  });

  it("times out when a file never arrives and the judge says so", async () => {
    const w = world((t) => sample(t, {}, "up_to_date"));
    const m = await measureChange({ expect: [{ path: "a.md", want: "published" }], probe: w.probe, now: w.now, sleep: w.sleep, timeoutMs: 2000 });
    expect(m.timedOut).toBe(true);
    expect(judge(m, { notPoll: false }).join("\n")).toMatch(/did not settle[\s\S]*never appeared/u);
  });

  it("the page is held to the same states: a page that never shows the row fails when required", async () => {
    const w = world((t) => sample(t, { "a.md": { state: "published" } }, "up_to_date", { uiRows: new Map() }));
    const m = await measureChange({ expect: [{ path: "a.md", want: "published" }], probe: w.probe, now: w.now, sleep: w.sleep, uiRequired: true, timeoutMs: 1500 });
    expect(judge(m, { uiRequired: true, notPoll: false, expectTransition: false }).join("\n")).toMatch(/never showed it published/u);
  });
});

describe("judge", () => {
  const base = { timedOut: false, files: [{ path: "a.md", want: "published", rowMs: 500, settledMs: 2000, uiSettledMs: null, finalState: "published" }], freshness: { api: [], ui: [], apiLeftUpToDate: true, apiEnd: "up_to_date", uiEnd: null }, syncs: [] };
  it("passes a change that the watcher took quickly", () => expect(judge(base, { uiRequired: false })).toEqual([]));
  it("fails over-budget rows and publishes without loosening", () => {
    const slow = { ...base, files: [{ ...base.files[0], rowMs: BUDGET.rowMs + 1, settledMs: BUDGET.publishMs + 1 }] };
    expect(judge(slow, {}).length).toBe(2);
  });
  it("fails a change that only the 30 s poll took", () => {
    const polled = { ...base, syncs: [{ generation: 4, trigger: "poll", added: 1, changed: 0, removed: 0, renamed: 0 }] };
    expect(judge(polled, {}).join("\n")).toMatch(/poll/u);
    const idle = { ...base, syncs: [{ generation: 4, trigger: "poll", added: 0, changed: 0, removed: 0, renamed: 0 }] };
    expect(judge(idle, {})).toEqual([]);
  });
  it("fails freshness that ends anywhere but up_to_date, or never moved", () => {
    expect(judge({ ...base, freshness: { ...base.freshness, apiEnd: "held" } }, {}).join("\n")).toMatch(/held/u);
    expect(judge({ ...base, freshness: { ...base.freshness, apiLeftUpToDate: false } }, {}).join("\n")).toMatch(/never left/u);
  });
  it("percentile", () => { expect(percentile([5, 1, 3, 4, 2], 95)).toBe(5); expect(percentile([], 95)).toBeNull(); });
});

describe("P07 fixtures", () => {
  it("builds synthetic folders and a readable docx", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "p07-"));
    const f = await createAutosyncFixtures(root);
    expect(fs.readdirSync(f.watched).length).toBe(BASE_FILE_COUNT + 2);
    expect(fs.readdirSync(f.bulk).length).toBe(50);
    expect(fs.readdirSync(f.drop).sort()).toEqual(["note-one.md", "note-three.docx", "note-two.txt"]);
    const docx = fs.readFileSync(path.join(f.drop, "note-three.docx"));
    expect(docx.subarray(0, 2).toString()).toBe("PK");
    fs.rmSync(root, { recursive: true });
  });
});

describe("P07 summary", () => {
  it("turns timings into rows and a table", async () => {
    const { summarize, table } = await import("../e2e/t2/summarize-p07.mjs");
    const rows = summarize({ en: { steps: { create: { files: [{ rowMs: 400, settledMs: 2100, uiSettledMs: 2300 }], freshMs: 2600, freshness: { api: [{ state: "up_to_date" }, { state: "processing" }, { state: "up_to_date" }], ui: [] }, syncs: [], problems: [] }, bad: { files: [{ rowMs: null, settledMs: null }], freshness: { api: [], ui: [] }, syncs: [{ trigger: "poll", added: 1 }], problems: ["x"] } } } });
    expect(rows[0]).toMatchObject({ step: "create", rowMs: 400, publishedMs: 2100, pageMs: 2300, upToDateMs: 2600, freshness: "up_to_date > processing > up_to_date", viaPoll: false, ok: true });
    const skipped = summarize({ en: { steps: { copy: { notRun: "needs permission" } } } });
    expect(table(skipped)).toMatch(/\| not run \|/u);
    expect(rows[1]).toMatchObject({ rowMs: null, viaPoll: true, ok: false });
    expect(table(rows)).toMatch(/\| en \| create \| 1 \| 400 \| 2100 \| 2300 \| 2600 \|/u);
  });
});
