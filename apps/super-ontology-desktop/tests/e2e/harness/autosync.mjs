// The cases of P07 (auto-sync): changes made outside the app, and files dropped into it, must reach the ontology without waiting for the engine's
// 30 s poll. Every case measures with measureChange, writes what it saw to `timings.json` in the evidence folder and then judges it: a failed
// budget fails the run (nothing is relaxed), after the numbers are on disk.
import { execFile } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import { BUDGET, judge, measureChange } from "./autosync-measure.mjs";

const run = promisify(execFile);
const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

const UI_SCRIPT = `(() => {
  const rows = [...document.querySelectorAll('[data-testid="file-row"]')].map((e) => [e.getAttribute('data-name'), e.getAttribute('data-state')]);
  const detail = document.querySelector('[data-testid="screen-source-detail"]');
  const fresh = document.querySelector('[data-testid="source-freshness"]');
  const atlas = document.querySelector('[data-testid="atlas"]');
  const version = document.querySelector('[data-testid="atlas-version"]');
  return { source: detail ? detail.getAttribute('data-name') : null, rows, fresh: fresh ? fresh.getAttribute('data-freshness') : null,
    rings: atlas ? Number(atlas.getAttribute('data-rings')) : null, version: version ? version.textContent : null,
    count: document.querySelectorAll('[data-testid="file-row"]').length };
})()`;

