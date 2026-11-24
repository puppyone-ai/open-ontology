import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MAX_SYNC_PATHS, WATCH_DEBOUNCE_MS, WATCH_MAX_WAIT_MS, createDriveWatcher, isIgnoredPath, toDrivePath } from "@so-main/drive-watcher.mjs";
import { createSuperOntologyService } from "@so-main/service.mjs";
import { SuperOntologyError } from "@so-shared/errors.mjs";

const DRIVE = "00000000-0000-4000-8000-000000000001";
const OTHER = "00000000-0000-4000-8000-000000000002";
const drive = (id, root, extra = {}) => ({ id, name: id, backend: "fs", config: { root }, status: "active", revision: 1, counts: {}, created_at: "2026-11-17T00:00:00Z", ...extra });

/** `fs.watch` stand-in: records the watchers and lets a test fire events or errors. */
function fakeWatch() {
  const handles = [];
  const watch = vi.fn((root, options, listener) => {
    const handle = { root, options, listener, closed: false, errorListener: null, on(event, callback) { if (event === "error") this.errorListener = callback; return this; }, close() { this.closed = true; } };
    handles.push(handle);
    return handle;
  });
  return { watch, handles };
}

/**
 * The real `fs.watch` and real timers, observed: how many OS watchers were opened, how many events reached the watcher, and the waits it
 * scheduled. What these tests assert is counts and scheduled delays, never elapsed time or CPU share, so a loaded machine cannot change the
 * verdict: the waits below are only ceilings that stop a hung run.
 */
function observedReal() {
  const seen = { handles: 0, events: [], timeouts: [], pending: new Set() };
  const watch = (root, options, listener) => {
    seen.handles += 1;
    return fs.watch(root, options, (type, filename) => { seen.events.push(filename === null ? null : String(filename)); listener(type, filename); });
  };
  const timers = {
    setTimeout: (callback, wait) => {
      seen.timeouts.push(wait);
      const timer = setTimeout(() => { seen.pending.delete(timer); callback(); }, wait);
      seen.pending.add(timer);
      return timer;
    },
    clearTimeout: (timer) => { seen.pending.delete(timer); clearTimeout(timer); },
    now: () => Date.now(),
  };
  return { seen, watch, timers };
}
const until = async (condition, ceilingMs = 20_000) => {
  for (let waited = 0; waited < ceilingMs && !condition(); waited += 25) await new Promise((resolve) => { setTimeout(resolve, 25); });
  return condition();
};

/**
 * Barrier on the real watcher: a hidden file (ignored by the watcher, so it causes no timer and no request) is written until its event reaches
 * the listener. Before that the OS stream may not be live yet, which on a loaded machine loses a change made right after `refresh`; after it,
 * every event for an earlier change has been delivered, because one stream delivers in order. Then the debounce timers that those earlier events
 * started are waited out. No elapsed time is asserted: the loop only ends when the event arrives.
 */
async function settled(real, root) {
  for (let probe = 0; probe < 400; probe += 1) {
    const name = `.barrier-${probe}`;
    fs.writeFileSync(path.join(root, name), "x");
    if (await until(() => real.seen.events.includes(name), 250)) break;
  }
  expect(await until(() => real.seen.pending.size === 0)).toBe(true);
  await new Promise((resolve) => { setTimeout(resolve, 0); });
}

describe("path rules", () => {
  it("skips what the engine skips: hidden, lock, partial, junk and dependency folders, at any depth", () => {
    for (const ignored of [".DS_Store", "a/.git/HEAD", "~$report.docx", ".~lock.sheet.ods#", "dl.crdownload", "x.part", "x.tmp", "Thumbs.db", "desktop.ini", "node_modules/a/b.js", "src/__pycache__/m.pyc", ".so-upload-1.part", "dir/.hidden/file.md"]) expect(isIgnoredPath(ignored), ignored).toBe(true);
    for (const kept of ["plan.md", "Projects/a/plan.md", "报告 (2).pdf", "a.b.c.txt", "node_modules_not/x.md"]) expect(isIgnoredPath(kept), kept).toBe(false);
  });

  it("skips the same code project folders as the engine: target, dist, build, venv as folders, compiled files, at any depth", () => {
    for (const ignored of ["target/debug/app", "web/dist/bundle.js", "build/out.txt", "venv/lib/site.py", "src/mod.pyc", "src/mod.PYO", ".next/cache/a", ".venv/x", ".idea/workspace.xml", ".vscode/settings.json", ".cache/a"]) expect(isIgnoredPath(ignored), ignored).toBe(true);
    // The last name is taken as a file (a new folder called dist is not known to be one): it is not ignored by name, and a file called build is kept.
    for (const kept of ["docs/build", "build.rs", "targets/a.md", "distance/a.md", "venv.md", "src/main.rs"]) expect(isIgnoredPath(kept), kept).toBe(false);
  });

  it("turns what fs.watch reports into a DrivePath, or nothing", () => {
    expect(toDrivePath("Projects/plan.md")).toBe("Projects/plan.md");
    expect(toDrivePath(["Projects", "plan.md"].join(path.sep))).toBe("Projects/plan.md");
    for (const bad of ["", null, undefined, "/abs.md", "a//b.md", "../x.md", "a/./b.md", ".hidden", "a\nb.md", "x".repeat(1025)]) expect(toDrivePath(bad), String(bad)).toBeNull();
  });
});

