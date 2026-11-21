// Dropping files and folders anywhere in the window (25 §2, §4): a window-wide overlay, My files by default, the source under the pointer when
// there is one, folders kept with their structure, the engine's ignore rules applied and reported, the same drop twice left alone, and the
// choice to keep a dropped folder in sync. Real `Input.dispatchDragEvent` with real paths, a real engine, a window that is never on screen.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";

const write = async (file, text = "# Note\n\nA note for the drop test.\n") => { await fs.mkdir(path.dirname(file), { recursive: true }); await fs.writeFile(file, text); };

runScenario({ name: "drop", title: "Drag and drop of files and folders", timeoutMs: 290_000 }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("sources");
  await s.waitFor("screen-sources");
  await s.waitFor("source-row", { "data-name": "Sample docs" });

  // What the person drags in: one file, two files, a folder with litter, a folder next to a loose file, a file for a source row.
  const src = path.join(s.root, "dragged");
  const single = path.join(src, "single-note.md");
  const pair = [path.join(src, "pair-a.md"), path.join(src, "pair-b.txt")];
  const projectA = path.join(src, "ProjectA");
  const projectB = path.join(src, "ProjectB");
  const loose = path.join(src, "loose-note.md");
  const forSource = path.join(src, "for-sample-docs.md");
  await write(single, "# Single\n\nOne file dropped on the Sources page.\n");
  await write(pair[0], "# Pair A\n\nFirst of two files.\n");
  await write(pair[1], "Pair B is a plain text file.\n");
  await write(path.join(projectA, "readme.md"), "# Project A\n\nThe readme of project A.\n");
  await write(path.join(projectA, "notes", "deep.txt"), "A deeper note in project A.\n");
  await write(path.join(projectA, ".DS_Store"), "junk");
  await write(path.join(projectA, "node_modules", "dep", "index.md"), "# Dependency\n");
  await write(path.join(projectA, "~$lock.docx"), "lock");
  await fs.mkdir(path.join(projectA, "empty-folder"), { recursive: true });
  await fs.symlink("/etc/hosts", path.join(projectA, "escape-link.md"));
  await write(path.join(projectB, "plan.md"), "# Project B\n\nThe plan of project B.\n");
  await write(loose, "# Loose\n\nA loose file next to a folder.\n");
  await write(forSource, "# For a source\n\nDropped onto the row of a folder source.\n");

  const drives = async () => (await s.engineApi("/v1/docs/drives")).body?.items ?? [];
  const driveNamed = async (name) => (await drives()).find((drive) => drive.name === name);
  const filesOf = async (driveId) => (await s.engineApi(`/v1/docs/drives/${driveId}/files?limit=500&recursive=true`)).body?.items ?? [];
  const waitForFile = async (driveId, relative, { timeoutMs = 90_000 } = {}) => {
    const started = Date.now();
    while (Date.now() - started < timeoutMs) {
      const found = (await filesOf(driveId)).find((file) => file.path === relative);
      if (found) return found;
      await s.wait(500);
    }
    throw new Error(`the engine never listed ${relative}`);
  };
  const waitPublished = async (driveId, relatives, { timeoutMs = 120_000 } = {}) => {
    const started = Date.now();
    while (Date.now() - started < timeoutMs) {
      const files = await filesOf(driveId);
      const wanted = relatives.map((relative) => files.find((file) => file.path === relative));
      if (wanted.every((file) => file && ["published", "parsed"].includes(file.state))) return wanted;
      await s.wait(1000);
    }
    throw new Error(`not published in time: ${relatives.join(", ")}; the engine lists ${JSON.stringify((await filesOf(driveId)).map((file) => [file.path, file.state]))}`);
  };
  const exists = (file) => fs.access(file).then(() => true, () => false);

  // 1. The overlay: dragging over the Sources page names the target and counts the items; leaving takes it away.
  await s.dragHover("app-root", [single, ...pair]);
  await s.waitFor("drop-overlay", { "data-state": "ready", "data-target": "my-files" });
  s.check("the overlay says where the drop goes", /my files/iu.test(await s.text("drop-overlay-title")), await s.text("drop-overlay-title"));
  s.check("the overlay counts what is dragged", (await s.attr("drop-overlay-count", "data-count")) !== null, String(await s.attr("drop-overlay-count", "data-count")));
  await s.shot("drop-overlay");
  await s.dragCancel();
  await s.until("!document.querySelector('[data-testid=\"drop-overlay\"]')", "the overlay goes when the drag leaves");
  s.check("the overlay is gone after the drag leaves", !(await s.exists("drop-overlay")));

  // 2. One file onto the Sources list: it lands in My files (created on the way) and is published.
  await s.dropFiles("app-root", [single]);
  await s.waitFor("drop-toast", {}, { timeoutMs: 60_000 });
  await s.waitFor("source-row", { "data-name": "My files" }, { timeoutMs: 60_000 });
  s.check("My files appears in the Sources list", true);
  const myFiles = await driveNamed("My files");
  s.check("the engine has a My files drive", Boolean(myFiles));
  await waitForFile(myFiles.id, "single-note.md");
  await waitPublished(myFiles.id, ["single-note.md"]);
  s.check("a single file dropped on the list is published in My files", true);
  await s.shot("drop-toast");

  // 3. Several files at once.
  await s.dropFiles("app-root", pair);
  await waitPublished(myFiles.id, ["pair-a.md", "pair-b.txt"]);
  s.check("several files dropped together are all published", true);

  // 4. A folder: structure kept, the engine's ignore rules applied and reported, an empty folder kept, a link never followed.
  await s.dropFiles("app-root", [projectA]);
  await waitPublished(myFiles.id, ["ProjectA/readme.md", "ProjectA/notes/deep.txt"]);
  s.check("a dropped folder keeps its structure in My files", true);
  const root = myFiles.config.root;
  s.check("hidden, dependency and lock names were not copied", !(await exists(path.join(root, "ProjectA", ".DS_Store"))) && !(await exists(path.join(root, "ProjectA", "node_modules"))) && !(await exists(path.join(root, "ProjectA", "~$lock.docx"))));
  s.check("a link inside the folder was not followed", !(await exists(path.join(root, "ProjectA", "escape-link.md"))));
  s.check("an empty folder stays a folder", await exists(path.join(root, "ProjectA", "empty-folder")));
  await s.waitFor("upload-skipped", {}, { timeoutMs: 30_000 });
  s.check("what was skipped is listed with reasons", Number(await s.attr("upload-skipped", "data-count")) >= 4, String(await s.attr("upload-skipped", "data-count")));
  await s.waitFor("drop-keep-in-sync", {}, { timeoutMs: 30_000 }).catch(() => undefined);
  await s.shot("drop-folder");

  // 5. Folders mixed with files.
  await s.dropFiles("app-root", [projectB, loose]);
  await waitPublished(myFiles.id, ["ProjectB/plan.md", "loose-note.md"]);
  s.check("a folder dropped with a loose file lands with its structure, the file beside it", true);

  // 6. The same drop twice: nothing is copied again (no numbered copies).
  const before = (await filesOf(myFiles.id)).length;
  await s.dropFiles("app-root", [projectB, loose]);
  await s.until("document.querySelector('[data-testid=\"upload-skipped\"] [data-reason=\"duplicate\"]') !== null", "the repeated drop is reported as already there", { timeoutMs: 30_000 });
  await s.wait(1500);
  const after = (await filesOf(myFiles.id)).length;
  s.check("the same drop twice adds nothing", after === before, `${before} -> ${after}`);
  s.check("no numbered copy was made", !(await exists(path.join(root, "loose-note (2).md"))) && !(await exists(path.join(root, "ProjectB", "plan (2).md"))));

  // 7. Onto a source row: the file goes into that source, not into My files.
  const sample = await driveNamed("Sample docs");
  await s.dropFiles("source-row", [forSource], { "data-name": "Sample docs" });
  await waitPublished(sample.id, ["for-sample-docs.md"]);
  s.check("a file dropped on a source row lands in that source", true);
  s.check("and not in My files", !(await filesOf(myFiles.id)).some((file) => file.path === "for-sample-docs.md"));

  // 8. A folder kept in sync: the toast offers it after the copy, and a click registers the folder as a watched source.
  const synced = path.join(src, "WatchedFolder");
  await write(path.join(synced, "watched.md"), "# Watched\n\nThis folder is kept in sync.\n");
  await s.dropFiles("app-root", [synced]);
  await s.click("drop-keep-in-sync");
  await s.waitFor("drop-toast", { "data-sync": "done" }, { timeoutMs: 60_000 });
  await s.waitFor("source-row", { "data-name": "WatchedFolder" }, { timeoutMs: 60_000 });
  s.check("a dropped folder can also be kept in sync as a source", (await s.attr("source-row", "data-kind")) !== null);
  const watched = await driveNamed("WatchedFolder");
  // The engine scans a new folder source on its own poll (30 s); asking for the sync now keeps the run short, as the other scenarios do.
  await s.engineApi(`/v1/docs/drives/${watched.id}:sync`, { method: "POST", body: {}, headers: { "idempotency-key": `t1-drop-sync-${watched.id}`.padEnd(16, "0") } });
  await waitPublished(watched.id, ["watched.md"]);
  s.check("the watched folder's files reach the engine", true);
  s.check("the copy in My files was taken back, the original is untouched", !(await exists(path.join(root, "WatchedFolder"))) && (await exists(path.join(synced, "watched.md"))));
  s.check("the toast says the source replaced the copy", /in place of the copy/iu.test(await s.text("drop-toast-synced")), await s.text("drop-toast-synced"));
  await s.shot("sources-after-drop");
  s.check("no console errors", s.consoleErrors().length === 0, JSON.stringify(s.consoleErrors().slice(0, 3)));
});
