#!/usr/bin/env node
// Review screenshots of EVERY screen and state, taken from the real Electron window (preload, IPC, bundled-style `oo serve`),
// at 1x and 2x. One session: empty app -> first run with a real upload -> every screen -> error and edge states.
//   node scripts/electron-shots.mjs <out-dir> [--oo <path to oo>] [--corpus <dir of files>]
import { _electron } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : fallback; };
const outDir = args[0] && !args[0].startsWith("--") ? path.resolve(args[0]) : null;
if (!outDir) { console.error("usage: electron-shots.mjs <out-dir> [--oo <path>] [--corpus <dir>]"); process.exit(2); }
const oo = option("--oo", process.env.SUPER_ONTOLOGY_OO_BINARY);
const corpus = path.resolve(option("--corpus", path.join(os.tmpdir(), "so-corpus")));
fs.mkdirSync(outDir, { recursive: true });
const userData = fs.mkdtempSync(path.join(os.tmpdir(), "so-shots-"));
fs.writeFileSync(path.join(userData, "settings.json"), JSON.stringify({ language: "en", runtime: "auto" }));

const app = await _electron.launch({
  args: ["."], cwd: root,
  env: { ...process.env, SUPER_ONTOLOGY_USER_DATA: userData, ...(oo ? { SUPER_ONTOLOGY_OO_BINARY: oo } : {}) },
});
const page = await app.firstWindow();
page.on("pageerror", (error) => console.log("[pageerror]", error.message));
const cdp = await page.context().newCDPSession(page);
const size = await page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
const shots = [];
const pause = (ms) => page.waitForTimeout(ms);
const app$ = (selector) => page.locator(selector).first();
const textButton = (text) => page.getByRole("button", { name: text, exact: false }).first();

/** The same state at 1x and 2x: the window is the real one, only the device pixel ratio is switched. */
async function shot(name) {
  await pause(350);
  for (const scale of [1, 2]) {
    await cdp.send("Emulation.setDeviceMetricsOverride", { ...size, deviceScaleFactor: scale, mobile: false });
    await pause(120);
    await page.screenshot({ path: path.join(outDir, `${name}@${scale}x.png`) });
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride");
  shots.push(name);
  console.log("shot", name);
}
const go = async (section) => { await app$(`[data-testid=nav-${section}]`).click(); await pause(500); };
const waitFor = (fn, arg, timeout = 120_000) => page.waitForFunction(fn, arg, { timeout });

try {
  await page.waitForSelector("[data-testid=app-root]", { timeout: 30_000 });
  // First run covers the empty app: skip it, shoot the empty screens, then bring it back from Settings.
  await waitFor(() => document.querySelector("[data-testid=first-run]") !== null);
  await shot("first-run-welcome");
  await waitFor(() => /Engine ready|Healthy|Ready/iu.test(document.querySelector("[data-testid=first-engine]")?.textContent ?? ""));
  await shot("first-run-welcome-ready");
  await textButton("Skip").click();
  await pause(600);
  await shot("home-empty");
  for (const section of ["sources", "ontology", "agents"]) { await go(section); await shot(`${section}-empty`); }
  await go("agents");
  await shot("agents-connect-no-source");

  // Real first run: My files -> upload -> reading -> ontology -> agent.
  await go("settings");
  await textButton("first run").click().catch(() => {});
  await pause(500);
  if (!(await page.locator("[data-testid=first-run]").count())) { await page.evaluate(() => { window.localStorage.removeItem("so.firstRun.done"); }); await page.reload(); await page.waitForSelector("[data-testid=app-root]"); await pause(1500); }
  await waitFor(() => /ready/iu.test(document.querySelector("[data-testid=first-engine]")?.textContent ?? ""));
  await app$("[data-testid=first-start]").click();
  await pause(400);
  await shot("first-run-add");
  const files = fs.readdirSync(corpus).filter((name) => /\.(md|txt)$/u.test(name)).map((name) => path.join(corpus, name));
  await page.locator("input[type=file]").first().setInputFiles(files);
  await pause(600);
  await shot("first-run-reading");
  await waitFor(() => /see|ontology|found/iu.test(document.querySelector("[data-testid=first-run] h1")?.textContent ?? "") && !/Reading/iu.test(document.querySelector("[data-testid=first-run] h1")?.textContent ?? ""), undefined, 240_000);
  await pause(800);
  await shot("first-run-see");
  await textButton("Continue").click();
  await pause(600);
  await shot("first-run-agent-step1");
  await textButton("Create").click();
  await pause(1500);
  await shot("first-run-agent-step2");
  await textButton("Done").click();
  await pause(800);

  // Every screen with data.
  await go("home"); await shot("home");
  await go("sources"); await shot("sources-list");
  await textButton("Add source").click(); await pause(300); await shot("add-source-menu");
  await page.getByRole("menuitem").filter({ hasText: /cloud|S3|bucket/iu }).first().click().catch(async () => { await page.keyboard.press("Escape"); });
  await pause(400); await shot("add-cloud-dialog");
  await page.keyboard.press("Escape"); await pause(300);
  await page.locator("[data-testid=source-row]").first().click(); await pause(900); await shot("source-detail");
  await page.locator("tr[data-selectable]").first().click(); await pause(900); await shot("source-file-panel");
  await go("ontology"); await shot("ontology-entities");
  await page.locator("[data-testid=entity-list] button").first().click(); await pause(1200); await shot("ontology-entity");
  await go("ontology");
  for (const [tab, name] of [["Review", "ontology-review"], ["Conflicts", "ontology-conflicts"]]) {
    await page.getByRole("tab", { name: new RegExp(tab, "iu") }).first().click().catch(() => {}); await pause(700); await shot(name);
  }
  await go("agents"); await shot("agents-connect");
  await page.getByRole("tab", { name: /packs/iu }).first().click().catch(() => {}); await pause(700); await shot("agents-packs");
  await page.getByRole("tab", { name: /access|log/iu }).first().click().catch(() => {}); await pause(700); await shot("agents-access");
  await page.getByRole("tab", { name: /connect/iu }).first().click().catch(() => {}); await pause(500);
  await textButton("Create").click(); await pause(1500); await shot("agents-token-config");
  for (const tab of ["General", "Engine", "Model", "Privacy", "About"]) {
    await go("settings");
    await page.getByRole("tab", { name: new RegExp(tab, "iu") }).first().click().catch(() => {}); await pause(700); await shot(`settings-${tab.toLowerCase()}`);
  }
  // Interaction states on one control: hover / focus / pressed / disabled.
  await go("sources");
  const add = textButton("Add source");
  await add.hover(); await shot("state-button-hover");
  await page.keyboard.press("Tab"); await shot("state-focus-ring");
  await page.setViewportSize?.({ width: 960, height: 640 }).catch(() => {});
  await shot("narrow-960");
  // Engine stopped: the error / partial state of every screen is the same banner and pill.
  await app.evaluate(() => {});
  await page.evaluate(() => window.superOntology.invoke("engine-stop", {}));
  await pause(1500);
  await go("home"); await shot("home-engine-stopped");
  await go("sources"); await shot("sources-engine-stopped");
  await go("agents"); await shot("agents-engine-stopped");
} catch (error) {
  console.error("FAILED:", error.message);
  await page.screenshot({ path: path.join(outDir, "_failure.png") }).catch(() => {});
  process.exitCode = 1;
} finally {
  await app.close().catch(() => {});
  fs.rmSync(userData, { recursive: true, force: true });
}
console.log(`${shots.length} states x 2 scales in ${outDir}`);
