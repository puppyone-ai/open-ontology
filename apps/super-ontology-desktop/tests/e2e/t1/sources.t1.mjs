// Sources: one list for every kind of source, a source's files with their states, and adding a local folder through the native dialog.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "sources", title: "Sources lists, opens and adds a source" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("sources");
  await s.waitFor("screen-sources");
  await s.waitFor("source-list");
  await s.waitFor("source-row", { "data-name": "Sample docs" });
  s.check("the registered folder is listed as a folder source", (await s.attr("source-row", "data-kind")) === "folder", String(await s.attr("source-row", "data-kind")));

  await s.click("source-row", { "data-name": "Sample docs" });
  await s.waitFor("screen-source-detail", { "data-name": "Sample docs" });
  await s.waitFor("file-panel");
  await s.waitFor("file-row", { "data-name": "alpha.md" });
  s.check("every fixture file is listed", (await s.count("file-row")) >= s.fixtures.count, String(await s.count("file-row")));
  s.check("files are in a processed state", (await s.attrs("file-row", "data-state")).every((value) => ["parsed", "published"].includes(value)), (await s.attrs("file-row", "data-state")).join(","));
  await s.shot("source-detail");

  // Add a second local folder through the native dialog (answered by the harness).
  const second = path.join(s.root, "Second docs");
  await fs.mkdir(second, { recursive: true });
  await fs.writeFile(path.join(second, "extra.md"), "# Extra\n\nA second folder.\n");
  await s.click("nav-sources");
  await s.waitFor("screen-sources");
  await s.click("add-source");
  await s.answerDialog(second);
  await s.click("add-source-folder");
  if (await s.exists("add-source-confirm")) await s.click("add-source-confirm");
  await s.waitFor("source-row", { "data-name": "Second docs" }, { timeoutMs: 60_000 });
  s.check("the added folder appears in the same list", (await s.count("source-row")) === 2, String(await s.count("source-row")));
  await s.shot("sources");
});
