// The whole database path in the app against a real database, the real `oo` and a real model, with no CLI typed by a person:
//   add (check, add) -> import (oo up, oo build) -> review in the app -> publish -> Atlas source view -> Query.
// Not part of a default run: `--only databases-live`, with
//   SO_T1_LIVE=postgres|mysql        which database to start (a labelled local container, synthetic: 4 tables, 3 foreign keys)
//   SO_T1_OO_BIN=<an oo built with --features docs, with oo-docs-engine next to it>
//   SO_T1_MODEL_ENDPOINT_FILE / SO_T1_MODEL_KEY_FILE   private files holding the model endpoint and key (default: the two in ~/.config/open-ontology/secrets)
//   SO_T1_MODEL            the model name (default ds/deepseek-chat)
//   SO_T1_LITE_IMAGE       the runtime image reference (default: the digest pinned for this machine in deploy/lite/images.lock.json)
// Everything lives in a temporary folder that is also the home of every process; containers carry oo.lane=super-ontology-appp (the lite
// stack, whose lane label is 8 characters at most, carries oo.lane=appp) and are removed at the end. Endpoint, key and password values
// are read from files and typed into the app; they are never printed or written to the report.
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { EXIT_SKIPPED, runScenario } from "../harness/session.mjs";
import { startLiveDatabase } from "../harness/live-db.mjs";

const kind = process.env.SO_T1_LIVE;
const secretsHome = path.join(os.homedir(), ".config", "open-ontology", "secrets");
const endpointFile = process.env.SO_T1_MODEL_ENDPOINT_FILE ?? path.join(secretsHome, "model-endpoint-9r");
const keyFile = process.env.SO_T1_MODEL_KEY_FILE ?? path.join(secretsHome, "model-key-9r-sol");
const here = path.dirname(fileURLToPath(import.meta.url));
const appRoot = path.resolve(here, "..", "..", "..");

if (!kind) { console.error("databases-live: set SO_T1_LIVE=postgres|mysql (see the header of this file)"); process.exit(EXIT_SKIPPED); }
for (const file of [endpointFile, keyFile]) {
  try { await fs.access(file); } catch { console.error("databases-live: the model endpoint and key files are missing (SO_T1_MODEL_ENDPOINT_FILE, SO_T1_MODEL_KEY_FILE)"); process.exit(EXIT_SKIPPED); }
}

/** The runtime image the lock pins for this machine, as a local `repository@digest` reference. */
function liteImage() {
  if (process.env.SO_T1_LITE_IMAGE) return process.env.SO_T1_LITE_IMAGE;
  const lock = JSON.parse(execFileSync("cat", [path.resolve(appRoot, "..", "..", "deploy", "lite", "images.lock.json")], { encoding: "utf8" }));
  const digest = lock.platforms?.["linux/arm64"]?.image;
  const listed = execFileSync("docker", ["images", "--digests", "--format", "{{.Repository}}@{{.Digest}}"], { encoding: "utf8" }).split("\n");
  return listed.find((line) => digest && line.endsWith(`@${digest}`) && !line.startsWith("<none>")) ?? "";
}
process.env.OO_LITE_RUNTIME_IMAGE = liteImage();
process.env.OO_LITE_LANE = "appp";
process.env.OO_LITE_OFFLINE = "1";
if (!process.env.OO_LITE_RUNTIME_IMAGE) { console.error("databases-live: no local runtime image matches the pinned digest (set SO_T1_LITE_IMAGE)"); process.exit(EXIT_SKIPPED); }

const removeLabelled = () => {
  for (const [list, remove] of [[["ps", "-aq"], ["rm", "-f", "-v"]], [["volume", "ls", "-q"], ["volume", "rm", "-f"]], [["network", "ls", "-q"], ["network", "rm"]]]) {
    for (const lane of ["appp", "super-ontology-appp"]) {
      try { const ids = execFileSync("docker", [...list, "--filter", `label=oo.lane=${lane}`], { encoding: "utf8" }).split("\n").filter(Boolean); if (ids.length > 0) execFileSync("docker", [...remove, ...ids], { stdio: "ignore" }); } catch { /* nothing to remove */ }
    }
  }
};
process.on("exit", removeLabelled);