describe("the watcher with a fake file system and clock", () => {
  let timers;
  let client;
  let fake;
  let watcher;
  beforeEach(() => {
    vi.useFakeTimers();
    timers = { setTimeout, clearTimeout, now: () => Date.now() };
    client = { request: vi.fn(async () => ({ value: { status: "running" }, replayed: false })) };
    fake = fakeWatch();
    watcher = createDriveWatcher({ client, logger: { debug: vi.fn() }, watch: fake.watch, exists: () => true, readDrives: async () => [drive(DRIVE, "/Users/example/Docs")], timers });
  });
  afterEach(() => { watcher.dispose(); vi.useRealTimers(); });
  const start = async () => { await watcher.refresh(); return fake.handles[0]; };
  const syncs = () => client.request.mock.calls.map(([name, options]) => ({ name, id: options.params.id, body: options.body, operationId: options.operationId }));

  it("watches each active folder Drive recursively and ignores other backends, paused Drives and relative roots", async () => {
    watcher = createDriveWatcher({
      client, watch: fake.watch, exists: () => true, timers, logger: { debug: vi.fn() },
      readDrives: async () => [drive(DRIVE, "/a"), drive(OTHER, "/b", { status: "paused" }), drive("3", "/c", { backend: "s3" }), drive("4", "relative/root")],
    });
    await watcher.refresh();
    expect(fake.handles.map((handle) => handle.root)).toEqual(["/a"]);
    expect(fake.handles[0].options).toMatchObject({ recursive: true });
    expect(watcher.watching()).toEqual([DRIVE]);
  });

  it("merges changes for 300 ms and sends one targeted sync with the distinct paths", async () => {
    const handle = await start();
    handle.listener("rename", "a.md");
    handle.listener("change", "a.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS - 50);
    handle.listener("rename", `sub${path.sep}b.md`);
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS - 50);
    expect(client.request).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(60);
    expect(syncs()).toEqual([{ name: "driveSync", id: DRIVE, body: { paths: ["a.md", "sub/b.md"] }, operationId: expect.stringMatching(/^watch-[A-Za-z0-9_-]{8,}-1$/u) }]);
    expect(syncs()[0].operationId.length).toBeGreaterThanOrEqual(16);
  });

  it("a batch with a path that is gone from disk (delete, rename) is one full sync, not a targeted one", async () => {
    const present = new Set(["/Users/example/Docs/new-name.md"]);
    watcher.dispose();
    watcher = createDriveWatcher({ client, logger: { debug: vi.fn() }, watch: fake.watch, exists: (target) => present.has(target), readDrives: async () => [drive(DRIVE, "/Users/example/Docs")], timers });
    const handle = await start();
    handle.listener("rename", "old-name.md");
    handle.listener("rename", "new-name.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs().map((entry) => entry.body)).toEqual([{}]);
    // Only paths that exist stay targeted.
    handle.listener("change", "new-name.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs().map((entry) => entry.body)).toEqual([{}, { paths: ["new-name.md"] }]);
  });

  it("honours .gitignore and .ooignore of the Drive, and a changed rules file is one full sync", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "so-watch-rules-"));
    try {
      fs.mkdirSync(path.join(dir, "src", "gen"), { recursive: true });
      fs.writeFileSync(path.join(dir, ".gitignore"), "src/gen/\n*.log\n");
      fs.writeFileSync(path.join(dir, ".ooignore"), "secrets/\n");
      watcher.dispose();
      watcher = createDriveWatcher({ client, logger: { debug: vi.fn() }, watch: fake.watch, exists: () => true, readDrives: async () => [drive(DRIVE, dir)], timers });
      const handle = await start();
      for (const name of ["src/gen/api.ts", "run.log", "logs/a.log", "secrets/key.txt"]) handle.listener("change", name);
      await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
      expect(syncs()).toEqual([]);
      handle.listener("change", "src/main.ts");
      await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
      expect(syncs()).toMatchObject([{ body: { paths: ["src/main.ts"] } }]);
      client.request.mockClear();
      fs.writeFileSync(path.join(dir, ".ooignore"), "*.ts\n");
      handle.listener("change", ".ooignore");
      await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
      expect(syncs()).toMatchObject([{ body: {} }]);
      client.request.mockClear();
      handle.listener("change", "src/main.ts");
      await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
      expect(syncs()).toEqual([]);
    } finally { fs.rmSync(dir, { recursive: true, force: true }); }
  });

  it("sends nothing for hidden or junk files", async () => {
    const handle = await start();
    for (const name of [".DS_Store", "x.part", "~$a.docx", ".git/index.lock", "node_modules/x/y.js"]) handle.listener("change", name);
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS * 3);
    expect(client.request).not.toHaveBeenCalled();
  });

  it("does not wait forever while changes keep arriving", async () => {
    const handle = await start();
    for (let index = 0; index < 40; index += 1) {
      handle.listener("change", `copy-${index}.md`);
      await vi.advanceTimersByTimeAsync(100);
    }
    expect(client.request.mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(WATCH_MAX_WAIT_MS).toBeGreaterThan(WATCH_DEBOUNCE_MS);
  });

  it("falls back to a full sync when a change has no path (lost events) or too many paths pile up", async () => {
    const handle = await start();
    handle.listener("rename", null);
    handle.listener("change", "a.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs().map((entry) => entry.body)).toEqual([{}]);
    for (let index = 0; index <= MAX_SYNC_PATHS; index += 1) handle.listener("change", `f${index}.md`);
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs().map((entry) => entry.body)).toEqual([{}, {}]);
  });

  it("sends exactly the maximum as paths", async () => {
    const handle = await start();
    for (let index = 0; index < MAX_SYNC_PATHS; index += 1) handle.listener("change", `f${index}.md`);
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs()[0].body.paths).toHaveLength(MAX_SYNC_PATHS);
  });

  it("asks again shortly when a sync is already running, and gives up after a few tries", async () => {
    client.request.mockImplementation(async () => { throw new SuperOntologyError("DOCS_SOURCE_SYNC_RUNNING"); });
    const handle = await start();
    handle.listener("change", "a.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(client.request).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1_100);
    expect(client.request).toHaveBeenCalledTimes(2);
    expect(syncs()[1].body).toEqual({ paths: ["a.md"] });
    await vi.advanceTimersByTimeAsync(10_000);
    expect(client.request).toHaveBeenCalledTimes(6);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(client.request).toHaveBeenCalledTimes(6);
  });

  it("a failing request (engine restarting) is dropped quietly: the engine's poll finds the file", async () => {
    client.request.mockImplementation(async () => { throw new SuperOntologyError("SO_ENGINE_UNREACHABLE"); });
    const handle = await start();
    handle.listener("change", "a.md");
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(client.request).toHaveBeenCalledTimes(1);
  });

  it("restarts after a watcher error with one full sync, and releases handles when a Drive goes or moves", async () => {
    const handle = await start();
    handle.errorListener({ code: "EMFILE" });
    expect(handle.closed).toBe(true);
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS + 10);
    expect(syncs().map((entry) => entry.body)).toEqual([{}]);
    await vi.advanceTimersByTimeAsync(5_100);
    expect(fake.handles).toHaveLength(2);
    expect(fake.handles[1].closed).toBe(false);

    watcher.reconcile([drive(DRIVE, "/Users/example/Moved")]);
    expect(fake.handles[1].closed).toBe(true);
    expect(fake.handles[2].root).toBe("/Users/example/Moved");
    watcher.reconcile([]);
    expect(fake.handles[2].closed).toBe(true);
    expect(watcher.watching()).toEqual([]);
  });

  it("releases everything on dispose and ignores late events", async () => {
    const handle = await start();
    handle.listener("change", "a.md");
    watcher.dispose();
    expect(handle.closed).toBe(true);
    handle.listener("change", "b.md");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(client.request).not.toHaveBeenCalled();
    await watcher.refresh();
    expect(fake.handles).toHaveLength(1);
  });

  it("does not start a watcher it cannot create, and tries again on the next refresh", async () => {
    let fail = true;
    const watch = vi.fn((...args) => { if (fail) throw Object.assign(new Error("gone"), { code: "ENOENT" }); return fake.watch(...args); });
    watcher = createDriveWatcher({ client, watch, timers, logger: { debug: vi.fn() }, readDrives: async () => [drive(DRIVE, "/missing")] });
    await watcher.refresh();
    expect(watcher.watching()).toEqual([]);
    fail = false;
    await watcher.refresh();
    expect(watcher.watching()).toEqual([DRIVE]);
  });

  it("overlapping refreshes run one more pass instead of racing", async () => {
    let reads = 0;
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    watcher = createDriveWatcher({ client, watch: fake.watch, timers, logger: { debug: vi.fn() }, readDrives: async () => { reads += 1; if (reads === 1) await gate; return [drive(DRIVE, "/a")]; } });
    const first = watcher.refresh();
    const second = watcher.refresh();
    release();
    await Promise.all([first, second]);
    expect(reads).toBe(2);
    expect(fake.handles).toHaveLength(1);
  });

  it("reads the Drive list from the engine when no reader is given", async () => {
    client.request.mockImplementation(async (name) => ({ value: { items: [drive(DRIVE, "/x")], next_cursor: null, as_of_seq: 1 }, replayed: false, name }));
    watcher = createDriveWatcher({ client, watch: fake.watch, timers, logger: { debug: vi.fn() } });
    await watcher.refresh();
    expect(client.request).toHaveBeenCalledWith("drivesList", { query: { limit: 200, cursor: undefined } });
    expect(watcher.watching()).toEqual([DRIVE]);
  });
});

