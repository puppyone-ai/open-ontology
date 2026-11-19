// The same Add database flow against real database containers and the real `oo` (not part of a default run: --only databases-live).
// Start labelled containers with a TLS server certificate that has the serverAuth usage (macOS TLS requires it), then set
//   SO_T1_LIVE_DB='{"kind":"postgres","host":"127.0.0.1","port":55432,"database":"shop","user":"postgres","password":"...","caFile":"/abs/ca.pem","include":["public.customers"]}'
// and SO_CLI_OO=<absolute path of an oo built with docs; for mysql, with oo-mysql-doctor beside it>
// (the value comes from the environment and is never written to a report). Without it the scenario is skipped.
import fs from "node:fs/promises";
import path from "node:path";
import { EXIT_SKIPPED, runScenario } from "../harness/session.mjs";

const live = process.env.SO_T1_LIVE_DB ? JSON.parse(process.env.SO_T1_LIVE_DB) : null;
if (!live) { console.error("databases-live: set SO_T1_LIVE_DB (see the header of this file)"); process.exit(EXIT_SKIPPED); }

runScenario({ name: "databases-live", title: `Add a real ${live.kind} database` , engine: "fake" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  // The first source writes the stack file, which carries the engine's model: the fake engine holds a synthetic one.
  await s.fakeDocs({ drives: [], model: { configured: true, provider: "openai-compatible", base_url: "https://model.example.test/v1", model: "synthetic", key_ref: `file:${path.join(s.root, "cli-home", "model-key")}` } });
  await s.click("nav-sources");
  await s.click("add-source");
  await s.click("add-source-database");
  await s.waitFor("add-database-name");
  await s.evaluate(`(() => { const select = document.querySelector('[data-testid="add-database-kind"]'); select.value = ${JSON.stringify(live.kind)}; select.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await s.type("add-database-name", "live");
  await s.type("add-database-host", live.host);
  await s.type("add-database-port", String(live.port));
  await s.type("add-database-database", live.database);
  await s.type("add-database-user", live.user);
  await s.type("add-database-password", live.password);
  await s.type("add-database-ca-pem", await fs.readFile(live.caFile, "utf8"));
  await s.type("add-database-include", live.include.join("\n"));
  await s.click("add-database-test");
  await s.waitFor("add-database-check-ok", {}, { timeoutMs: 60_000 });
  s.check("the real database answers the connection check", true);
  await s.click("add-database-confirm");
  await s.waitFor("screen-database-detail", { "data-name": "live" }, { timeoutMs: 60_000 });
  await s.click("database-test");
  await s.waitFor("database-test-result", {}, { timeoutMs: 30_000 });
  s.check("the reachability test passes", true);
  await s.click("database-doctor");
  await s.waitFor("database-doctor-result", {}, { timeoutMs: 180_000 });
  s.check("the read-only doctor reads the catalog", (await s.attr("database-doctor-result", "data-status")) !== "blocked", String(await s.attr("database-doctor-result", "data-status")));
  const stackText = await fs.readFile(path.join(s.userData, "engine", "stack.yaml"), "utf8");
  s.check("the stack file holds no password", !stackText.includes(live.password));
  await s.shot("database-live");
});
