// Lane AH: whatever an agent does through the local `oo` CLI shows up live in the app, which shares the CLI's engine.
//   `oo serve` is started from the CLI in an isolated HOME; the app is then launched (never shown, no Dock icon) against that same HOME and
//   attaches to it. From the CLI: `oo docs upload` of three files, `oo source add fs`, a Postgres source added and imported (`oo source add`,
//   `oo init`, `oo up`, `oo build --resume --review-file`) and a review item accepted. Each change must be in the app within 5 s of the CLI
//   command returning, without a reload; the page is read over the DevTools protocol only (a few navigation clicks reach the hidden window, never the screen).
// Not part of a default run: `--only cli-live`, with
//   SO_T1_OO_BIN=<an oo built with --features docs, with oo-docs-engine next to it>
//   SO_T1_CLI_LIVE_DB=0     skips the database part (no Docker, no model); default: it runs against a labelled Postgres container
//   SO_T1_LANE=ah           label suffix of the containers (super-ontology-ah) and the lite stack lane (ah)
//   SO_T1_MODEL             model name (default cb/gpt-6.2-sol); endpoint and key come from the two private files named by
//                           SO_T1_MODEL_ENDPOINT_FILE / SO_T1_MODEL_KEY_FILE (default: the 9r files in ~/.config/open-ontology/secrets), copied by path into the isolated HOME.
// Everything lives under a temporary folder that is also the HOME of every process; the containers are removed at the end.
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { EXIT_SKIPPED, runScenario } from "../harness/session.mjs";
import { startLiveDatabase } from "../harness/live-db.mjs";

const withDatabase = process.env.SO_T1_CLI_LIVE_DB !== "0";
const lane = process.env.SO_T1_LANE ?? "ah";
process.env.SO_T1_LANE = lane;
const secretsHome = path.join(os.homedir(), ".config", "open-ontology", "secrets");
const endpointFile = process.env.SO_T1_MODEL_ENDPOINT_FILE ?? path.join(secretsHome, "model-endpoint-9r");
const keyFile = process.env.SO_T1_MODEL_KEY_FILE ?? path.join(secretsHome, "model-key-9r-sol");
const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const LIMIT_MS = 5_000;
// Debugging aid: SO_T1_CLI_LIVE_SKIP_DOCS=1 goes straight to the database part.
const skipDocs = process.env.SO_T1_CLI_LIVE_SKIP_DOCS === "1";

if (withDatabase) {
  for (const file of [endpointFile, keyFile]) {
    try { await fs.access(file); } catch { console.error("cli-live: the model endpoint and key files are missing (SO_T1_MODEL_ENDPOINT_FILE, SO_T1_MODEL_KEY_FILE); SO_T1_CLI_LIVE_DB=0 skips the database part"); process.exit(EXIT_SKIPPED); }
  }
  const lock = JSON.parse(await fs.readFile(path.resolve(appRoot, "..", "..", "deploy", "lite", "images.lock.json"), "utf8"));
  const digest = lock.platforms?.["linux/arm64"]?.image;
  const listed = execFileSync("docker", ["images", "--digests", "--format", "{{.Repository}}@{{.Digest}}"], { encoding: "utf8" }).split("\n");
  process.env.OO_LITE_RUNTIME_IMAGE = process.env.SO_T1_LITE_IMAGE ?? listed.find((line) => digest && line.endsWith(`@${digest}`) && !line.startsWith("<none>")) ?? "";
  if (!process.env.OO_LITE_RUNTIME_IMAGE) { console.error("cli-live: no local runtime image matches the pinned digest (SO_T1_LITE_IMAGE)"); process.exit(EXIT_SKIPPED); }
  process.env.OO_LITE_LANE = lane;
  process.env.OO_LITE_OFFLINE = "1";
}

const removeLabelled = () => {
  for (const [list, remove] of [[["ps", "-aq"], ["rm", "-f", "-v"]], [["volume", "ls", "-q"], ["volume", "rm", "-f"]], [["network", "ls", "-q"], ["network", "rm"]]]) {
    for (const label of [lane, `super-ontology-${lane}`]) {
      try { const ids = execFileSync("docker", [...list, "--filter", `label=oo.lane=${label}`], { encoding: "utf8" }).split("\n").filter(Boolean); if (ids.length > 0) execFileSync("docker", [...remove, ...ids], { stdio: "ignore" }); } catch { /* nothing to remove */ }
    }
  }
};
if (withDatabase) process.on("exit", removeLabelled);

