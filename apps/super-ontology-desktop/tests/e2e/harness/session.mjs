// The T1 harness (volume 25 section 6): the built app, started the way a person starts it, against a real `oo serve`, driven only by
// test ids (TEST-IDS.md) over the Chrome DevTools Protocol. A scenario is `runScenario(options, async (s) => { ... })`; this file
// owns everything around it: a private user-data folder, the engine, the app process, evidence, and a clean exit.
//
//   engine "real" (default): the app starts `oo serve --runtime embedded` from SO_T1_OO_BIN (or <repo>/target/debug/oo): embedded
//                            PostgreSQL, no Docker, no container. Markdown fixtures need no parser.
//   engine "fake":           the app starts tests/support/fake-oo.mjs instead (no database); `s.fakeDocs(...)` sets what it answers.
//
// Environment: SO_T1_PACKAGED_APP (path of a built `Super Ontology.app`: that executable is launched instead of electron + the source tree, with the oo it bundles;
//               the native-dialog queue does not exist there, so steps that answer a dialog are skipped: `session.packaged`), SO_T1_OO_BIN (absolute path of an `oo` built with docs, with `oo-docs-engine` next to it), SO_T1_ENGINE,
// SO_APP_ROOT (the app directory to launch; default: this one), SO_T1_KEEP=1 (keep the temporary folder).
import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import fs from "node:fs/promises";
import { createRequire } from "node:module";
import { request as httpRequest } from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { connectToPage, readDevToolsPort } from "./cdp.mjs";
import { createFixtures, createLiteRuntimeStub } from "./fixtures.mjs";
import { createFakeToolbox } from "../../support/fake-engine-tools.mjs";

const execute = promisify(execFile);
const harnessDirectory = path.dirname(fileURLToPath(import.meta.url));
const defaultAppRoot = path.resolve(harnessDirectory, "..", "..", "..");
const repoRoot = path.resolve(defaultAppRoot, "..", "..");
export const wait = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });
/** A scenario that cannot run here (no `oo` binary) exits with this code: not a pass, not a failure of the app. */
export const EXIT_SKIPPED = 77;

export const selector = (id) => `[data-testid="${id}"]`;

async function firstExisting(candidates) {
  for (const candidate of candidates.filter(Boolean)) { try { await fs.access(candidate, fs.constants.X_OK); return candidate; } catch { /* next */ } }
  return null;
}

/**
 * @param {{ name: string, title?: string, engine?: "real" | "fake", timeoutMs?: number, language?: string, settings?: object,
 *   window?: { width: number, height: number }, firstRun?: boolean }} options
 * @param {(session: object) => Promise<void>} scenario
 */