export function createAutosync({ engineApi, cdp, directory, locale, variables, state, osa, peekaboo, pid: pidOf }) {
  const timings = { locale, steps: {}, problems: [] };
  const write = () => fs.writeFile(path.join(directory, "timings.json"), JSON.stringify(timings, null, 2));
  const root = () => variables.watchedDir;

  async function listFiles(driveId) {
    const files = new Map();
    let cursor = "";
    for (let page = 0; page < 20; page += 1) {
      const { status, body } = await engineApi(`/v1/docs/drives/${driveId}/files?limit=200${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`);
      if (status !== 200) break;
      for (const file of body.items ?? []) files.set(file.path, { state: file.state, b3: file.b3, revision: file.revision, latency: file.latency_ms });
      if (!body.next_cursor) break;
      cursor = body.next_cursor;
    }
    return files;
  }

  function probeFor(driveId, sourceName) {
    return async () => {
      const [files, drive, page] = await Promise.all([
        listFiles(driveId),
        engineApi(`/v1/docs/drives/${driveId}`).then((r) => r.body),
        cdp().evaluate(UI_SCRIPT).catch(() => null),
      ]);
      const onSource = page && sourceName && page.source === sourceName;
      return { files, freshness: drive?.freshness?.state ?? null, lastSync: drive?.last_sync ?? null, uiFreshness: onSource ? page.fresh : null, uiRows: onSource ? new Map(page.rows) : null, rings: page?.rings ?? null, version: page?.version ?? null, uiCount: page?.count ?? null };
    };
  }

  async function findDrive(name) {
    for (let attempt = 0; attempt < 120; attempt += 1) {
      const { body } = await engineApi("/v1/docs/drives?limit=200").catch(() => ({ body: null }));
      const found = (body?.items ?? []).find((drive) => drive.name === name);
      if (found) return found;
      await sleep(500);
    }
    throw new Error(`no source named ${name}`);
  }

  async function settleAll(driveId, minFiles) {
    let last = "";
    for (let attempt = 0; attempt < 240; attempt += 1) {
      const files = await listFiles(driveId);
      const drive = (await engineApi(`/v1/docs/drives/${driveId}`)).body;
      const settled = new Set(["parsed", "published", "failed", "skipped", "unsupported"]);
      if (files.size >= minFiles && [...files.values()].every((file) => settled.has(file.state)) && drive?.freshness?.state === "up_to_date") return files;
      const counts = {};
      for (const file of files.values()) counts[file.state] = (counts[file.state] ?? 0) + 1;
      last = `${files.size} files ${JSON.stringify(counts)}, freshness ${drive?.freshness?.state} ${drive?.freshness?.reason_code ?? ""}`;
      await sleep(500);
    }
    throw new Error(`the source did not settle: ${last}`);
  }

  async function measure(label, { driveId, sourceName, expect, uiRequired = false, ...options }) {
    const probe = probeFor(driveId, sourceName);
    const measurement = await measureChange({ expect, probe, sleep, uiRequired, ...options.measure });
    const problems = judge(measurement, { uiRequired, ...options.judge });
    timings.steps[label] = { ...measurement, problems, ...(options.extra ?? {}) };
    for (const problem of problems) timings.problems.push(`${label}: ${problem}`);
    await write();
    return measurement;
  }

  const entitiesFor = async (query) => ((await engineApi(`/v1/docs/entities?limit=50&q=${encodeURIComponent(query)}`)).body?.items ?? []);
  const factsOf = async (key) => JSON.stringify((await engineApi(`/v1/docs/entities/${key}`)).body ?? {});
  const finish = async () => { await write(); if (timings.problems.length > 0) throw new Error(`auto-sync budgets missed:\n  ${timings.problems.join("\n  ")}`); };
  const fileText = (name, role) => `---\ntype: person\nname: ${name}\nrole: ${role}\n---\n# ${name}\n\nRole: ${role}\n`;

  const cases = {
    // The folder was registered through the app; wait for its twenty-two notes to settle and remember the drives.
    async baseline() {
      const drive = await findDrive(path.basename(root()));
      state.watched = drive.id;
      state.watchedName = drive.name;
      await settleAll(drive.id, 22);
      timings.baseline = { drive: drive.id, root: drive.config?.root };
      await write();
    },

    // 3 files chosen in the app's own upload (the native open panel) into the open My files screen; timed from the moment the panel closed.
    async drop() {
      const mine = await findDrive("My files");
      state.myFiles = mine.id;
      const before = await listFiles(mine.id);
      const sinceMs = Date.now() - (state.pickedAt ?? Date.now());
      await measure("drop-3-files", {
        driveId: mine.id, sourceName: "My files", uiRequired: true,
        expect: [{ path: "note-one.md", want: "published" }, { path: "note-two.txt", want: "published" }],
        measure: { offsetMs: sinceMs }, judge: { notPoll: false },
        extra: { measuredFromPanelCloseMs: sinceMs, rowsBefore: before.size, docx: "recorded, not asserted: the Docker-free runtime ships no parser sidecar (known item)" },
      });
      timings.steps["drop-3-files"].docxState = (await listFiles(mine.id)).get("note-three.docx")?.state ?? null;
      timings.steps["drop-3-files"].diagnostics = { managedRoot: mine.config?.root, managedFiles: await fs.readdir(mine.config?.root ?? "/nonexistent").catch((error) => String(error.code)), where: (await run("/usr/bin/find", [path.dirname(mine.config?.root ?? "/tmp"), "-name", "note-*", "-not", "-path", "*/Drop three/*"]).catch(() => ({ stdout: "" }))).stdout.trim().split("\n").slice(0, 10), page: String(await cdp().evaluate("document.body.innerText").catch(() => "")).slice(0, 600) };
      await finish();
    },

    // A file created, edited, renamed and deleted by plain file operations next to the app; one more copied by Finder itself.
    async externalOps() {
      const driveId = state.watched;
      const name = path.basename(root());
      const base = { driveId, sourceName: name };
      const a = path.join(root(), "zed-quill.md");
      await fs.writeFile(a, fileText("Zed Quill", "Tester"), "utf8");
      await measure("create", { ...base, uiRequired: true, expect: [{ path: "zed-quill.md", want: "published" }] });
      const created = (await listFiles(driveId)).get("zed-quill.md");
      const before = await entitiesFor("Zed Quill");
      state.zedPersonKey = before.find((entity) => entity.type === "Person")?.key;
      if (!state.zedPersonKey) timings.problems.push("create: the person Zed Quill did not become an entity");
      const versionsOf = (body) => new Set([...body.matchAll(/"version_id":"([^"]+)"/gu)].map((match) => match[1]));
      state.zedVersions = versionsOf(await factsOf(state.zedPersonKey));

      await fs.writeFile(a, fileText("Zed Quill", "Lead tester"), "utf8");
      await measure("edit", { ...base, expect: [{ path: "zed-quill.md", want: "published", changedFrom: created.b3 }], judge: { expectTransition: true } });
      const nowKey = (await entitiesFor("Zed Quill")).find((entity) => entity.type === "Person")?.key;
      // The rule extractor keeps no role text as a fact (it links the person to the file), so the edit shows as the entity's evidence
      // pointing at the file's new version and no longer at the old one.
      const carried = nowKey ? await factsOf(nowKey) : "";
      const after = versionsOf(carried);
      timings.editEntity = { keyBefore: state.zedPersonKey, keyAfter: nowKey, versionsBefore: [...state.zedVersions], versionsAfter: [...after] };
      if (after.size === 0 || [...after].some((version) => state.zedVersions.has(version))) timings.problems.push("edit: the entity's evidence still points at the old version of the file");

      const b = path.join(root(), "zed-quill-renamed.md");
      await fs.rename(a, b);
      await measure("rename", { ...base, uiRequired: true, // A renamed file ends as `duplicate`: the engine gives the new path a version that points at the old finished one, so the evidence moves across.
        expect: [{ path: "zed-quill-renamed.md", want: "duplicate" }, { path: "zed-quill.md", want: "gone" }] });
      const people = (await entitiesFor("Zed Quill")).filter((entity) => entity.type === "Person" && (entity.counts?.facts ?? 0) > 0);
      if (people.length !== 1) timings.problems.push(`rename: ${people.length} Zed Quill entities carry facts after a rename (want exactly 1)`);

      await fs.rm(b);
      await measure("delete", { ...base, uiRequired: true, expect: [{ path: "zed-quill-renamed.md", want: "gone" }] });
      const left = (await entitiesFor("Zed Quill")).filter((entity) => (entity.counts?.facts ?? 0) > 0);
      if (left.length > 0) timings.problems.push(`delete: ${left.length} entities still carry facts of the deleted file (${left.map((entity) => entity.name).join(", ")})`);

      timings.steps["finder-copy"] = { notRun: "needs the Automation permission for Finder, which the owner has not granted; every outside change here is a plain file operation" };
      await finish();
    },

    // 50 files copied in one go.
    async bulk() {
      const driveId = state.watched;
      const started = Date.now();
      await run("/bin/cp", ["-R", `${variables.bulk50}/.`, `${root()}/`]);
      timings.bulkCopyMs = Date.now() - started;
      const expect = Array.from({ length: 50 }, (_, index) => ({ path: `bulk-${String(index + 1).padStart(2, "0")}.md`, want: "published" }));
      const measurement = await measure("bulk-50", { driveId, sourceName: path.basename(root()), expect, uiRequired: false, measure: { timeoutMs: 120_000 }, judge: { rowMs: BUDGET.bulkRowMs, settleMs: BUDGET.bulkPublishMs } });
      state.beforeQuit = await listFiles(driveId); // the app is quit next: nothing can be asked of the engine then
      timings.steps["bulk-50"].summary = { lastRowMs: Math.max(...measurement.files.map((item) => item.rowMs ?? Infinity)), lastPublishedMs: Math.max(...measurement.files.map((item) => item.settledMs ?? Infinity)) };
      await finish();
    },

    // While the app is quit: one note edited, one created, one deleted.
    async offlineChange() {
      state.offline = { edited: "base-03.md", editedFrom: state.beforeQuit.get("base-03.md")?.b3, created: "offline-new.md", deleted: "base-04.md" };
      await fs.appendFile(path.join(root(), state.offline.edited), "\nEdited while the app was closed.\n");
      await fs.writeFile(path.join(root(), state.offline.created), fileText("Ola Brandt", "Archivist"), "utf8");
      await fs.rm(path.join(root(), state.offline.deleted));
      state.offline.at = Date.now();
    },

    // After the relaunch: from the moment the engine answers, how long until the offline changes are in.
    async offlineCheck() {
      const o = state.offline;
      for (let attempt = 0; attempt < 240; attempt += 1) { try { if ((await engineApi("/v1/docs/health")).status === 200) break; } catch { /* not yet */ } await sleep(500); }
      await measure("offline-catch-up", {
        driveId: state.watched, sourceName: null,
        expect: [{ path: o.edited, want: "published", changedFrom: o.editedFrom }, { path: o.created, want: "published" }, { path: o.deleted, want: "gone" }],
        measure: { timeoutMs: 120_000 }, extra: { offlineForMs: Date.now() - o.at },
      });
      await finish();
    },

    // The Atlas is open in its by-source view; edit a file that holds an entity the Atlas shows. Seen: the change ring on the page, the
    // entity's evidence moving to the file's new version, and the page's version tick agreeing with the engine's ontology version.
    async ring() {
      const driveId = state.watched;
      const people = (await entitiesFor("Mira Okafor")).filter((entity) => entity.type === "Person");
      if (people.length === 0) throw new Error("Mira Okafor is not an entity yet");
      const key = people[0].key;
      const versionsOf = (body) => new Set([...body.matchAll(/"version_id":"([^"]+)"/gu)].map((match) => match[1]));
      const versionsBefore = versionsOf(await factsOf(key));
      const before = (await listFiles(driveId)).get("mira-okafor.md");
      const page = await cdp().evaluate(UI_SCRIPT);
      const driveBefore = (await engineApi(`/v1/docs/drives/${driveId}`)).body;
      const text = (await fs.readFile(path.join(root(), "mira-okafor.md"), "utf8")).replace("Platform lead", "Staff engineer");
      const marks = { ringMs: null, evidenceMs: null };
      const probe = probeFor(driveId, null);
      let t0 = 0;
      let checked = 0;
      const watching = async () => {
        const sample = await probe();
        const t = Date.now() - t0;
        if (sample.rings !== null && page.rings !== null && sample.rings > page.rings && marks.ringMs === null) marks.ringMs = t;
        if (marks.evidenceMs === null && Date.now() - checked > 300) {
          checked = Date.now();
          const now = versionsOf(await factsOf(key));
          if (now.size > 0 && ![...now].some((version) => versionsBefore.has(version))) marks.evidenceMs = t;
        }
        return sample;
      };
      await fs.writeFile(path.join(root(), "mira-okafor.md"), text, "utf8");
      t0 = Date.now();
      const measurement = await measureChange({ expect: [{ path: "mira-okafor.md", want: "published", changedFrom: before.b3 }], probe: watching, sleep, intervalMs: 100, timeoutMs: 90_000 });
      const problems = judge(measurement, {});
      for (const [what, ms] of Object.entries(marks)) {
        if (ms === null) problems.push(`${what}: never happened (the Atlas / entity did not show the change)`);
        else if (ms > BUDGET.publishMs + BUDGET.ringLagMs) problems.push(`${what}: ${ms} ms > ${BUDGET.publishMs + BUDGET.ringLagMs} ms`);
      }
      if (page.rings === null) problems.push("the Atlas was not open (no data-rings on the page)");
      const driveAfter = (await engineApi(`/v1/docs/drives/${driveId}`)).body;
      const pageAfter = await cdp().evaluate(UI_SCRIPT);
      const versionOf = (drive) => drive?.freshness?.ontology_version;
      if (pageAfter.version !== `v${versionOf(driveAfter)}`) problems.push(`the Atlas shows ${pageAfter.version} but the engine's ontology version is ${versionOf(driveAfter)}`);
      timings.steps.ring = { ...measurement, problems, marks, ringsBefore: page.rings, ringsAfter: pageAfter.rings, ontologyVersion: { before: versionOf(driveBefore), after: versionOf(driveAfter) }, atlasVersion: { before: page.version, after: pageAfter.version } };
      for (const problem of problems) timings.problems.push(`ring: ${problem}`);
      await finish();
    },
  };

  return Object.freeze({ cases, timings, write });
}