describe("the watcher on the real file system", () => {
  let root;
  let watcher;
  beforeEach(() => { root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "sd-watch-"))); });
  afterEach(() => { watcher?.dispose(); fs.rmSync(root, { recursive: true, force: true }); });

  it("sends a targeted sync for a file put into a sub-folder after one debounce, and none for a hidden one", async () => {
    const sent = [];
    const client = { request: vi.fn(async (_name, options) => { sent.push(options.body); return { value: {}, replayed: false }; }) };
    const real = observedReal();
    watcher = createDriveWatcher({ client, logger: { debug() {} }, watch: real.watch, timers: real.timers, readDrives: async () => [drive(DRIVE, root)] });
    await watcher.refresh();
    expect(watcher.watching()).toEqual([DRIVE]);
    await settled(real, root);
    fs.mkdirSync(path.join(root, "notes"));
    // The folder's own event has been seen and answered; only then is the change under test made.
    await settled(real, root);
    sent.length = 0;
    real.seen.timeouts.length = 0;
    fs.writeFileSync(path.join(root, ".hidden"), "x");
    fs.writeFileSync(path.join(root, "notes", "plan.md"), "# plan");
    expect(await until(() => sent.some((body) => body.paths?.includes("notes/plan.md")))).toBe(true);
    // "Within a second" is the delay the watcher schedules, not how fast this machine ran it.
    expect(real.seen.timeouts.length).toBeGreaterThan(0);
    expect(Math.max(...real.seen.timeouts)).toBeLessThanOrEqual(WATCH_DEBOUNCE_MS);
    expect(WATCH_DEBOUNCE_MS).toBeLessThan(1_000);
    expect(sent.flatMap((body) => body.paths ?? []).some((entry) => entry.includes(".hidden"))).toBe(false);
  });

  it("deleting and renaming a file on disk each send a full sync after one debounce", async () => {
    fs.writeFileSync(path.join(root, "a.md"), "a\n");
    fs.writeFileSync(path.join(root, "b.md"), "b\n");
    const sent = [];
    const client = { request: vi.fn(async (_name, options) => { sent.push(options.body); return { value: {}, replayed: false }; }) };
    const real = observedReal();
    watcher = createDriveWatcher({ client, logger: { debug() {} }, watch: real.watch, timers: real.timers, readDrives: async () => [drive(DRIVE, root)] });
    await watcher.refresh();
    await settled(real, root);
    real.seen.timeouts.length = 0;
    sent.length = 0;
    fs.rmSync(path.join(root, "a.md"));
    expect(await until(() => sent.length >= 1)).toBe(true);
    expect(sent[0]).toEqual({});
    fs.renameSync(path.join(root, "b.md"), path.join(root, "c.md"));
    expect(await until(() => sent.length >= 2)).toBe(true);
    expect(sent[1]).toEqual({});
    expect(Math.max(...real.seen.timeouts)).toBeLessThanOrEqual(WATCH_DEBOUNCE_MS);
  });

  it("a Drive of 10,000 files is one OS watcher and costs no wake-ups while nothing changes", async () => {
    for (let folder = 0; folder < 100; folder += 1) {
      fs.mkdirSync(path.join(root, `d${folder}`));
      for (let file = 0; file < 100; file += 1) fs.writeFileSync(path.join(root, `d${folder}`, `f${file}.md`), "x");
    }
    const sent = [];
    const client = { request: vi.fn(async (_name, options) => { sent.push(options.body); return { value: {}, replayed: false }; }) };
    const real = observedReal();
    watcher = createDriveWatcher({ client, logger: { debug() {} }, watch: real.watch, timers: real.timers, readDrives: async () => [drive(DRIVE, root)] });
    await watcher.refresh();
    // Cost does not grow with the tree: one recursive watcher, not one per file or folder.
    expect(real.seen.handles).toBe(1);
    // Drain what the tree's creation left in the stream, driven by events and never by elapsed time: one barrier round writes a hidden file (the
    // watcher ignores it: no timer, no request) and waits for its own event. Creating 10,000 files can leave a long backlog (the OS may deliver it
    // late on a loaded machine), so rounds repeat until QUIET_ROUNDS in a row delivered nothing but their own barrier.
    const QUIET_ROUNDS = 5;
    const barrier = (name) => name.startsWith(".barrier-");
    let round = 0;
    const roundOf = async () => {
      const before = real.seen.events.filter((name) => !barrier(name)).length;
      const name = `.barrier-drain-${round}`;
      round += 1;
      fs.writeFileSync(path.join(root, name), "x");
      expect(await until(() => real.seen.events.includes(name), 60_000)).toBe(true);
      return real.seen.events.filter((entry) => !barrier(entry)).length - before;
    };
    for (let quiet = 0, rounds = 0; quiet < QUIET_ROUNDS; rounds += 1) {
      expect(rounds).toBeLessThan(5_000);
      quiet = (await roundOf()) === 0 ? quiet + 1 : 0;
    }
    // The backlog is delivered; the debounce timers it started have run out and every request they made has been answered.
    expect(await until(() => real.seen.pending.size === 0, 60_000)).toBe(true);
    client.request.mockClear();
    real.seen.events.length = 0;
    real.seen.timeouts.length = 0;
    // Idle window, in events: the watcher is woken by nothing. The same barrier rounds now run with nothing else happening on disk; each event of
    // the stream is delivered in order, so when the last barrier is back, everything that could have woken the watcher before it has been
    // delivered. No event other than the barrier files, no timer and no request may appear in any of them.
    for (let idle = 0; idle < QUIET_ROUNDS; idle += 1) expect(await roundOf()).toBe(0);
    expect(real.seen.events.filter((name) => !barrier(name))).toEqual([]);
    expect(real.seen.timeouts).toEqual([]);
    expect(real.seen.pending.size).toBe(0);
    expect(client.request).not.toHaveBeenCalled();
    expect(real.seen.handles).toBe(1);
  }, 60_000);

  it("is released when the Drive is removed from the list", async () => {
    const client = { request: vi.fn(async () => ({ value: {}, replayed: false })) };
    let drives = [drive(DRIVE, root)];
    watcher = createDriveWatcher({ client, logger: { debug() {} }, readDrives: async () => drives });
    await watcher.refresh();
    drives = [];
    await watcher.refresh();
    expect(watcher.watching()).toEqual([]);
    fs.writeFileSync(path.join(root, "late.md"), "x");
    await new Promise((resolve) => { setTimeout(resolve, 700); });
    expect(client.request).not.toHaveBeenCalled();
  });
});

describe("the service starts and stops the watcher with the engine", () => {
  it("watches when the engine becomes healthy, releases when it is not usable, and on shutdown", async () => {
    const fake = fakeWatch();
    const readDrives = vi.fn(async () => [drive(DRIVE, "/Users/example/Docs")]);
    const service = createSuperOntologyService({ logger: { debug() {} }, driveWatcherOptions: { watch: fake.watch, readDrives } });
    const { snapshots } = service.internals;
    expect(fake.handles).toHaveLength(0);
    snapshots.update({ status: "starting" });
    expect(readDrives).not.toHaveBeenCalled();
    snapshots.update({ status: "healthy" });
    await vi.waitFor(() => expect(fake.handles).toHaveLength(1));
    snapshots.update({ health: { events_head: 1 } });
    snapshots.update({ health: { events_head: 2 } });
    expect(readDrives).toHaveBeenCalledTimes(1);
    snapshots.update({ status: "crashed" });
    expect(fake.handles[0].closed).toBe(true);
    snapshots.update({ status: "healthy" });
    await vi.waitFor(() => expect(fake.handles).toHaveLength(2));
    await service.shutdown();
    expect(fake.handles[1].closed).toBe(true);
  });
});