export async function runScenario(options, scenario) {
  const { name, title = name, timeoutMs = 280_000, language = "en", settings = {}, window: size = { width: 1280, height: 820 }, firstRun = false } = options;
  const engineKind = process.env.SO_T1_ENGINE ?? options.engine ?? "real";
  const appRoot = process.env.SO_APP_ROOT ?? defaultAppRoot;
  const packagedApp = process.env.SO_T1_PACKAGED_APP ? path.join(process.env.SO_T1_PACKAGED_APP, "Contents", "MacOS", "Super Ontology") : null;
  const stamp = new Date().toISOString().replaceAll(":", "-");
  const reportDirectory = path.join(process.env.SO_T1_REPORTS ?? path.join(defaultAppRoot, "artifacts", "t1"), stamp, name);
  await fs.mkdir(reportDirectory, { recursive: true });
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), `so-t1-${name}-`)));
  const userData = path.join(root, "user-data");
  const stateDirectory = path.join(userData, "engine", "state");
  await fs.mkdir(userData, { recursive: true, mode: 0o700 });
  const fixtures = await createFixtures(root);
  const queueFile = path.join(root, "dialog-queue.json");
  await fs.writeFile(queueFile, "[]");

  const report = { name, title, engine: engineKind, ok: false, skipped: null, started: new Date().toISOString(), checks: [], notes: [], timings: {}, error: null };
  const state = { child: null, page: null, toolbox: null, ooBinary: null };
  const session = { packaged: Boolean(packagedApp), report, reportDirectory, root, userData, stateDirectory, fixtures, queueFile, engine: engineKind };
  const log = (line) => console.log(`[t1:${name}] ${line}`);

  // ---- engine arrangement
  const appSettings = { language, ...settings };
  const environment = { ...process.env };
  delete environment.ELECTRON_RUN_AS_NODE;
  if (engineKind === "real") {
    state.ooBinary = await firstExisting([process.env.SO_T1_OO_BIN, path.join(repoRoot, "target", "debug", "oo"), path.join(repoRoot, "target", "release", "oo")]);
    if (!state.ooBinary) {
      report.skipped = "no oo binary: set SO_T1_OO_BIN to an absolute path of an oo built with docs";
      console.error(report.skipped);
      await fs.rm(root, { recursive: true, force: true });
      process.exit(EXIT_SKIPPED);
    }
    // The embedded runtime is the engine's own (`oo serve --runtime embedded` brings its PostgreSQL); the stub folder only lets the
    // app's preflight see "an embedded runtime is present", like the folder the DMG ships.
    if (packagedApp) {
      // The packaged app finds its own bundled `oo` and embedded runtime: nothing is arranged for it.
      Object.assign(appSettings, { runtime: "lite" });
    } else {
      const lite = await createLiteRuntimeStub(root);
      environment.SUPER_ONTOLOGY_LITE_DIR = lite;
      Object.assign(appSettings, { ooExecutable: state.ooBinary, runtime: "lite" });
    }
  } else {
    state.toolbox = await createFakeToolbox();
    Object.assign(appSettings, { ooExecutable: await state.toolbox.install("oo", "fake-oo"), runtime: "auto" });
  }
  await fs.writeFile(path.join(userData, "settings.json"), JSON.stringify(appSettings), { mode: 0o600 });

  // ---- helpers over the page
  const evaluate = (expression) => state.page.evaluate(expression);
  const q = (id) => JSON.stringify(selector(id));
  Object.assign(session, {
    wait, evaluate, selector,
    log,
    note: (text) => { report.notes.push(text); log(text); },
    check(label, ok, detail = "") {
      report.checks.push({ check: label, ok: Boolean(ok), detail });
      log(`${ok ? "PASS" : "FAIL"} ${label}${detail ? ` - ${detail}` : ""}`);
      assert.ok(ok, `${label} ${detail}`);
    },
    async until(expression, description = expression, { timeoutMs: limit = 30_000, interval = 100 } = {}) {
      const started = Date.now();
      while (Date.now() - started < limit) {
        if (await evaluate(expression).catch(() => false)) return;
        await wait(interval);
      }
      throw new Error(`Timed out waiting for ${description}`);
    },
    /** Waits for an element by test id (and optional extra attribute filters, e.g. `{ "data-name": "Notes" }`). */
    async waitFor(id, attributes = {}, options = {}) {
      const filter = Object.entries(attributes).map(([key, value]) => `[${key}=${JSON.stringify(String(value))}]`).join("");
      const css = `${selector(id)}${filter}`;
      await session.until(`!!document.querySelector(${JSON.stringify(css)})`, `${css}`, options);
      return css;
    },
    /** Waits for an entity row whose name matches the pattern (the engine names the project "Project Alpha" or "alpha" depending on how its file arrived) and returns that name. */
    async waitForEntity(pattern, options = {}) {
      const source = pattern.source;
      await session.until(`[...document.querySelectorAll('[data-testid="entity-row"]')].some((row) => new RegExp(${JSON.stringify(source)}, "iu").test(row.getAttribute('data-name') ?? ''))`, `an entity matching ${source}`, options);
      return evaluate(`[...document.querySelectorAll('[data-testid="entity-row"]')].map((row) => row.getAttribute('data-name')).find((name) => new RegExp(${JSON.stringify(source)}, "iu").test(name ?? ''))`);
    },
    exists: (id) => evaluate(`!!document.querySelector(${q(id)})`),
    count: (id) => evaluate(`document.querySelectorAll(${q(id)}).length`),
    text: (id) => evaluate(`document.querySelector(${q(id)})?.innerText ?? null`),
    attr: (id, attribute) => evaluate(`document.querySelector(${q(id)})?.getAttribute(${JSON.stringify(attribute)}) ?? null`),
    attrs: (id, attribute) => evaluate(`[...document.querySelectorAll(${q(id)})].map((e) => e.getAttribute(${JSON.stringify(attribute)}))`),
    async point(css) {
      await session.until(`!!document.querySelector(${JSON.stringify(css)})`, `element ${css}`);
      await evaluate(`document.querySelector(${JSON.stringify(css)}).scrollIntoView({ block: "center", inline: "center" })`);
      return evaluate(`(() => { const r = document.querySelector(${JSON.stringify(css)}).getBoundingClientRect(); return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) }; })()`);
    },
    /** A real mouse click on the first element matching a test id (plus optional attribute filters). */
    async click(id, attributes = {}) {
      const css = await session.waitFor(id, attributes);
      const at = await session.point(css);
      await state.page.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...at });
      await state.page.send("Input.dispatchMouseEvent", { type: "mousePressed", ...at, button: "left", clickCount: 1 });
      await state.page.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...at, button: "left", clickCount: 1 });
    },
    /** Focuses the field, replaces its content and types the text. */
    async type(id, text) {
      await session.click(id);
      await evaluate(`(() => { const e = document.querySelector(${q(id)}); e.focus(); e.select?.(); })()`);
      await state.page.send("Input.insertText", { text });
    },
    async press(key, code = key) {
      await state.page.send("Input.dispatchKeyEvent", { type: "keyDown", key, code });
      await state.page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code });
    },
    /** Drops real files (absolute paths) on an element, the way a Finder drag does. */
    async dropFiles(id, paths) {
      const at = await session.point(await session.waitFor(id));
      const data = { items: [], files: paths, dragOperationsMask: 1 };
      for (const type of ["dragEnter", "dragOver", "drop"]) await state.page.send("Input.dispatchDragEvent", { type, ...at, data });
    },
    /** The next native open dialog answers with these absolute paths (one call = one dialog). */
    async answerDialog(...paths) {
      const queue = JSON.parse(await fs.readFile(queueFile, "utf8"));
      queue.push(paths);
      await fs.writeFile(queueFile, JSON.stringify(queue));
    },
    async shot(label) {
      await wait(200);
      const { data } = await state.page.send("Page.captureScreenshot", { format: "png" });
      await fs.writeFile(path.join(reportDirectory, `${label}.png`), Buffer.from(data, "base64"));
    },
    consoleErrors: () => state.page.console().filter((entry) => entry.type === "error"),
    /** What the fake engine answers (engine "fake"): { drives, files, dirs, ... }, read on every request. */
    async fakeDocs(documents) {
      await fs.mkdir(stateDirectory, { recursive: true });
      await fs.writeFile(path.join(stateDirectory, "fake-docs.json"), JSON.stringify(documents));
    },
    /** A call to the running engine's control API with the token the engine wrote, for seeding and for checking what the app did. */
    async engineApi(route, { method = "GET", body, headers = {} } = {}) {
      const info = JSON.parse(await fs.readFile(path.join(stateDirectory, "serve.json"), "utf8"));
      const token = (await fs.readFile(info.token_file ?? path.join(stateDirectory, "serve.token"), "utf8")).trim();
      const payload = body === undefined ? null : JSON.stringify(body);
      return new Promise((resolve, reject) => {
        const request = httpRequest({ host: "127.0.0.1", port: info.port, path: route, method, timeout: 30_000, headers: { authorization: `Bearer ${token}`, ...(payload ? { "content-type": "application/json", "content-length": Buffer.byteLength(payload) } : {}), ...headers } }, (response) => {
          const chunks = [];
          response.on("data", (chunk) => chunks.push(chunk));
          response.on("end", () => { const text = Buffer.concat(chunks).toString("utf8"); try { resolve({ status: response.statusCode, body: text ? JSON.parse(text) : null }); } catch { resolve({ status: response.statusCode, body: text }); } });
        });
        request.on("error", reject);
        request.on("timeout", () => request.destroy(new Error("engine api timeout")));
        if (payload) request.write(payload);
        request.end();
      });
    },
    /** Resolves once `serve.json` names a live engine the app started. */
    async engineUp(limit = 120_000) {
      const started = Date.now();
      while (Date.now() - started < limit) {
        try { const { status, body } = await session.engineApi("/v1/docs/health"); if (status === 200 && body?.status) return body; } catch { /* not yet */ }
        await wait(500);
      }
      throw new Error("the engine did not come up");
    },
    /** Registers a folder as a source through the control API and waits until its files have settled (nothing processing). */
    async seedFolder({ name: driveName = "Sample docs", folder = fixtures.folder, expectFiles = fixtures.count, limit = 180_000 } = {}) {
      const created = await session.engineApi("/v1/docs/drives", { method: "POST", headers: { "idempotency-key": `t1-${name}-${driveName}`.replace(/[^A-Za-z0-9_-]/gu, "-").padEnd(16, "0") }, body: { name: driveName, backend: "fs", config: { root: folder } } });
      assert.ok(created.status === 201 || created.status === 200, `drive create answered ${created.status} ${JSON.stringify(created.body).slice(0, 200)}`);
      const driveId = created.body.id;
      await session.engineApi(`/v1/docs/drives/${driveId}:sync`, { method: "POST", body: {}, headers: { "idempotency-key": `t1-sync-${driveId}`.padEnd(16, "0") } });
      const settled = new Set(["published", "failed", "skipped", "unsupported"]);
      const started = Date.now();
      while (Date.now() - started < limit) {
        const { body } = await session.engineApi(`/v1/docs/drives/${driveId}/files?limit=200`);
        const states = (body?.items ?? []).map((file) => file.state);
        if (states.length >= expectFiles && states.every((value) => settled.has(value))) return { driveId, states };
        await wait(1000);
      }
      throw new Error(`the source ${driveName} did not settle`);
    },
  });

  /** The state most area scenarios start from: the app's engine is up and one source with the fixture files is registered and settled. */
  session.openSeeded = async (screen = "home") => {
    await session.engineUp();
    const seeded = await session.seedFolder();
    // The engine needs a moment to extract after the files settle; the area scenarios wait for what they read.
    await evaluate("location.reload()");
    await session.waitFor("app-root", {}, { timeoutMs: 60_000 });
    await session.click(`nav-${screen}`);
    return seeded;
  };

  // ---- app process
  async function launch() {
    const out = await fs.open(path.join(reportDirectory, "app.log"), "w");
    if (packagedApp) {
      state.child = spawn(packagedApp, ["--remote-debugging-port=0", `--user-data-dir=${userData}`, "--disable-renderer-backgrounding"], { env: { ...environment, SUPER_ONTOLOGY_USER_DATA: userData }, stdio: ["ignore", out.fd, out.fd] });
    } else {
      const electronPath = createRequire(path.join(appRoot, "package.json"))("electron");
      state.child = spawn(electronPath, [path.join(harnessDirectory, "electron-main.mjs"), "--remote-debugging-port=0", `--user-data-dir=${userData}`, "--disable-renderer-backgrounding"], {
        env: { ...environment, SO_APP_ROOT: appRoot, SUPER_ONTOLOGY_USER_DATA: userData, SO_T1_DIALOG_QUEUE: queueFile },
        stdio: ["ignore", out.fd, out.fd],
      });
    }
    state.exited = new Promise((resolve) => { state.child.once("exit", resolve); });
    const port = await readDevToolsPort(userData);
    state.page = await connectToPage(port, { urlPattern: /^(app|http):/u });
    await state.page.send("Page.enable");
    // Only the first-run scenario starts on the welcome overlay; every other scenario starts like a returning user (the overlay would sit on top of the navigation).
    if (!firstRun) {
      await state.page.send("Page.addScriptToEvaluateOnNewDocument", { source: "try { localStorage.setItem('so.firstRun.done', '1'); } catch {}" });
      await state.page.send("Page.reload");
    }
    await state.page.send("Emulation.setDeviceMetricsOverride", { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: false }).catch(() => {});
    await session.until("document.readyState === 'complete'", "page load", { timeoutMs: 30_000 });
  }
  session.launch = launch;
  /** Quits the app the way a person does (the engine it owns is stopped by the app) and starts it again with the same user data. */
  session.relaunch = async () => { await quit(); await launch(); };

  async function quit() {
    if (!state.child || state.child.exitCode !== null) return;
    try { await Promise.race([state.page?.send("Browser.close"), wait(5_000)]); } catch { /* the connection closes with the app */ }
    const gone = await Promise.race([state.exited.then(() => true), wait(20_000).then(() => false)]);
    if (!gone) { state.child.kill("SIGKILL"); await state.exited; }
    state.page = null;
  }

  async function stopEngine() {
    try {
      const info = JSON.parse(await fs.readFile(path.join(stateDirectory, "serve.json"), "utf8"));
      process.kill(info.pid, "SIGTERM");
      for (let attempt = 0; attempt < 100; attempt += 1) { try { process.kill(info.pid, 0); } catch { return; } await wait(100); }
      process.kill(info.pid, "SIGKILL");
    } catch { /* no engine left */ }
    if (state.ooBinary) await execute(state.ooBinary, ["serve", "--state-dir", stateDirectory, "--stop"], { timeout: 20_000 }).catch(() => {});
  }

  async function finish() {
    clearTimeout(guard);
    const started = Date.now();
    await quit().catch(() => {});
    await stopEngine();
    report.timings.shutdownMs = Date.now() - started;
    await fs.writeFile(path.join(reportDirectory, "report.json"), JSON.stringify(report, null, 2));
    const lines = [`# ${title} (T1, ${engineKind} engine)`, "", `Result: ${report.ok ? "PASS" : "FAIL"}`, "", "| check | result | detail |", "|---|---|---|",
      ...report.checks.map((entry) => `| ${entry.check} | ${entry.ok ? "pass" : "FAIL"} | ${String(entry.detail).replaceAll("|", "/")} |`), "", report.error ? `Error:\n${report.error}` : ""];
    await fs.writeFile(path.join(reportDirectory, "report.md"), `${lines.join("\n")}\n`);
    log(`report ${reportDirectory}/report.md`);
    if (process.env.SO_T1_KEEP !== "1") { await state.toolbox?.dispose(); await fs.rm(root, { recursive: true, force: true }); }
    process.exit(report.ok ? 0 : 1);
  }

  const guard = setTimeout(() => { console.error(`${name} timed out after ${timeoutMs} ms`); report.error = "timeout"; void finish(); }, timeoutMs);
  try {
    await launch();
    await scenario(session);
    report.ok = true;
  } catch (error) {
    report.error = error instanceof Error ? error.stack : String(error);
    console.error(report.error);
    try {
      await session.shot("failure");
      await fs.writeFile(path.join(reportDirectory, "failure.txt"), String(await evaluate("document.body.innerText")));
      await fs.writeFile(path.join(reportDirectory, "console.json"), JSON.stringify(state.page.console(), null, 2));
    } catch { /* the page may be gone */ }
    try { await fs.copyFile(path.join(userData, "engine", "logs", "super-ontology-serve.log"), path.join(reportDirectory, "serve.log")); } catch { /* no log */ }
  }
  await finish();
}
