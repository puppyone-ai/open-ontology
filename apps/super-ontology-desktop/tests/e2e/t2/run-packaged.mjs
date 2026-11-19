#!/usr/bin/env node
// T2 (volume 25 section 6): black-box scenarios against the PACKAGED app from the DMG. Operation goes through Peekaboo by accessible
// name; assertions only read the renderer over the DevTools protocol. It needs macOS Accessibility and Screen Recording permission
// for the host that runs it and a quiet desktop, so it is a LOCAL gate, never CI. Start it from a Terminal window with run-t2.command.
//
//   node tests/e2e/t2/run-packaged.mjs --app "/Applications/Super Ontology.app" --scenarios all
//   node tests/e2e/t2/run-packaged.mjs --scenarios P01,P03 --locales en
//   node tests/e2e/t2/run-packaged.mjs --dry-run         validates the scenarios and resolves every accessible name; launches nothing
//
// Options: --engine-bin <path of oo-docs-engine> (only for an app whose bundle lacks it), --evidence <dir>, --expect-sha256 <hash of Contents/MacOS/Super Ontology>, --locales en,zh-Hans, --allow-unresolved
// (dry run: do not fail on names the renderer's catalog does not have yet), --no-record.
// Limits: a scenario gets at most 2 attempts of 5 minutes each. Fixtures are created once, up front, and chosen by full path.
// Scenario file: scenarios/<id>-<name>.t2.json; names are message keys of the app catalog (`app.<key>`, resolved per locale) or, when a
// step says `"literal": true`, text a person would read (an entity or source name from the fixtures).
import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { connectToPage, readDevToolsPort } from "../harness/cdp.mjs";
import { createFixtures, createLiteRuntimeStub } from "../harness/fixtures.mjs";
import { createPeekaboo } from "../harness/peekaboo.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const appRoot = path.resolve(here, "..", "..", "..");
export const SCENARIO_DIRECTORY = path.join(here, "scenarios");
export const ASSERTION_DIRECTORY = path.join(here, "assertions");
export const LOCALES = ["en", "zh-Hans"];
const APP_NAME = "Super Ontology";
const STEP_KINDS = new Set(["launch", "relaunch", "quit", "click", "type", "press", "typeFocused", "chooseFolder", "wait", "assert", "screenshot", "provision"]);
const ATTEMPT_MS = 5 * 60_000;
const EXIT = { ok: 0, failed: 1, usage: 2, permission: 78 };
const TEST_ID_ATTRIBUTE = ["data", "testid"].join("-");

const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