const dir = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-cli-live-")));
await fs.chmod(dir, 0o700);
const database = withDatabase ? await startLiveDatabase({ kind: "postgres", dir }) : null;
process.on("exit", () => { if (database) { try { execFileSync("docker", ["rm", "-f", "-v", database.live.container], { stdio: "ignore" }); } catch { /* gone */ } } try { execFileSync("rm", ["-rf", dir], { stdio: "ignore" }); } catch { /* gone */ } });

runScenario({ name: "cli-live", title: "What the CLI does shows up in the app, live", sharedEngine: true, timeoutMs: 30 * 60_000, beforeLaunch: async (s) => { await s.startCliServe(); } }, async (s) => {
  const timings = [];
  let t0 = 0;
  /** The change must be on screen within LIMIT_MS of `since` (the moment the CLI command returned); the page is only read. */
  async function within(label, expression, since) {
    await s.until(expression, label, { timeoutMs: 60_000, interval: 50 });
    const ms = Date.now() - since;
    timings.push({ label, ms });
    s.check(`${label} (within ${LIMIT_MS / 1000} s, no reload)`, ms <= LIMIT_MS, `${ms} ms`);
  }
  const rows = (name, kind = null) => `document.querySelectorAll('[data-testid="source-row"][data-name=${JSON.stringify(name)}]${kind ? `[data-kind=${JSON.stringify(kind)}]` : ""}').length > 0`;
  const json = (result) => { try { return JSON.parse(result.stdout); } catch { return null; } };
  const cli = async (args, options) => {
    const r = await s.cli(args, options);
    s.note(`oo ${args.slice(0, 3).join(" ")} -> ${r.code}`);
    if (r.code !== 0 && r.code !== null) s.note(`  stderr: ${r.stderr.replace(/\s+/gu, " ").slice(0, 700)} | stdout: ${r.stdout.replace(/\s+/gu, " ").slice(0, 700)}`);
    return r;
  };

  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });

  // ---- one engine: the app attached to the serve the CLI started
  await s.until(`window.superOntology.invoke("engine-read", {}).then((r) => r.ok && r.value.status === "healthy")`, "the app sees a healthy engine", { timeoutMs: 120_000 });
  const engine = await s.evaluate(`window.superOntology.invoke("engine-read", {}).then((r) => ({ owned: r.value.ownedByDesktop, status: r.value.status }))`);
  s.check("the app attached to the engine the CLI started (it did not start another)", engine.owned === false && engine.status === "healthy", JSON.stringify(engine));
  const serve = json(await cli(["serve", "--status", "--json"]));
  const info = JSON.parse(await fs.readFile(path.join(s.stateDirectory, "serve.json"), "utf8"));
  s.check("one engine, one state folder: oo serve --status and the app name the same pid", serve?.pid === info.pid, `${serve?.pid} / ${info.pid}`);
  const state = await s.evaluate(`window.superOntology.invoke("engine-state-read", {}).then((r) => r.value)`);
  s.check("Settings > Engine reports the shared state (the CLI's folder) and the attach", state.shared === true && state.attached === true && state.stateDir === s.stateDirectory, JSON.stringify({ shared: state.shared, attached: state.attached }));
  await s.click("nav-sources");
  await s.waitFor("screen-sources");

  if (!skipDocs) {
    // ---- oo docs upload: three files
    const agentFiles = path.join(s.root, "agent-files");
    await fs.mkdir(agentFiles, { recursive: true });
    const names = ["alpha.md", "dana-lee.md", "kickoff.md"];
    await fs.writeFile(path.join(agentFiles, "alpha.md"), "---\ntype: project\nname: Project Alpha\nowner: Dana Lee\n---\n# Project Alpha\n\nOwner: Dana Lee\n\nStatus: in progress\n");
    await fs.writeFile(path.join(agentFiles, "dana-lee.md"), "---\ntype: person\nname: Dana Lee\nrole: Engineering lead\n---\n# Dana Lee\n\nRole: Engineering lead\n");
    await fs.writeFile(path.join(agentFiles, "kickoff.md"), "---\ntype: project\nname: Project Alpha\nowner: Sam Poe\n---\n# Project Alpha\n\nKickoff notes: Project Alpha starts in March. Owner: Sam Poe\n");
    t0 = Date.now();
    // The agent says who it is with OO_AGENT_NAME; the app shows the change as "via agent qa-bot".
    const agent = { env: { OO_AGENT_NAME: "qa-bot" } };
    const first = await cli(["docs", "upload", path.join(agentFiles, names[0]), "--wait", "none", "--json"], agent);
    s.check("oo docs upload creates the managed source", first.code === 0, `${first.code} ${first.stderr.slice(0, 200)}`);
    t0 = Date.now();
    await within("the managed source appears in Sources", rows("default"), t0);
    await s.click("source-row", { "data-name": "default" });
    await within("its first file row appears", `document.querySelectorAll('[data-testid="file-row"]').length === 1`, Date.now());
    // Two more files while the source is open: their rows must appear without leaving or reloading the page.
    const more = await cli(["docs", "upload", path.join(agentFiles, names[1]), path.join(agentFiles, names[2]), "--wait", "none", "--json"], agent);
    t0 = Date.now();
    s.check("oo docs upload of two more files succeeds", more.code === 0, `${more.code} ${more.stderr.slice(0, 200)}`);
    await within("the other two file rows appear in the open list", `document.querySelectorAll('[data-testid="file-row"]').length === 3`, t0);
    const wait = await cli(["docs", "wait", "--state", "published", "--timeout", "180s", "--json"]);
    t0 = Date.now();
    s.check("the files settle (oo docs wait)", wait.code === 0, `${wait.code}`);
    await within("every file row shows its settled state", `[...document.querySelectorAll('[data-testid="file-row"]')].every((row) => ["published", "stored", "indexed", "extracted"].includes(row.getAttribute("data-state")))`, t0);
    await within("the file rows carry the actor tag 'via agent qa-bot'", `[...document.querySelectorAll('[data-testid="file-row"] [data-testid="actor-tag"]')].some((tag) => tag.getAttribute("data-actor") === "agent" && tag.getAttribute("data-name") === "qa-bot")`, t0);
    await s.shot("after-upload");

    // ---- oo source add fs
    await s.click("nav-sources");
    await s.waitFor("screen-sources");
    const folder = path.join(s.root, "Notes folder");
    await fs.mkdir(folder, { recursive: true });
    await fs.writeFile(path.join(folder, "plan.md"), "# Plan\n\nProject Alpha ships in April.\n");
    await fs.writeFile(path.join(folder, "risks.md"), "# Risks\n\nDana Lee tracks the risks of Project Alpha.\n");
    const added = await cli(["source", "add", "fs", "--name", "notes", "--root", folder, "--json"]);
    t0 = Date.now();
    s.check("oo source add fs succeeds", added.code === 0, `${added.code} ${added.stderr.slice(0, 200)}`);
    await within("the folder source appears in Sources", rows("notes", "folder"), t0);
    await within("its two files are counted", `[...document.querySelectorAll('[data-testid="source-row"][data-name="notes"]')].some((row) => /\\b2 files\\b/.test(row.innerText))`, t0);

    // ---- a review item accepted from the CLI
    await s.click("nav-home");
    await s.waitFor("screen-home");
    const pending = async (seconds) => {
      for (let attempt = 0; attempt < seconds / 2; attempt += 1) {
        const found = json(await s.cli(["docs", "review", "ls", "--status", "pending", "--json"]))?.items ?? [];
        if (found.length > 0) return found;
        await s.wait(2_000);
      }
      return [];
    };
    let items = await pending(20);
    if (items.length === 0) {
      // Two documents that name the same project become a merge candidate once a further document goes through the pipeline.
      await fs.writeFile(path.join(agentFiles, "extra.md"), "---\ntype: project\nname: Project Beta\nowner: Kim Ray\n---\n# Project Beta\n");
      await cli(["docs", "upload", path.join(agentFiles, "extra.md"), "--wait", "terminal", "--json"], agent);
      items = await pending(90);
    }
    if (items.length === 0) {
      s.note("the engine produced no pending review item for these files");
    } else {
      // Home puts "review what is waiting" first as soon as an item is pending, and takes it away when none is left.
      const step = `document.querySelector('[data-testid="home-next-step"]')?.getAttribute("data-step")`;
      await within("Home asks for a review as soon as the CLI lists a pending item", `${step} === "review"`, Date.now());
      const decided = await cli(["docs", "review", "decide", "accept", items[0].id, "--json"]);
      t0 = Date.now();
      s.check("oo docs review decide accept succeeds", decided.code === 0, `${decided.code} ${decided.stderr.slice(0, 200)}`);
      await within("the review request leaves Home after the CLI accepts the item", `${step} !== "review"`, t0);
    }
    await within("Home lists what changed with its actor", `document.querySelectorAll('[data-testid="activity-row"] [data-testid="actor-tag"]').length > 0`, Date.now());

  }

  // ---- a Postgres database: source add, init, up, build with the CLI
  if (database) {
    const { live } = database;
    let cellsBefore = 0;
    if (!skipDocs) {
      await s.click("nav-ontology");
      await s.waitFor("screen-ontology");
      await s.click("ontology-tab-atlas");
      await s.waitFor("atlas-canvas", {}, { timeoutMs: 60_000 });
      // Source view: the files of the sources, and (after the build) the tables of the database next to them.
      await s.click("atlas-mode-source");
      await s.waitFor("atlas-mode-source", { "data-selected": "true" });
      await s.wait(1_000);
      cellsBefore = Number(await s.attr("atlas", "data-live-cells"));
    }
    s.note(`atlas cells before the database: ${cellsBefore}`);
    // The model reaches the stack by path: the key is copied (0600) into the isolated HOME's secret store, never printed.
    const secrets = path.join(s.root, "cli-home", ".config", "open-ontology", "secrets");
    await fs.mkdir(secrets, { recursive: true, mode: 0o700 });
    await fs.copyFile(keyFile, path.join(secrets, "model-key"));
    await fs.chmod(path.join(secrets, "model-key"), 0o600);
    const endpoint = (await fs.readFile(endpointFile, "utf8")).trim();
    const project = path.join(s.root, "shop-project");
    await fs.mkdir(project, { recursive: true });
    const stack = path.join(project, "stack.yaml");
    await fs.writeFile(stack, `${JSON.stringify({ version: "oo.stack/v1", model: { provider: "openai-compatible", base_url: endpoint, model: process.env.SO_T1_MODEL ?? "cb/gpt-6.2-sol", key: { secret_ref: `file:${path.join(secrets, "model-key")}` } } })}\n`, { mode: 0o600 });
    // oo refuses a credential file others can read: the certificate authority goes in as a private (0600) copy, as the app's own Add database does.
    const caFile = path.join(secrets, "ca_shop.pem");
    await fs.copyFile(live.caFile, caFile);
    await fs.chmod(caFile, 0o600);
    const password = (await fs.readFile(live.passwordFile, "utf8")).replace(/\r?\n$/u, "");
    const addDb = await cli(["source", "add", "postgres", "--name", "shop", "--host", live.host, "--port", String(live.port), "--database", live.database, "--user", live.user, "--tls-ca-file", caFile, ...live.include.flatMap((entry) => ["--include", entry]), "--password-stdin", "--config", stack, "--json"], { stdin: `${password}\n`, cwd: project });
    t0 = Date.now();
    s.check("oo source add postgres succeeds", addDb.code === 0, `${addDb.code} ${addDb.stderr.slice(0, 300)}`);
    await s.click("nav-sources");
    await s.waitFor("screen-sources");
    await within("the database source appears in Sources", rows("shop", "database"), t0);
    if (!skipDocs) {
      await s.click("nav-ontology");
      await s.waitFor("atlas-canvas", {}, { timeoutMs: 30_000 });
    }
    const initialised = await cli(["init", "--config", stack, "--yes", "--json"], { cwd: project, timeoutMs: 240_000 });
    s.check("oo init succeeds", initialised.code === 0, `${initialised.code} ${initialised.stderr.slice(0, 300)}`);
    const up = await cli(["up", "--config", stack, "--yes", "--json"], { cwd: project, timeoutMs: 20 * 60_000 });
    s.check("oo up succeeds", up.code === 0, `${up.code} ${up.stderr.slice(0, 300)}`);
    const first = await cli(["build", "--config", stack, "--yes", "--json"], { cwd: project, timeoutMs: 20 * 60_000 });
    s.check("oo build stops at the review the CLI asks for", first.code === 1 || first.code === 0, `${first.code} ${first.stderr.slice(0, 300)}`);
    let finalBuild = first;
    if (first.code !== 0) {
      const reviewFile = path.join(s.root, "review.json");
      await fs.writeFile(reviewFile, `${JSON.stringify({ protocol: "oo.build-review/v1", reason: "Checked against the synthetic shop: the declared foreign keys are the real links." })}\n`, { mode: 0o600 });
      finalBuild = await cli(["build", "--resume", "--config", stack, "--review-file", reviewFile, "--yes", "--json"], { cwd: project, timeoutMs: 20 * 60_000 });
    }
    if (skipDocs) {
      // Debugging mode only: the atlas is opened after the build, so the live refresh is not what is measured here.
      await s.click("nav-ontology");
      await s.waitFor("screen-ontology");
      await s.click("ontology-tab-atlas");
      await s.waitFor("atlas-canvas", {}, { timeoutMs: 60_000 });
      await s.click("atlas-mode-source");
      await s.waitFor("atlas-mode-source", { "data-selected": "true" });
    }
    t0 = Date.now();
    s.check("oo build --resume publishes", finalBuild.code === 0, `${finalBuild.code} ${finalBuild.stderr.slice(0, 300)}`);
    // On a timeout the run says what the app's own reads return, so the cause is in the report (counts and codes only).
    const diagnose = async () => {
      const read = await s.evaluate(`(async () => { const schema = await window.superOntology.invoke("ontology-schema", { limit: 50 }); const dbs = await window.superOntology.invoke("cli-db-list", {}); const sub = await window.superOntology.invoke("events-subscribe", {}); const since = sub.ok ? await window.superOntology.invoke("events-since", { instanceId: sub.value.instanceId, seq: 0 }) : sub; const dbEvents = since.ok ? since.value.events.filter((e) => e.type.startsWith("db.")).map((e) => [e.seq, e.type, e.data.command ?? e.data.action, e.data.ok, e.data.published, e.actor?.kind].join(":")) : since.error; const atlas = document.querySelector('[data-testid="atlas-canvas"]'); return JSON.stringify({ dbEvents, schema: schema.ok ? { tables: schema.value.tables?.length, relationships: schema.value.relationships?.length, sides: schema.value.sides, warnings: schema.value.warnings } : schema.error, dbs: dbs.ok ? (dbs.value.data?.sources ?? []).map((x) => x.name) : dbs.error, cells: atlas?.getAttribute("data-cells"), links: atlas?.getAttribute("data-links"), live: document.querySelector('[data-testid="atlas"]')?.getAttribute("data-live-cells") }); })()`).catch((error) => String(error));
      s.note(`atlas diagnosis: ${String(read).slice(0, 900)}`);
      // Leaving the Atlas and coming back reads everything again: if the tables are there now, the earlier refresh came too early.
      await s.click("nav-home");
      await s.click("nav-ontology");
      await s.waitFor("atlas-canvas", {}, { timeoutMs: 30_000 }).catch(() => {});
      await s.wait(4_000);
      s.note(`atlas after coming back: cells ${await s.attr("atlas-canvas", "data-cells")}, links ${await s.attr("atlas-canvas", "data-links")}`);
    };
    await within("the atlas gains the four tables", `Number(document.querySelector('[data-testid="atlas"]')?.getAttribute("data-live-cells")) >= ${cellsBefore + 4}`, t0).catch(async (error) => { await diagnose(); throw error; });
    await within("and the three foreign keys as links", `Number(document.querySelector('[data-testid="atlas"]')?.getAttribute("data-live-links")) >= 3`, t0);
    await s.shot("atlas-with-database");
    await s.click("nav-sources");
    await s.waitFor("screen-sources");
    await s.click("source-row", { "data-name": "shop" });
    await s.waitFor("screen-database-detail", { "data-name": "shop" }, { timeoutMs: 30_000 });
    s.check("the database page shows what the CLI built (every stage complete)", (await s.attrs("build-stage", "data-state")).length > 0 && (await s.attrs("build-stage", "data-state")).every((value) => value === "complete"), (await s.attrs("build-stage", "data-state")).join(","));
    s.check("the stack file holds no password", !(await fs.readFile(stack, "utf8")).includes(password));
    const down = await cli(["down", "--config", stack, "--yes", "--json"], { cwd: project, timeoutMs: 5 * 60_000 });
    s.note(`oo down -> ${down.code}`);
  }
  s.note(`timings: ${timings.map((entry) => `${entry.label}: ${entry.ms} ms`).join("; ")}`);
  s.check("the page threw no console errors", s.consoleErrors().length === 0, s.consoleErrors().map((e) => e.text).join(" | ").slice(0, 300));
});
