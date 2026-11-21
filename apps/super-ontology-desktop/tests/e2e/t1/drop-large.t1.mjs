// A code project dropped on the window (instant feedback, no file cap, big folders linked or copied): real `Input.dispatchDragEvent` with real
// paths, a real engine, a window that is never on screen. A synthetic tree of 25,000 source files plus the litter of a code project (.git,
// node_modules, target, dist, build, .venv, __pycache__, *.pyc, a .gitignore and an .ooignore) goes through the link path end to end; a second,
// smaller big folder goes through the streamed copy.
//   SO_T1_OO_BIN=<oo with docs (and the drive max_files migration)> node tests/e2e/run-t1.mjs --only drop-large
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";
import { createSyntheticTree } from "../../support/synthetic-tree.mjs";

const FEEDBACK_LIMIT_MS = 300;
const OBSERVER = `(() => {
  const marks = {};
  window.__soMarks = marks;
  const mark = (key) => { if (marks[key] === undefined) marks[key] = performance.now(); };
  window.addEventListener("drop", () => mark("drop"), true);
  const look = () => {
    const toast = document.querySelector('[data-testid="drop-toast"]');
    if (toast) {
      mark("toast");
      if (toast.getAttribute("data-phase") === "preparing") mark("preparing");
      if (toast.querySelector('[data-testid="drop-toast-found"]')) mark("found");
      if (toast.getAttribute("data-phase") === "choice") mark("choice");
    }
    if (document.querySelector('[data-testid="upload-item"]')) mark("row");
  };
  new MutationObserver(look).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
})()`;
const formatted = (value) => value.toLocaleString("en-US");
const IGNORED_SEGMENT = /(^|\/)(node_modules|\.git|target|dist|build|\.venv|\.next|__pycache__|generated|secrets)(\/|$)|\.pyc$|\.log$/u;

