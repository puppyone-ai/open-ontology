// Database sources (D88): Add database through the app, the stack file and credential file it leaves, the check, doctor and import
// progress on the database page. The `oo` it runs is the canned one (tests/support/fake-oo-cli.mjs): this scenario proves what the app
// asks of `oo` and how it shows the answers; databases-live.t1.mjs does the same against real database containers.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";

// The canned `oo build` stops for the owner's review with a recorded real Builder draft, then continues from the review file.
process.env.FAKE_OO_REVIEW = "1";
const PASSWORD = "fake-pw-t1";
const PEM = "-----BEGIN CERTIFICATE-----\nMIIBsynthetic\n-----END CERTIFICATE-----";

runScenario({ name: "databases", title: "Add a database, check it, import it", engine: "fake", cliOo: "fake" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  // The first database writes the stack file, which carries the engine's model: the fake engine holds a synthetic one.
  await s.fakeDocs({ drives: [], model: { configured: true, provider: "openai-compatible", base_url: "https://model.example.test/v1", model: "synthetic", key_ref: `file:${path.join(s.root, "cli-home", "model-key")}` } });
  await s.click("nav-sources");
  await s.waitFor("screen-sources");
  await s.click("add-source");
  await s.click("add-source-database");
  await s.waitFor("add-database-name");

  // Nothing is saved and the form says what is missing.
  await s.click("add-database-confirm");
  s.check("an empty form is refused with field errors", await s.exists("add-database-name") && (await s.evaluate("document.querySelectorAll('.so-dialog .so-field-error').length")) >= 5);

  await s.type("add-database-name", "shop");
  await s.type("add-database-host", "db.example.test");
  await s.type("add-database-database", "shop");
  await s.type("add-database-user", "reader");
  await s.type("add-database-password", PASSWORD);
  await s.type("add-database-ca-pem", PEM);
  await s.type("add-database-include", "public.customers\npublic.orders");
  await s.click("add-database-test");
  await s.waitFor("add-database-check-ok");
  s.check("the connection check lists its steps", (await s.count("add-database-check")) >= 3);
  await s.shot("add-database-checked");

  await s.click("add-database-confirm");
  await s.waitFor("screen-database-detail", { "data-name": "shop" }, { timeoutMs: 30_000 });
  s.check("the database page opens after adding", true);

  const stackFile = path.join(s.userData, "engine", "stack.yaml");
  const stackText = await fs.readFile(stackFile, "utf8");
  const stack = JSON.parse(stackText);
  s.check("the stack file names the source and its tables", stack.sources?.[0]?.id === "shop" && stack.sources[0].scope.include.length === 2);
  s.check("the stack file holds a reference, not the password", !stackText.includes(PASSWORD) && /^file:/u.test(stack.sources[0].password.secret_ref));
  const secrets = path.join(s.root, "cli-home", ".config", "open-ontology", "secrets");
  const names = await fs.readdir(secrets);
  const pwName = names.find((name) => name.startsWith("src_shop_pw"));
  s.check("the password is a private file", pwName !== undefined && ((await fs.stat(path.join(secrets, pwName))).mode & 0o777) === 0o600, names.join(","));
  s.check("the password never reaches the page", !(await s.evaluate("document.documentElement.innerHTML")).includes(PASSWORD));
  const calls = (await fs.readFile(path.join(s.root, "cli-calls.jsonl"), "utf8")).split("\n").filter(Boolean).map((line) => JSON.parse(line));
  s.check("no oo command line carried the password", calls.every((call) => !JSON.stringify(call.args).includes(PASSWORD)));

  await s.click("database-test");
  await s.waitFor("database-test-result");
  await s.click("database-doctor");
  await s.waitFor("database-doctor-result", { "data-status": "ok" });
  s.check("the doctor result shows", true);
  await s.click("database-import");
  await s.waitFor("database-import-progress", {}, { timeoutMs: 15_000 });
  s.check("import progress shows while oo up and build run", true);
  // The build stops for the owner: the review screen shows what the Builder proposed (4 tables, 3 declared foreign keys, 6 candidates).
  await s.waitFor("build-review", {}, { timeoutMs: 60_000 });
  s.check("the build stops at the review instead of failing", true);
  s.check("the review opens with a summary bound to the CLI's hash", (await s.exists("review-summary")) && String(await s.attr("review-summary", "data-hash")).length === 12);
  s.check("the review lists the tables", (await s.count("review-entity")) === 4, String(await s.count("review-entity")));
  s.check("the review lists the relationships in groups", (await s.count("review-relation")) === 9 && (await s.count("review-group")) === 2, `${await s.count("review-relation")} in ${await s.count("review-group")}`);
  s.check("declared foreign keys are accepted and found candidates are rejected by default", (await s.evaluate(`[...document.querySelectorAll('[data-testid="review-relation"]')].every((row) => row.getAttribute("data-decision") === (row.getAttribute("data-origin") === "declared" ? "adopted" : "rejected"))`)) === true);
  s.check("every relationship says why", (await s.count("review-evidence")) === 9);
  await s.shot("build-review");
  // Flip one found candidate: accept it.
  await s.evaluate(`document.querySelector('[data-testid="review-relation"][data-origin="discovered"] [data-testid="review-accept"]').scrollIntoView({ block: "center" })`);
  await s.evaluate(`document.querySelector('[data-testid="review-relation"][data-origin="discovered"] [data-testid="review-accept"]').click()`);
  await s.until(`document.querySelector('[data-testid="review-changes"]')?.getAttribute("data-count") === "1"`, "one decision differs from the proposal");
  s.check("a changed decision is counted", true);
  await s.type("review-reason", "Checked with the data owner in the T1 run");
  await s.click("review-confirm");
  await s.waitFor("database-import-done", {}, { timeoutMs: 60_000 });
  const reviewCall = (await fs.readFile(path.join(s.root, "cli-calls.jsonl"), "utf8")).split("\n").filter(Boolean).map((line) => JSON.parse(line)).find((call) => call.reviewFile);
  s.check("the decision reached oo as a private file in a private folder", reviewCall?.reviewFileMode === 0o600 && reviewCall.reviewFileFolderMode === 0o700, JSON.stringify([reviewCall?.reviewFileMode, reviewCall?.reviewFileFolderMode]));
  s.check("the file carries the reason and the one override", reviewCall?.reviewFile?.reason?.startsWith("Checked with") && Object.keys(reviewCall.reviewFile.overrides?.items ?? {}).length >= 1);
  s.check("the database page is in its done phase", (await s.attr("database-pipeline", "data-phase")) === "done", String(await s.attr("database-pipeline", "data-phase")));
  s.check("the finished import offers the atlas, the YAML files and the Query tab", (await s.exists("build-stages")) && (await s.exists("database-open-files")) && (await s.exists("database-open-query")));
  s.check("the import ends with every stage complete", (await s.attrs("build-stage", "data-state")).every((state) => state === "complete"), (await s.attrs("build-stage", "data-state")).join(","));
  s.check("the finished build connects the Query tab to its Gateway", (await fs.readFile(path.join(s.userData, "engine", "gateway.json"), "utf8")).includes("127.0.0.1:9443"));
  await s.shot("database-detail");
  await s.click("database-import-cli-toggle");
  s.check("the equivalent commands are copyable text", (await s.text("database-import-cli-value")).includes("oo up"));

  await s.click("database-open-atlas");
  await s.waitFor("screen-ontology");
  await s.waitFor("atlas-mode-source", { "data-selected": "true" }, { timeoutMs: 30_000 });
  s.check("Open in Atlas lands on the source view", true);
  await s.click("nav-sources");
  await s.click("source-row", { "data-kind": "database" });
  await s.waitFor("screen-database-detail", { "data-name": "shop" });
  await s.click("database-remove");
  await s.click("database-remove-confirm");
  await s.waitFor("screen-sources");
  const after = (await fs.readFile(path.join(s.root, "cli-calls.jsonl"), "utf8")).split("\n").filter(Boolean).map((line) => JSON.parse(line));
  s.check("remove went through oo source rm", after.some((call) => call.args?.[0] === "source" && call.args[1] === "rm" && call.args[2] === "shop"));
});
