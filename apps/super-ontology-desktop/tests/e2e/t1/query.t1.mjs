// Ontology > Query, Tables, Knowledge, Files and Settings > Diagnostics (D88). A canned `oo` answers the Gateway commands; the scenario
// proves the Gateway is connected through granted files, that read-only SQL is validated and run through `oo query`, and that every
// action shows its equivalent command.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "query", title: "Query, tables, knowledge, files and diagnostics", engine: "fake", cliOo: "fake" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  const token = path.join(s.root, "gateway.token");
  await fs.writeFile(token, "oodt_synthetic_token_for_tests\n", { mode: 0o600 });

  await s.click("nav-ontology");
  await s.click("ontology-tab-query");
  await s.waitFor("query-gateway", { "data-state": "disconnected" });
  // A packaged app has neither the native-dialog queue nor the canned `oo` this scenario drives (it runs its own bundled oo), so the token
  // file cannot be granted: the packaged run proves the tab renders its disconnected state and stops here; the full path runs from source.
  if (s.packaged) {
    s.check("the packaged Query tab shows the Gateway as disconnected", (await s.count("query-gateway-token")) === 1);
    await s.shot("query-packaged-disconnected");
    return;
  }
  await s.answerDialog(token);
  await s.click("query-gateway-token");
  await s.until("document.querySelector('[data-testid=\"query-gateway\"]').textContent.includes('gateway.token')", "the chosen token file");
  await s.click("query-gateway-connect");
  await s.waitFor("query-gateway", { "data-state": "connected" });
  s.check("the Gateway is connected through a granted token file", true);

  await s.type("query-sql-input", "select id, name from public.customers");
  await s.click("query-validate");
  await s.waitFor("query-valid");
  await s.click("query-run");
  await s.waitFor("query-result", { "data-rows": "2" });
  s.check("rows come back as a table", (await s.count("query-result")) === 1);
  s.check("the page never shows the token", !(await s.evaluate("document.documentElement.innerHTML")).includes("oodt_synthetic"));
  await s.click("query-cli-toggle");
  const command = await s.text("query-cli-value");
  s.check("the equivalent oo query command is shown and carries no token value", command.includes("oo query") && command.includes("--token-file") && !command.includes("oodt_"), command.slice(0, 120));
  await s.shot("query-result");
  const calls = (await fs.readFile(path.join(s.root, "cli-calls.jsonl"), "utf8")).split("\n").filter(Boolean).map((line) => JSON.parse(line));
  s.check("the SQL went through oo query after a separator", calls.some((call) => call.args[0] === "query" && call.args.includes("--") && call.args.at(-1).startsWith("select id")));

  await s.click("query-mode-tables");
  await s.waitFor("query-table-row", { "data-name": "public.customers" });
  await s.click("query-table-row", { "data-name": "public.customers" });
  await s.waitFor("query-describe-cli-toggle");
  s.check("a table is described", (await s.evaluate("document.querySelectorAll('[data-testid=\"query-describe\"] tbody tr').length")) === 2);
  await s.click("query-mode-knowledge");
  await s.type("query-knowledge-input", "customer");
  await s.click("query-knowledge-run");
  await s.waitFor("query-knowledge-hit");
  await s.click("query-knowledge-hit");
  await s.waitFor("query-cat-body");
  s.check("a knowledge file is read", (await s.text("query-cat-body")).includes("entity: customer"));

  await s.click("ontology-tab-files");
  await s.waitFor("ontology-files");
  s.check("the files tab has no editable field", (await s.evaluate("document.querySelectorAll('[data-testid=\"ontology-files\"] input:not([type=search]):not(#ontology-files-prefix), [data-testid=\"ontology-files\"] textarea').length")) === 0);

  await s.click("nav-settings");
  await s.click("settings-section-diagnostics");
  await s.waitFor("settings-diagnostics");
  await s.waitFor("diag-oo");
  s.check("diagnostics lists oo, the readiness check, the runtime and the engine", (await s.count("diag-check")) === 1 && (await s.count("diag-runtime")) === 1 && (await s.count("diag-serve")) === 1);
  await s.shot("diagnostics");
});