runScenario({ name: "drop-large", title: "A code project dropped: instant feedback, linked without a cap, ignore rules, engine scale", timeoutMs: 900_000 }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("sources");
  await s.waitFor("screen-sources");

  const src = path.join(s.root, "dragged");
  const exists = (file) => fs.access(file).then(() => true, () => false);
  const drives = async () => (await s.engineApi("/v1/docs/drives")).body?.items ?? [];
  const driveNamed = async (name) => (await drives()).find((drive) => drive.name === name);
  const timings = {};

  const project = path.join(src, "proj");
  const made = await createSyntheticTree(project, { kept: 25_000, litter: 3_000 });
  s.note(`synthetic tree: ${made.kept} kept files, ${made.ignoredFiles} files in ignored places, ${JSON.stringify(made.skipped)} entries skipped by reason`);

  // 0. Not connected: the files the engine cannot read, or has not read yet, are one entry per reason with a count, not one row each. Checked first, on a small
  // code folder that settles quickly (reading every file of a busy 25,000 file engine to build that list takes minutes on a debug build).
  const codeFolder = path.join(src, "codeproj");
  const codeMade = await createSyntheticTree(codeFolder, { kept: 1_200, litter: 50 });
  const codeDrive = (await s.engineApi("/v1/docs/drives", { method: "POST", headers: { "idempotency-key": "t1-codeproj-0000000000" }, body: { name: "codeproj", backend: "fs", config: { root: codeFolder } } })).body;
  await s.engineApi(`/v1/docs/drives/${codeDrive.id}:sync`, { method: "POST", body: {}, headers: { "idempotency-key": "t1-codeproj-sync-000000" } });
  const settleLimit = Date.now() + 300_000;
  for (;;) {
    const detail = (await s.engineApi(`/v1/docs/drives/${codeDrive.id}`)).body;
    if ((detail?.files_total ?? 0) >= codeMade.kept && (detail?.freshness?.processing ?? 1) === 0) break;
    if (Date.now() > settleLimit) throw new Error(`the code folder did not settle: ${JSON.stringify(detail?.freshness)}`);
    await s.wait(1_000);
  }
  await s.click("nav-ontology");
  await s.waitFor("screen-ontology");
  await s.click("ontology-tab-atlas");
  await s.waitFor("atlas-unlinked-filter", {}, { timeoutMs: 120_000 });
  await s.click("atlas-unlinked-filter");
  await s.waitFor("atlas-unlinked-view", {}, { timeoutMs: 60_000 });
  await s.until("(document.querySelectorAll('[data-testid=\"atlas-list-group\"][aria-expanded=\"false\"]').forEach((group) => group.click()), !!document.querySelector('[data-reason=\"group\"]'))", "the grouped entries", { timeoutMs: 240_000, interval: 500 });
  const members = await s.evaluate("[...document.querySelectorAll('[data-testid=\"atlas-reason\"]')].map((e) => e.getAttribute('data-reason'))");
  const groupRows = members.filter((reason) => reason === "group").length;
  const rowCount = await s.count("atlas-list-member");
  s.check("files with one reason are grouped, not listed one by one", groupRows >= 1 && rowCount < 100, `${groupRows} grouped entries, ${rowCount} rows in all`);
  const groupText = await s.evaluate("[...document.querySelectorAll('[data-reason=\"group\"]')].map((e) => e.textContent).join(' | ')");
  s.check("the grouped entry for unsupported files says how many and which formats", /In formats the engine doesn’t read: /u.test(groupText), groupText);
  await s.shot("not-connected-grouped");
  await s.click("nav-sources");
  await s.waitFor("screen-sources");

  // 1. The drop: feedback within the frame, the files found counted live, a choice for the big folder.
  await s.evaluate(OBSERVER);
  const t0 = Date.now();
  await s.dropFiles("app-root", [project]);
  await s.until("!!document.querySelector('[data-testid=\"drop-toast\"]')", "the toast", { timeoutMs: 5_000, interval: 5 });
  const outside = Date.now() - t0;
  const marks = await s.evaluate("window.__soMarks");
  timings.toastInPageMs = Math.round(marks.toast - marks.drop);
  timings.toastOutsideMs = outside;
  s.check(`feedback appears within ${FEEDBACK_LIMIT_MS} ms of the drop event (in the page)`, timings.toastInPageMs < FEEDBACK_LIMIT_MS, `${timings.toastInPageMs} ms`);
  s.check(`feedback appears within ${FEEDBACK_LIMIT_MS} ms of the drop in the harness (dispatch to first sight, CDP round trips included)`, outside < FEEDBACK_LIMIT_MS, `${outside} ms`);
  s.check("the first thing shown says Preparing", marks.preparing !== undefined && Math.round(marks.preparing - marks.drop) < FEEDBACK_LIMIT_MS, `${Math.round((marks.preparing ?? NaN) - marks.drop)} ms`);

  await s.waitFor("drop-toast", { "data-phase": "choice" }, { timeoutMs: 120_000, interval: 20 });
  const choiceMarks = await s.evaluate("window.__soMarks");
  timings.foundInPageMs = choiceMarks.found === undefined ? null : Math.round(choiceMarks.found - choiceMarks.drop);
  timings.choiceInPageMs = Math.round(choiceMarks.choice - choiceMarks.drop);
  s.check("the files found so far were shown while the walk went on", choiceMarks.found !== undefined, `first count after ${timings.foundInPageMs} ms; choice after ${timings.choiceInPageMs} ms`);
  const title = await s.text("drop-toast-title");
  s.check("a big folder asks instead of copying silently, and says how big", title.includes(formatted(made.kept)) && /big folder/iu.test(title), title);
  s.check("no cap: every file the rules keep was found", Number(await s.attr("drop-toast", "data-found")) === made.kept, String(await s.attr("drop-toast", "data-found")));
  const skippedTotal = Object.values(made.skipped).reduce((sum, value) => sum + value, 0);
  s.check("the skipped count is shown", Number(await s.attr("drop-toast-skipped", "data-count")) === skippedTotal, `${await s.attr("drop-toast-skipped", "data-count")} of ${skippedTotal}`);
  s.check("linking is the default (primary) action and says why", (await s.attr("drop-link-folder", "data-variant")) === "primary" && /recommended/iu.test(await s.text("drop-link-folder")), await s.text("drop-link-folder"));
  s.check("copying is the secondary action", (await s.attr("drop-copy-folder", "data-variant")) === "secondary", await s.text("drop-copy-folder"));
  await s.waitFor("upload-skipped", {}, { timeoutMs: 10_000 });
  const reasons = await s.attrs('[data-testid="upload-skipped"] [data-reason]', "data-reason").catch(() => []);
  const reasonList = await s.evaluate("[...document.querySelectorAll('[data-testid=\"upload-skipped\"] [data-reason]')].map((e) => e.getAttribute('data-reason') + ':' + e.querySelector('.so-tray-count')?.textContent)");
  s.check("what was skipped is listed by reason, including .gitignore and .ooignore", ["hidden", "junk", "gitignore", "ooignore"].every((reason) => reasonList.some((entry) => entry.startsWith(`${reason}:`))), `${reasonList.join(", ")} (${reasons.length} reasons)`);
  s.check("nothing was copied while the choice is open", !(await exists(path.join((await driveNamed("My files"))?.config?.root ?? "/nonexistent", "proj"))));
  await s.shot("big-folder-choice");

  // 2. Link: the folder becomes a source in place; every file the rules keep reaches the engine; nothing the rules skip does.
  const linkedAt = Date.now();
  await s.click("drop-link-folder");
  await s.waitFor("drop-toast", { "data-choice": "linked" }, { timeoutMs: 60_000 });
  s.check("the toast says the folder was linked and nothing was copied", (await s.text("drop-toast-title")).includes(`Linked “proj”: ${formatted(made.kept)} files`), await s.text("drop-toast-title"));
  await s.waitFor("source-row", { "data-name": "proj" }, { timeoutMs: 60_000 });
  const linked = await driveNamed("proj");
  s.check("the folder is a source that points at the original folder", linked?.backend === "fs" && (await fs.realpath(linked.config.root)) === (await fs.realpath(project)), JSON.stringify(linked?.config));
  const myFilesRoot = (await driveNamed("My files"))?.config?.root;
  s.check("no copy of it was made", !myFilesRoot || !(await exists(path.join(myFilesRoot, "proj"))));
  s.check("the files are not copied twice: the dropped copy was never started", Number(await s.attr("drop-toast", "data-queued")) === 0);

  // The engine lists every file the rules keep (drive sync of a 25,000 file folder), keeps answering list pages quickly while it works through them
  // (a debug build takes minutes to parse 25,000 files; what matters is that it never stalls), and reports its freshness honestly while it does.
  const listLatencies = [];
  let firstFileAt = null;
  const listedAt = await (async () => {
    const limit = Date.now() + 180_000;
    while (Date.now() < limit) {
      const started = Date.now();
      const first = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=200`)).body;
      listLatencies.push(Date.now() - started);
      if (firstFileAt === null && (first?.items ?? []).length > 0) firstFileAt = Date.now() - linkedAt;
      const detail = (await s.engineApi(`/v1/docs/drives/${linked.id}`)).body;
      if ((detail?.files_total ?? 0) >= made.kept) return Date.now() - linkedAt;
      await s.wait(250);
    }
    return null;
  })();
  timings.firstEngineFileAfterLinkMs = firstFileAt;
  timings.allFilesListedAfterLinkMs = listedAt;
  s.check("the engine's drive sync lists every file the rules keep", listedAt !== null, `first file after ${firstFileAt} ms, all listed after ${listedAt} ms`);
  const progress = [];
  for (let sample = 0; sample < 4; sample += 1) {
    const started = Date.now();
    const page = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=200`)).body;
    listLatencies.push(Date.now() - started);
    const detail = (await s.engineApi(`/v1/docs/drives/${linked.id}`)).body;
    progress.push({ at: Date.now(), processing: detail.freshness?.processing ?? 0, state: detail.freshness?.state, listed: (page?.items ?? []).length });
    if (sample < 3) await s.wait(15_000);
  }
  const done = progress.map((entry) => made.kept - entry.processing);
  timings.processedFiles = done;
  timings.processedPerSecond = Math.round(((done.at(-1) - done[0]) / ((progress.at(-1).at - progress[0].at) / 1000)) * 10) / 10;
  timings.freshnessStates = [...new Set(progress.map((entry) => entry.state))];
  timings.fileListPageMs = { max: Math.max(...listLatencies), median: listLatencies.toSorted((a, b) => a - b)[Math.floor(listLatencies.length / 2)], polls: listLatencies.length };
  s.check("the engine never stalls: every 15 s more files are done than before", done.every((value, index) => index === 0 || value > done[index - 1]) || progress.at(-1).processing === 0, `${done.join(" -> ")} done of ${made.kept} (${timings.processedPerSecond} files/s)`);
  s.check("the freshness state tells the truth while files are processed (processing, publishing or up to date)", progress.every((entry) => ["processing", "publishing", "syncing", "up_to_date"].includes(entry.state)), JSON.stringify(timings.freshnessStates));
  s.check("a file list page stays quick while the engine ingests (under 2 s every time)", timings.fileListPageMs.max < 2_000, JSON.stringify(timings.fileListPageMs));

  // Everything the engine lists, page by page: exactly what the rules keep.
  const listed = [];
  let cursor = null;
  const pageStarted = Date.now();
  for (;;) {
    const page = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=500&recursive=true${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`)).body;
    for (const file of page.items) listed.push(file);
    cursor = page.next_cursor;
    if (!cursor) break;
  }
  timings.fullListingMs = Date.now() - pageStarted;
  s.check("the engine's listing holds exactly the files the rules keep", listed.length === made.kept, `${listed.length} listed, ${made.kept} expected, listed in ${timings.fullListingMs} ms`);
  s.check("none of node_modules, .git, target, dist, build, .venv, __pycache__, *.pyc, .gitignore'd or .ooignore'd files is listed", !listed.some((file) => IGNORED_SEGMENT.test(file.path)), listed.find((file) => IGNORED_SEGMENT.test(file.path))?.path ?? "");
  const states = new Map();
  for (const file of listed) states.set(file.state, (states.get(file.state) ?? 0) + 1);
  s.note(`file states: ${JSON.stringify(Object.fromEntries(states))}`);
  await s.shot("linked-source");

  // 3. The watcher: a new file in the linked folder reaches the engine by itself; one a .gitignore or .ooignore leaves out never does.
  const watchStarted = Date.now();
  await fs.writeFile(path.join(project, "generated", "later.ts"), "// ignored by .gitignore\n");
  await fs.writeFile(path.join(project, "secrets", "later.txt"), "ignored by .ooignore\n");
  await fs.writeFile(path.join(project, "node_modules", "later.js"), "x");
  await fs.writeFile(path.join(project, "src", "added-later.md"), "# Added later\n\nWritten after the folder was linked.\n");
  const found = await (async () => {
    const limit = Date.now() + 60_000;
    while (Date.now() < limit) {
      const page = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=50&recursive=true&path=src`)).body;
      if ((page?.items ?? []).some((file) => file.path === "src/added-later.md")) return Date.now() - watchStarted;
      await s.wait(250);
    }
    return null;
  })();
  timings.watcherPickedUpNewFileMs = found;
  s.check("the watcher sends a file written into the linked folder to the engine within a minute", found !== null, `${found} ms`);
  await s.wait(3_000);
  const again = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=500&recursive=true&path=generated`)).body;
  const secretPage = (await s.engineApi(`/v1/docs/drives/${linked.id}/files?limit=500&recursive=true&path=secrets`)).body;
  s.check("files the .gitignore or .ooignore leaves out are not listed after a change either", (again?.items ?? []).length === 0 && (secretPage?.items ?? []).length === 0);

  // 4. A big folder copied as a stream: rows as they are released, exact counts, the engine lists everything.
  await s.click("nav-sources");
  await s.waitFor("screen-sources");
  const copyTree = path.join(src, "copyproj");
  const copyMade = await createSyntheticTree(copyTree, { kept: 2_600, litter: 200 });
  await s.evaluate(OBSERVER);
  const copyStarted = Date.now();
  await s.dropFiles("app-root", [copyTree]);
  await s.waitFor("drop-toast", { "data-phase": "choice" }, { timeoutMs: 60_000, interval: 20 });
  await s.click("drop-copy-folder");
  await s.waitFor("upload-item", {}, { timeoutMs: 30_000, interval: 20 });
  const copyMarks = await s.evaluate("window.__soMarks");
  timings.copyFirstRowInPageMs = Math.round(copyMarks.row - copyMarks.drop);
  await s.until(`document.querySelector('[data-testid="drop-toast"]')?.getAttribute('data-phase') === 'done' || document.querySelector('[data-testid="upload-tray"]')?.getAttribute('data-done') === '${copyMade.kept}'`, "the streamed copy finishing", { timeoutMs: 240_000, interval: 250 });
  const myFiles = await driveNamed("My files");
  const copied = await (async () => {
    const limit = Date.now() + 240_000;
    while (Date.now() < limit) {
      const detail = (await s.engineApi(`/v1/docs/drives/${myFiles.id}`)).body;
      if ((detail?.files_total ?? 0) >= copyMade.kept) return Date.now() - copyStarted;
      await s.wait(500);
    }
    return null;
  })();
  timings.copyAllListedMs = copied;
  s.check("the streamed copy lands every kept file in My files and the engine lists them", copied !== null, `${copied} ms`);
  s.check("the copy keeps the structure and leaves the litter behind", (await exists(path.join(myFiles.config.root, "copyproj", "src"))) && !(await exists(path.join(myFiles.config.root, "copyproj", "node_modules"))) && !(await exists(path.join(myFiles.config.root, "copyproj", "target"))));
  await s.shot("big-folder-copied");

  await fs.writeFile(path.join(s.reportDirectory, "drop-large-timings.json"), JSON.stringify(timings, null, 2));
  s.note(`timings: ${JSON.stringify(timings)}`);
  s.check("no console errors", s.consoleErrors().length === 0, JSON.stringify(s.consoleErrors().slice(0, 3)));
});