const dir = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-live-")));
await fs.chmod(dir, 0o700);
const database = await startLiveDatabase({ kind, dir });
const { live } = database;
process.on("exit", () => {
  try { execFileSync("docker", ["rm", "-f", "-v", live.container], { stdio: "ignore" }); } catch { /* gone */ }
  try { execFileSync("rm", ["-rf", dir], { stdio: "ignore" }); } catch { /* gone */ } // certificates and the password file
});

runScenario({ name: "databases-live", title: `A real ${live.kind} database, from Add to Query`, timeoutMs: 25 * 60_000, cliHome: "home" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  const mark = Date.now();
  const lap = (label) => s.note(`${label}: ${Math.round((Date.now() - mark) / 1000)}s`);

  // The model goes in through the app's own model settings call: the key lands in a private file the engine only references.
  const endpoint = (await fs.readFile(endpointFile, "utf8")).trim();
  const apiKey = (await fs.readFile(keyFile, "utf8")).trim();
  const model = { provider: "openai-compatible", baseUrl: endpoint, model: process.env.SO_T1_MODEL ?? "ds/deepseek-chat", apiKey };
  const saved = await s.evaluate(`(async () => { const current = await window.superOntology.invoke("model-config-read", {}); const revision = current.ok ? (current.value.revision ?? 0) : 0; const r = await window.superOntology.invoke("model-config-save", { ...${JSON.stringify(model)}, revision }); return r.ok ? "ok" : JSON.stringify(r.error).slice(0, 300); })()`);
  s.check("the model is saved through the app's model settings", saved === "ok", String(saved));

  await s.click("nav-sources");
  await s.click("add-source");
  await s.click("add-source-database");
  await s.waitFor("add-database-name");
  await s.evaluate(`(() => { const select = document.querySelector('[data-testid="add-database-kind"]'); select.value = ${JSON.stringify(live.kind)}; select.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  // The password file ends with a newline (as a hand-written secret does); a person types the password without it.
  const password = (await fs.readFile(live.passwordFile, "utf8")).replace(/\r?\n$/u, "");
  await s.type("add-database-name", "shop");
  await s.type("add-database-host", live.host);
  await s.type("add-database-port", String(live.port));
  await s.type("add-database-database", live.database);
  await s.type("add-database-user", live.user);
  await s.type("add-database-password", password);
  await s.type("add-database-ca-pem", await fs.readFile(live.caFile, "utf8"));
  await s.type("add-database-include", live.include.join("\n"));
  await s.click("add-database-test");
  await s.waitFor("add-database-check-ok", {}, { timeoutMs: 60_000 });
  s.check("the real database answers the connection check", true);
  await s.click("add-database-confirm");
  await s.waitFor("screen-database-detail", { "data-name": "shop" }, { timeoutMs: 90_000 });
  await s.click("database-test");
  await s.waitFor("database-test-result", {}, { timeoutMs: 30_000 });
  s.check("the reachability test passes", true);
  await s.click("database-doctor");
  await s.waitFor("database-doctor-result", {}, { timeoutMs: 180_000 });
  s.check("the read-only doctor reads the catalog", (await s.attr("database-doctor-result", "data-status")) !== "blocked", String(await s.attr("database-doctor-result", "data-status")));
  const stackText = await fs.readFile(path.join(s.userData, "engine", "stack.yaml"), "utf8");
  s.check("the stack file holds no password and no model key", !stackText.includes(password) && !stackText.includes(apiKey));
  lap("connected");

  // Import: oo up, then oo build until it stops for the owner.
  await s.click("database-import");
  await s.until(`!!document.querySelector('[data-testid="build-review"]') || !!document.querySelector('[data-testid="database-import-error"]')`, "the build review or an error", { timeoutMs: 15 * 60_000 });
  if (await s.exists("database-import-error")) {
    const failure = await s.text("database-import-error");
    await s.shot("import-failed");
    throw new Error(`the import stopped before the review: ${failure.replace(/\s+/gu, " ").slice(0, 400)}`);
  }
  lap("review ready");
  s.check("the build stops at the review", true);
  s.check("the review shows the four tables", (await s.count("review-entity")) === 4, String(await s.count("review-entity")));
  s.check("and the three declared foreign keys, accepted", (await s.evaluate(`document.querySelectorAll('[data-testid="review-relation"][data-origin="declared"][data-decision="adopted"]').length`)) === 3);
  await s.shot("build-review");
  // Decisions are real: flip one found candidate and flip it back (the published ontology keeps exactly the three foreign keys).
  const flip = (to) => s.evaluate(`(() => { const row = document.querySelector('[data-testid="review-relation"][data-origin="discovered"]'); if (!row) return false; row.querySelector('[data-testid="review-${to}"]').click(); return true; })()`);
  if (await flip("accept")) {
    await s.until(`document.querySelector('[data-testid="review-changes"]')?.getAttribute("data-count") === "1"`, "one decision changed");
    await flip("reject");
    await s.until(`document.querySelector('[data-testid="review-changes"]')?.getAttribute("data-count") === "0"`, "no decision changed");
    s.check("a decision can be changed and changed back", true);
  }
  await s.type("review-reason", "Checked in the app against the synthetic shop: the three declared foreign keys are the real links.");
  await s.click("review-confirm");
  await s.until(`!!document.querySelector('[data-testid="database-import-done"]') || !!document.querySelector('[data-testid="database-import-error"]') || !!document.querySelector('[data-testid="review-error"]')`, "the published build or an error", { timeoutMs: 10 * 60_000 });
  if (!(await s.exists("database-import-done"))) throw new Error(`the build did not publish: ${(await s.text("database-import-error") ?? await s.text("review-error") ?? "").replace(/\s+/gu, " ").slice(0, 400)}`);
  lap("published");
  s.check("every stage completes", (await s.attrs("build-stage", "data-state")).every((state) => state === "complete"), (await s.attrs("build-stage", "data-state")).join(","));
  await s.shot("import-done");

  // Atlas, source view: the four tables and the three foreign keys as the Gateway publishes them.
  await s.click("database-open-atlas");
  await s.waitFor("atlas-canvas", {}, { timeoutMs: 60_000 });
  await s.waitFor("atlas-mode-source", { "data-selected": "true" });
  await s.until(`Number(document.querySelector('[data-testid="atlas-canvas"]')?.getAttribute('data-cells')) >= 4 && Number(document.querySelector('[data-testid="atlas-canvas"]')?.getAttribute('data-links')) >= 3`, "four tables and three foreign keys in the atlas", { timeoutMs: 90_000 });
  s.check("the atlas source view shows the 4 tables", Number(await s.attr("atlas-canvas", "data-cells")) >= 4, String(await s.attr("atlas-canvas", "data-cells")));
  s.check("and the 3 foreign keys as links", Number(await s.attr("atlas-canvas", "data-links")) >= 3, String(await s.attr("atlas-canvas", "data-links")));
  await s.shot("atlas-source");

  const schema = live.kind === "mysql" ? "shop" : "public";
  // Query: the Gateway the build issued is already connected; the counts are the generated ones.
  await s.click("nav-ontology");
  await s.click("ontology-tab-query");
  await s.waitFor("query-gateway", { "data-state": "connected" }, { timeoutMs: 30_000 });
  s.check("Query is connected to the Gateway the build created", true);
  for (const [table, count] of Object.entries(live.counts)) {
    await s.type("query-sql-input", `select count(*) as n from ${schema}.${table}`);
    await s.click("query-run");
    await s.until(`(document.querySelector('[data-testid="query-result"]')?.innerText ?? "").split(/\\s+/u).includes(${JSON.stringify(String(count))})`, `${count} rows in ${table}`, { timeoutMs: 60_000 });
    s.check(`Query returns ${count} rows for ${table}`, true);
  }
  await s.type("query-sql-input", `select count(*) as n from ${schema}.orders o join ${schema}.customers c on c.id = o.customer_id`);
  await s.click("query-run");
  await s.until(`(document.querySelector('[data-testid="query-result"]')?.innerText ?? "").split(/\\s+/u).includes(${JSON.stringify(String(live.counts.orders))})`, "the join keeps every order", { timeoutMs: 60_000 });
  s.check("a join along the foreign key keeps every order", true);
  await s.shot("query-counts");
  s.check("no secret reached the page", !(await s.evaluate("document.documentElement.innerHTML")).includes(password) && !(await s.evaluate("document.documentElement.innerHTML")).includes(apiKey));
  lap("done");
});
