// How long a drop takes to show anything, and to show a row: measured, not asserted (the asserting scenario is drop-large). It runs unchanged against
// older builds, which is how the "before" numbers of the instant-drop work were taken (SO_APP_ROOT points at the other tree).
//   SO_T1_OO_BIN=<oo with docs> node tests/e2e/run-t1.mjs --only drop-latency     (writes drop-latency.json next to the report)
// Three drops from a Finder-like drag, each timed in the page (drop event -> toast, "Preparing…", files found, first row) and from outside
// (drop dispatched -> first file the engine lists): a small folder (the first drop also creates My files), 5,000 files, 25,000 files.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";
import { createSyntheticTree } from "../../support/synthetic-tree.mjs";

const OBSERVER = `(() => {
  const marks = {};
  window.__soMarks = marks;
  const mark = (key) => { if (marks[key] === undefined) marks[key] = performance.now(); };
  window.addEventListener("drop", () => mark("drop"), true);
  const look = () => {
    const toast = document.querySelector('[data-testid="drop-toast"]');
    if (toast) {
      mark("toast");
      if (/prepar|vorbereit|准备/iu.test(toast.textContent ?? "") || toast.getAttribute("data-phase") === "preparing") mark("preparing");
      if (toast.querySelector('[data-testid="drop-toast-found"]')) mark("found");
      if (toast.getAttribute("data-phase") === "choice") mark("choice");
      if (toast.getAttribute("data-phase") === "failed" || /too many|limit|zu viele|太多/iu.test(toast.textContent ?? "")) mark("refused");
    }
    if (document.querySelector('[data-testid="upload-item"]')) mark("row");
  };
  new MutationObserver(look).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
})()`;

runScenario({ name: "drop-latency", title: "Time to first feedback and first row of a drop", timeoutMs: 900_000 }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("sources");
  await s.waitFor("screen-sources");
  const src = path.join(s.root, "dragged");
  const results = {};
  const drives = async () => (await s.engineApi("/v1/docs/drives")).body?.items ?? [];

  async function measure(label, folderName, tree, { choose = null, expectFiles = null, waitMs = 240_000 } = {}) {
    const top = path.join(src, folderName);
    await createSyntheticTree(top, tree);
    await s.evaluate(OBSERVER);
    const wanted = (name) => drives().then((list) => list.find((drive) => drive.name === name));
    const t0 = Date.now();
    await s.dropFiles("app-root", [top]);
    const afterDispatch = Date.now() - t0;
    const harness = {};
    // From outside: when does the toast show up, when does the first file row, when does the engine list the first file.
    const deadline = Date.now() + waitMs;
    let chose = false;
    let target = null;
    while (Date.now() < deadline) {
      const seen = await s.evaluate(`({ toast: !!document.querySelector('[data-testid="drop-toast"]'), row: !!document.querySelector('[data-testid="upload-item"]'), choice: document.querySelector('[data-testid="drop-toast"]')?.getAttribute('data-phase') === 'choice', failed: document.querySelector('[data-testid="drop-toast"]')?.getAttribute('data-phase') === 'failed' || !!document.querySelector('[data-testid="upload-tray"] [role="alert"]') })`);
      const elapsed = Date.now() - t0;
      if (seen.toast && harness.toast === undefined) harness.toast = elapsed;
      if (seen.row && harness.row === undefined) harness.row = elapsed;
      if (seen.failed && harness.refused === undefined) harness.refused = elapsed;
      if (seen.choice && !chose && choose) { chose = true; harness.choice = elapsed; await s.click(choose === "link" ? "drop-link-folder" : "drop-copy-folder"); }
      if (seen.choice && !choose && harness.choice === undefined) { harness.choice = elapsed; break; }
      if (harness.refused !== undefined) break;
      target ??= await wanted(choose === "link" ? folderName : "My files");
      if (target && harness.engineFirst === undefined) {
        const page = (await s.engineApi(`/v1/docs/drives/${target.id}/files?limit=1&recursive=true`)).body;
        if ((page?.items ?? []).length > 0) harness.engineFirst = Date.now() - t0;
      }
      if (target && expectFiles !== null) {
        const detail = (await s.engineApi(`/v1/docs/drives/${target.id}`)).body;
        if ((detail?.files_total ?? 0) >= expectFiles) { harness.engineAll = Date.now() - t0; break; }
      } else if (harness.engineFirst !== undefined) break;
      await s.wait(40);
    }
    const marks = await s.evaluate("window.__soMarks");
    const inPage = {};
    for (const key of ["toast", "preparing", "found", "row", "choice", "refused"]) if (marks[key] !== undefined && marks.drop !== undefined) inPage[key] = Math.round(marks[key] - marks.drop);
    results[label] = { files: tree.kept, dispatchMs: afterDispatch, inPageMs: inPage, outsideMs: harness };
    s.note(`${label}: ${JSON.stringify(results[label])}`);
    // Let the drop settle before the next one starts: the tray goes away by itself, or is closed.
    await s.wait(1500);
    if (await s.exists("upload-tray-close")) await s.click("upload-tray-close").catch(() => undefined);
    if (await s.exists("drop-toast-close")) await s.click("drop-toast-close").catch(() => undefined);
    if (await s.exists("drop-toast-cancel")) await s.click("drop-toast-cancel").catch(() => undefined);
  }

  await measure("small-40-files", "small", { kept: 40, litter: 5 }, { choose: null, expectFiles: 41 });
  await measure("mid-5000-files", "mid", { kept: 5_000, litter: 500 }, { choose: "copy", expectFiles: 5_001, waitMs: 420_000 });
  await measure("large-25000-files", "large", { kept: 25_000, litter: 3_000 }, { choose: "link", expectFiles: 25_001, waitMs: 420_000 });
  await fs.writeFile(path.join(s.reportDirectory, "drop-latency.json"), JSON.stringify(results, null, 2));
  s.check("the three drops were measured", Object.keys(results).length === 3, JSON.stringify(results));
});
