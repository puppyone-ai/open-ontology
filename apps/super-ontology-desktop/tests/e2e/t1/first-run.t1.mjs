// Volume 25 section 3, the five steps, on a fresh profile with the real engine: welcome and start (the app finds `oo` and starts
// `oo serve`), put the first files in (dropped on "my files": no configuration), watch them being understood, land in the ontology on a
// first entity with a fact and its source, and hand the ontology to an agent with a read-only token and the matching `oo` line.
import { runScenario } from "../harness/session.mjs";

const SETTLED = new Set(["parsed", "published"]);

runScenario({ name: "first-run", title: "First run: welcome to an agent token", timeoutMs: 290_000 }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.waitFor("screen-first-run");
  s.check("a fresh profile opens the first run at the welcome step", (await s.attr("screen-first-run", "data-step")) === "welcome");

  // 1. Welcome: one button; the engine is found and started with progress shown inline.
  await s.click("firstrun-start");
  await s.waitFor("firstrun-progress", { "data-state": "ready" }, { timeoutMs: 150_000 });
  s.check("the engine started from the welcome step", true);
  await s.waitFor("statusbar-engine", { "data-state": "running" });

  // 2. First files: drop them; they land in "my files" without any setting.
  await s.waitFor("firstrun-dropzone");
  await s.dropFiles("firstrun-dropzone", s.fixtures.files);
  await s.waitFor("firstrun-file-row", { "data-name": "alpha.md" }, { timeoutMs: 60_000 });
  s.check("every dropped file is listed", (await s.count("firstrun-file-row")) >= s.fixtures.files.length, String(await s.count("firstrun-file-row")));

  // 3. Understanding: per-file state until the files are processed (the person may leave earlier; the scenario waits).
  await s.until(`(() => { const rows = [...document.querySelectorAll('[data-testid="firstrun-file-row"]')]; return rows.length > 0 && rows.every((r) => ${JSON.stringify([...SETTLED])}.includes(r.getAttribute('data-state'))); })()`, "files processed", { timeoutMs: 150_000, interval: 1000 });
  s.check("the files were parsed", true);

  // 4. The ontology: the first entity with a fact that links back to its source.
  await s.click("firstrun-see-ontology");
  await s.waitFor("screen-ontology");
  await s.waitFor("entity-row", { "data-name": "Project Alpha" }, { timeoutMs: 90_000 });
  s.check("an entity recognised from the dropped files is listed", true);
  await s.click("entity-row", { "data-name": "Project Alpha" });
  await s.waitFor("screen-entity", { "data-name": "Project Alpha" });
  await s.waitFor("fact-row");
  s.check("the entity page shows a fact", (await s.count("fact-row")) > 0);
  s.check("a fact has its evidence one step away", (await s.attr("evidence-link", "data-file")) !== null, String(await s.attr("evidence-link", "data-file")));
  await s.shot("entity");

  // 5. Hand it to an agent: a read-only token, the connection text, the equivalent `oo` line.
  if (await s.exists("firstrun-create-token")) {
    await s.click("firstrun-create-token");
    await s.waitFor("firstrun-config-text");
    s.check("the connection text names the open-ontology MCP server", /open-ontology/u.test(await s.text("firstrun-config-text")));
    s.check("the equivalent oo line is shown", /\boo\b/u.test(await s.text("firstrun-cli-line")));
  } else {
    await s.click("nav-agents");
    await s.waitFor("screen-agents");
    await s.click("agent-token-create");
    await s.type("agent-token-name", "first-run-reader");
    await s.click("agent-token-submit");
    await s.waitFor("agent-token-secret");
    s.check("a token was created and is shown once", /^ooa_/u.test((await s.text("agent-token-secret")).trim()));
    await s.waitFor("agent-config-text");
  }
  s.check("the token secret never reaches the connection text", !/ooa_/u.test(await s.text((await s.exists("firstrun-config-text")) ? "firstrun-config-text" : "agent-config-text")));
  await s.shot("agent");
});