/** The accessible name of a message key: the catalog text up to its first placeholder. */
export function nameFor(catalog, key) {
  const text = catalog[key.replace(/^app\./u, "")];
  if (typeof text !== "string") throw new Error(`no message ${key}`);
  return text.split(/\{/u)[0].trim();
}

export async function loadCatalogs(localesDirectory = process.env.SO_LOCALES_DIR ?? path.join(appRoot, "src", "locales")) {
  return Object.fromEntries(await Promise.all(LOCALES.map(async (locale) => [locale, JSON.parse(await fs.readFile(path.join(localesDirectory, locale, "app.json"), "utf8").catch(() => "{}"))])));
}

/** Problems of one scenario document (empty when valid). */
export function validateScenario(scenario, { catalogs, assertionNames, allowUnresolved = false }) {
  const problems = [];
  const need = (condition, text) => { if (!condition) problems.push(text); };
  need(/^P\d{2}$/u.test(scenario.id ?? ""), "id is P<nn>");
  need(typeof scenario.title === "string" && scenario.title.length > 0, "title");
  need(Array.isArray(scenario.steps) && scenario.steps.length > 0, "steps");
  need(Array.isArray(scenario.locales) && scenario.locales.length > 0 && scenario.locales.every((locale) => LOCALES.includes(locale)), "locales are en and/or zh-Hans");
  for (const [index, step] of (scenario.steps ?? []).entries()) {
    const where = `step ${index + 1}`;
    need(STEP_KINDS.has(step.do), `${where}: unknown action ${step.do}`);
    if (["click", "type"].includes(step.do)) {
      need(typeof step.name === "string", `${where}: ${step.do} needs a name`);
      if (typeof step.name === "string" && !step.literal) {
        need(step.name.startsWith("app."), `${where}: name ${step.name} is not a message key (app.<key>); use "literal": true for fixture text`);
        if (!allowUnresolved) for (const locale of scenario.locales ?? []) { try { nameFor(catalogs[locale], step.name); } catch { problems.push(`${where}: ${step.name} has no message in ${locale}`); } }
      }
    }
    if (step.do === "assert") {
      need(assertionNames.has(step.assertion), `${where}: assertion ${step.assertion} does not exist`);
      need(!JSON.stringify(step).includes(TEST_ID_ATTRIBUTE), `${where}: write "testid", not the attribute`);
    }
    if (step.do === "wait") need(Number.isInteger(step.ms) && step.ms > 0, `${where}: wait ms`);
    if (step.do === "chooseFolder") need(typeof step.path === "string", `${where}: chooseFolder needs a path (a variable such as \${secondDocs})`);
  }
  return problems;
}

export async function loadScenarios(ids = null) {
  const names = (await fs.readdir(SCENARIO_DIRECTORY)).filter((name) => name.endsWith(".t2.json")).sort();
  const scenarios = await Promise.all(names.map(async (name) => ({ file: name, ...JSON.parse(await fs.readFile(path.join(SCENARIO_DIRECTORY, name), "utf8")) })));
  return ids ? scenarios.filter((scenario) => ids.includes(scenario.id)) : scenarios;
}

export function expandIds(text) {
  return text.split(",").flatMap((part) => {
    const range = part.trim().match(/^P(\d+)-P?(\d+)$/u);
    if (!range) return [part.trim()];
    return Array.from({ length: Number(range[2]) - Number(range[1]) + 1 }, (_, index) => `P${String(Number(range[1]) + index).padStart(2, "0")}`);
  });
}

async function loadAssertions() {
  const modules = {};
  for (const name of (await fs.readdir(ASSERTION_DIRECTORY)).filter((file) => file.endsWith(".mjs"))) modules[name.replace(/\.mjs$/u, "")] = (await import(pathToFileURL(path.join(ASSERTION_DIRECTORY, name)))).default;
  return modules;
}

const sha256 = async (file) => crypto.createHash("sha256").update(await fs.readFile(file)).digest("hex");

async function attemptScenario({ scenario, locale, catalog, assertions, application, evidence, profileRoot, fixtures, options }) {
  const run = { id: scenario.id, locale, ok: false, steps: [], error: null };
  const directory = path.join(evidence, `${scenario.id}-${locale}`);
  await fs.mkdir(directory, { recursive: true });
  const profile = path.join(profileRoot, `${scenario.id}-${locale}-${Date.now()}`);
  const stateDirectory = path.join(profile, "engine", "state");
  const peekaboo = createPeekaboo({ app: APP_NAME });
  let child = null;
  let cdp = null;
  let recording = null;
  const variables = { sampleDocs: fixtures.folder, secondDocs: fixtures.second, dropFiles: fixtures.loose };
  const text = (value) => String(value).replaceAll(/\$\{(\w+)\}/gu, (_whole, name) => variables[name] ?? "");

  const launch = async () => {
    await fs.mkdir(profile, { recursive: true });
    await fs.rm(path.join(profile, "DevToolsActivePort"), { force: true });
    // The language is the app's own setting; a relaunch keeps whatever the profile already holds.
    // The embedded runtime keeps the run off Docker: no container is ever created by a T2 run (the host may run other lanes' stacks).
    await fs.writeFile(path.join(profile, "settings.json"), JSON.stringify({ language: locale, runtime: "lite" }), { flag: "wx", mode: 0o600 }).catch(() => undefined);
    child = spawn(application.executable, [`--user-data-dir=${profile}`, "--remote-debugging-port=0"], {
      env: { ...process.env, SUPER_ONTOLOGY_USER_DATA: profile, SUPER_ONTOLOGY_LITE_DIR: fixtures.lite, ...(options.engineBin ? { OO_DOCS_ENGINE_BIN: options.engineBin } : {}) }, stdio: ["ignore", "pipe", "pipe"],
    });
    const log = [];
    child.stdout.on("data", (chunk) => log.push(String(chunk)));
    child.stderr.on("data", (chunk) => log.push(String(chunk)));
    child.once("exit", () => { fs.writeFile(path.join(directory, "main-process.log"), log.join("")).catch(() => undefined); });
    cdp = await connectToPage(await readDevToolsPort(profile), { urlPattern: /^app:/u });
    await peekaboo.retarget(child.pid);
  };
  const quit = async () => {
    cdp?.close();
    if (child && child.exitCode === null) { child.kill("SIGTERM"); await new Promise((resolve) => { child.once("exit", resolve); setTimeout(resolve, 20_000); }); }
    child = null;
    cdp = null;
  };
  async function engineApi(route, { method = "GET", body, headers = {} } = {}) {
    const info = JSON.parse(await fs.readFile(path.join(stateDirectory, "serve.json"), "utf8"));
    const token = (await fs.readFile(info.token_file ?? path.join(stateDirectory, "serve.token"), "utf8")).trim();
    const response = await fetch(`http://127.0.0.1:${info.port}${route}`, { method, headers: { authorization: `Bearer ${token}`, ...(body ? { "content-type": "application/json" } : {}), ...headers }, body: body ? JSON.stringify(body) : undefined });
    return { status: response.status, body: await response.json().catch(() => null) };
  }
  async function provision(step) {
    switch (step.action) {
      // Waits for the engine the app started, registers a folder as a source and waits until its files have settled.
      case "seedSource": {
        const folder = text(step.path);
        for (let attempt = 0; attempt < 240; attempt += 1) { try { if ((await engineApi("/v1/docs/health")).status === 200) break; } catch { /* not yet */ } await sleep(500); }
        const created = await engineApi("/v1/docs/drives", { method: "POST", headers: { "idempotency-key": `t2-${scenario.id}-${step.name}-${Date.now()}`.replace(/[^A-Za-z0-9_-]/gu, "-").padEnd(16, "0") }, body: { name: step.name, backend: "fs", config: { root: folder } } });
        if (![200, 201].includes(created.status)) throw new Error(`source create answered ${created.status}`);
        await engineApi(`/v1/docs/drives/${created.body.id}:sync`, { method: "POST", body: {}, headers: { "idempotency-key": `t2-sync-${created.body.id}-${Date.now()}` } });
        const settled = new Set(["parsed", "published", "failed", "skipped", "unsupported"]);
        for (let attempt = 0; attempt < 240; attempt += 1) {
          const { body } = await engineApi(`/v1/docs/drives/${created.body.id}/files?limit=200`);
          const states = (body?.items ?? []).map((file) => file.state);
          if (states.length >= fixtures.count && states.every((value) => settled.has(value))) return;
          await sleep(1000);
        }
        throw new Error("the seeded source did not settle");
      }
      default: throw new Error(`unknown provision action ${step.action}`);
    }
  }
  try {
    if (options.record) recording = spawn("screencapture", ["-v", path.join(directory, "run.mov")], { stdio: "ignore" });
    for (const [index, step] of scenario.steps.entries()) {
      const label = `${String(index + 1).padStart(2, "0")}-${step.do}`;
      const started = Date.now();
      const nameOf = () => (step.literal ? text(step.name) : nameFor(catalog, step.name));
      switch (step.do) {
        case "launch": await launch(); break;
        case "relaunch": await quit(); await launch(); break;
        case "quit": await quit(); break;
        case "click": await peekaboo.click(nameOf()); break;
        case "type": await peekaboo.click(nameOf()); await peekaboo.type(text(step.text)); break;
        case "press": await peekaboo.press(step.chord); break;
        case "typeFocused": await peekaboo.type(text(step.text)); break;
        // The native open panel: Go to folder (Cmd+Shift+G), the full path, Return to go there, Return to choose it.
        case "chooseFolder": await sleep(800); await peekaboo.press("cmd+shift+g", { native: true }); await sleep(600); await peekaboo.type(text(step.path), { native: true }); await sleep(300); for (const code of [36, 76, 36]) { await peekaboo.nativeReturn(code); await sleep(2000); } break;
        case "wait": await sleep(step.ms); break;
        case "provision": await provision(step); break;
        case "screenshot": await peekaboo.see(path.join(directory, `${label}-${step.label}.png`)); break;
        case "assert": {
          const result = await assertions[step.assertion]({ cdp, args: step.args ?? {}, locale, directory, variables, profile });
          if (!result.ok) throw new Error(`assertion ${step.assertion} failed: ${result.detail}`);
          run.steps.push({ label, ms: Date.now() - started, detail: result.detail });
          continue;
        }
        default: throw new Error(`unhandled action ${step.do}`);
      }
      run.steps.push({ label, ms: Date.now() - started });
    }
    run.ok = true;
  } catch (error) {
    run.error = error instanceof Error ? error.stack : String(error);
    await peekaboo.see(path.join(directory, "failure.png")).catch(() => undefined);
    await fs.writeFile(path.join(directory, "renderer-console.json"), JSON.stringify(cdp?.console() ?? [], null, 2)).catch(() => undefined);
  } finally {
    recording?.kill("SIGINT");
    await quit();
    // The engine the app started is stopped with the app; make sure none is left behind.
    try { const info = JSON.parse(await fs.readFile(path.join(stateDirectory, "serve.json"), "utf8")); process.kill(info.pid, "SIGTERM"); } catch { /* none */ }
  }
  return run;
}

async function withLimit(work, limitMs) {
  let timer;
  const timeout = new Promise((resolve) => { timer = setTimeout(() => resolve({ ok: false, steps: [], error: `exceeded ${limitMs / 60_000} minutes` }), limitMs); });
  try { return await Promise.race([work, timeout]); } finally { clearTimeout(timer); }
}

async function main() {
  const args = process.argv.slice(2);
  const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const flag = (name) => args.includes(name);
  const ids = option("--scenarios") && option("--scenarios") !== "all" ? expandIds(option("--scenarios")) : null;
  const locales = option("--locales")?.split(",") ?? LOCALES;
  const catalogs = await loadCatalogs();
  const assertions = await loadAssertions();
  const scenarios = await loadScenarios(ids);
  if (scenarios.length === 0 || (ids && scenarios.length !== ids.length)) { console.error("no such scenario"); return EXIT.usage; }
  let invalid = 0;
  for (const scenario of scenarios) {
    const problems = validateScenario(scenario, { catalogs, assertionNames: new Set(Object.keys(assertions)), allowUnresolved: flag("--allow-unresolved") });
    for (const problem of problems) console.error(`${scenario.id}: ${problem}`);
    invalid += problems.length;
  }
  if (invalid > 0) return EXIT.usage;
  if (flag("--dry-run")) { console.log(`dry run ok: ${scenarios.length} scenarios, every name resolves in ${LOCALES.join(" and ")}`); return EXIT.ok; }

  const bundle = option("--app") ?? "/Applications/Super Ontology.app";
  const executable = path.join(bundle, "Contents", "MacOS", APP_NAME);
  try { await fs.access(executable); } catch { console.error(`no app at ${bundle}`); return EXIT.usage; }
  const binaryHash = await sha256(executable);
  if (option("--expect-sha256") && option("--expect-sha256").toLowerCase() !== binaryHash) { console.error("the app under test is not the delivered file (sha256 differs)"); return EXIT.usage; }
  const permissions = await createPeekaboo({ app: APP_NAME }).permissions();
  if (!permissions.granted) { console.error(`Peekaboo lacks permission: ${permissions.missing.join(", ") || "unknown"}. Grant Screen Recording and Accessibility to the host that runs this.`); return EXIT.permission; }

  const stamp = new Date().toISOString().replaceAll(":", "-");
  const evidence = path.resolve(option("--evidence") ?? path.join(os.homedir(), "Desktop", "super-ontology-acceptance", `run-${stamp}`, "evidence"));
  await fs.mkdir(evidence, { recursive: true });
  const profileRoot = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-t2-")));
  const fixtures = await createFixtures(profileRoot);
  fixtures.lite = await createLiteRuntimeStub(profileRoot);
  fixtures.second = path.join(profileRoot, "Second docs");
  await fs.mkdir(fixtures.second, { recursive: true });
  await fs.writeFile(path.join(fixtures.second, "extra.md"), "# Extra\n\nA second folder.\n");
  const application = { executable, bundle, sha256: binaryHash };
  const results = [];
  for (const scenario of scenarios) {
    for (const locale of scenario.locales.filter((entry) => locales.includes(entry))) {
      let run = null;
      for (let attempt = 1; attempt <= 2 && !run?.ok; attempt += 1) {
        run = await withLimit(attemptScenario({ scenario, locale, catalog: catalogs[locale], assertions, application, evidence, profileRoot, fixtures, options: { record: !flag("--no-record"), engineBin: option("--engine-bin") } }), ATTEMPT_MS);
        run.attempts = attempt;
        console.log(`${run.ok ? "PASS" : "FAIL"} ${scenario.id} ${locale} attempt ${attempt}${run.error ? ` - ${String(run.error).split("\n")[0]}` : ""}`);
      }
      results.push(run);
    }
  }
  await fs.writeFile(path.join(evidence, "results.json"), JSON.stringify({ app: { bundle, sha256: binaryHash }, results }, null, 2));
  await fs.rm(profileRoot, { recursive: true, force: true });
  const failed = results.filter((entry) => !entry.ok);
  console.log(`\n${results.length - failed.length}/${results.length} passed; evidence in ${evidence}`);
  return failed.length > 0 ? EXIT.failed : EXIT.ok;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) process.exit(await main());
