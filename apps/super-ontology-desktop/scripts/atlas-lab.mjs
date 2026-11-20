#!/usr/bin/env node
/* global window, document */
// Measures and photographs the ontology atlas (D88) on synthetic corpora. It serves the renderer with vite, opens the lab pages and writes
// results next to the screenshots. Nothing here is part of a build.
//   node scripts/atlas-lab.mjs perf <out.json> [--sizes 1000,10000]      frame cost in the real Electron runtime
//   node scripts/atlas-lab.mjs shots <out-dir>                            screenshots (1x and 2x) and frame sequences of the lab app
//   node scripts/atlas-lab.mjs native <out-dir>                           the lab app in a real Electron window, captured by Peekaboo at the screen's own resolution
//                                                                        (run from a Terminal window that holds Screen Recording; no pixel ratio emulation)
import { execFile } from "node:child_process";
import { _electron, chromium } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command, out, ...rest] = process.argv.slice(2);
const option = (name, fallback) => { const i = rest.indexOf(name); return i >= 0 ? rest[i + 1] : fallback; };
if (!command || !out) { console.error("usage: atlas-lab.mjs perf|shots <out> [options]"); process.exit(2); }

const server = await createServer({ root: path.join(root, "src/renderer"), configFile: path.join(root, "vite.config.ts"), server: { port: 5290, strictPort: true, host: "127.0.0.1" }, logLevel: "error" });
await server.listen();
const base = "http://127.0.0.1:5290";
const done = async (code = 0) => { await server.close(); process.exit(code); };

function findChromium() {
  const cache = path.join(process.env.HOME ?? "", "Library/Caches/ms-playwright");
  const shells = fs.existsSync(cache) ? fs.readdirSync(cache).filter((name) => name.startsWith("chromium_headless_shell-")).sort().reverse() : [];
  for (const shell of shells) { const candidate = path.join(cache, shell, "chrome-headless-shell-mac-arm64", "chrome-headless-shell"); if (fs.existsSync(candidate)) return candidate; }
  return undefined;
}

try {
  if (command === "perf") {
    const sizes = option("--sizes", "1000,10000").split(",").map(Number);
    const results = [];
    for (const n of sizes) {
      const app = await _electron.launch({ args: [path.join(root, "scripts/atlas-lab.cjs")], env: { ...process.env, ATLAS_LAB_URL: `${base}/atlas-perf.html?n=${n}&auto=0`, ATLAS_LAB_SHOW: "1" } });
      const page = await app.firstWindow();
      page.on("pageerror", (error) => console.log("[pageerror]", error.message));
      await page.waitForFunction(() => typeof window.__perfRun === "function", null, { timeout: 60_000 });
      const result = await page.evaluate(() => window.__perfRun());
      const gpu = await app.evaluate(({ app: a }) => a.getGPUFeatureStatus());
      results.push({ n, ...result, canvas2d: gpu.canvas_oop_rasterization ?? gpu["2d_canvas"] ?? null });
      console.log(JSON.stringify({ n, idle: result.idle, panZoom: result.panZoom, zoomedIn: result.zoomedIn, setupMs: result.setupMs }));
      await app.close();
    }
    fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
    fs.writeFileSync(path.resolve(out), `${JSON.stringify({ measuredAt: new Date().toISOString(), runtime: "electron", results }, null, 2)}\n`);
  } else if (command === "native") {
    const outDir = path.resolve(out);
    fs.mkdirSync(outDir, { recursive: true });
    const grab = (app, name) => new Promise((resolve) => {
      execFile("peekaboo", ["see", "--pid", String(app.process().pid), "--mode", "window", "--no-elements", "--retina", "--no-remote", "--path", path.join(outDir, `${name}.png`)], { timeout: 60_000 }, (error, stdout, stderr) => { console.log(error ? `[peekaboo] ${String(stderr || stdout || error).slice(0, 200)}` : `native ${name}`); resolve(); });
    });
    for (const [name, params] of [["native-db-atlas", "shop=1&mode=source"], ["native-overview-type", ""], ["native-overview-source", "mode=source"]]) {
      const app = await _electron.launch({ args: [path.join(root, "scripts/atlas-lab.cjs")], env: { ...process.env, ATLAS_LAB_URL: `${base}/atlas-lab.html?${params}`, ATLAS_LAB_SHOW: "1" } });
      const page = await app.firstWindow();
      await page.waitForSelector("[data-testid=app-root]");
      await page.evaluate(() => document.fonts.ready);
      await page.locator("[data-testid=nav-ontology]").click();
      await page.locator("[data-testid=ontology-tab-atlas]").click();
      await page.locator("[data-testid=atlas-canvas]").waitFor();
      await page.waitForTimeout(3500);
      await page.mouse.move(5, 5);
      const ratio = await page.evaluate(() => window.devicePixelRatio);
      console.log("devicePixelRatio", ratio, await page.evaluate(() => { const c = document.querySelector("[data-testid=atlas-canvas]"); const r = c.getBoundingClientRect(); return JSON.stringify({ css: [r.width, r.height], backing: [c.width, c.height], left: r.left, top: r.top }); }));
      await grab(app, name);
      if (name === "native-db-atlas") {
        const box = await page.locator("[data-testid=atlas-canvas]").boundingBox();
        await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
        await page.waitForTimeout(700);
        await grab(app, "native-db-atlas-selected");
      }
      await app.close();
    }
  } else if (command === "shots") {
    const outDir = path.resolve(out);
    fs.mkdirSync(path.join(outDir, "frames"), { recursive: true });
    const browser = await chromium.launch({ executablePath: findChromium() });
    const run = async (scale) => {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: scale, colorScheme: "light", locale: "en-US" });
      const page = await context.newPage();
      page.on("pageerror", (error) => console.log("[pageerror]", error.message));
      page.on("console", (message) => { if (message.type() === "error") console.log("[console]", message.text()); });
      const shot = async (name, wait = 250) => { await page.waitForTimeout(wait); await page.screenshot({ path: path.join(outDir, `${name}@${scale}x.png`) }); console.log("shot", `${name}@${scale}x`); };
      const open = async (params = "", settle = 3500) => { await page.goto(`${base}/atlas-lab.html?${params}`); await page.waitForSelector("[data-testid=app-root]"); await page.evaluate(() => document.fonts.ready); await page.locator("[data-testid=nav-ontology]").click(); await page.locator("[data-testid=ontology-tab-atlas]").click(); await page.locator("[data-testid=atlas-canvas]").waitFor(); await page.waitForTimeout(settle); };
      const canvas = () => page.locator("[data-testid=atlas-canvas]");
      /** Screen position of a cell, in page pixels. */
      const where = async (label, kind = "group") => {
        const box = await canvas().boundingBox();
        const p = await page.evaluate(([l, k]) => { const c = window.__atlas; const n = c.scene.nodes.find((x) => x.label === l && x.kind === k); return n ? { x: n.x * c.camera.k + c.camera.x, y: n.y * c.camera.k + c.camera.y, r: n.r * c.camera.k } : null; }, [label, kind]);
        return p ? { x: box.x + p.x, y: box.y + p.y, r: p.r } : null;
      };
      /** A short run of frames of the atlas stage, for the recording. */
      const frames = async (name, ms, every = 90) => {
        if (scale !== 1) return;
        const dir = path.join(outDir, "frames", name);
        fs.mkdirSync(dir, { recursive: true });
        const clip = await page.locator(".so-atlas-stage").boundingBox();
        const start = Date.now();
        let i = 0;
        while (Date.now() - start < ms) { await page.screenshot({ path: path.join(dir, `${String(i).padStart(3, "0")}.png`), clip }); i += 1; await page.waitForTimeout(every); }
        console.log("frames", name, i);
      };

      await open();
      await shot("01-overview-type");

      // zoom in on a big group: labels appear when there is room
      const person = await where("Person");
      await page.mouse.move(person.x, person.y);
      await page.mouse.wheel(0, -2400);
      await page.waitForTimeout(500);
      await page.mouse.move(40, 40);
      await shot("02-zoomed-labels");
      const focus = await page.evaluate(() => { const c = window.__atlas; const g = c.scene.nodes.find((x) => x.label === "Person" && x.kind === "group"); const m = c.scene.nodes.find((x) => x.kind === "member" && x.group === g.idx && c.scene.neighbors(x.idx).size > 1); const b = document.querySelector("[data-testid=atlas-canvas]").getBoundingClientRect(); return { x: b.x + m.x * c.camera.k + c.camera.x, y: b.y + m.y * c.camera.k + c.camera.y }; });
      await page.mouse.move(focus.x, focus.y);
      await shot("03-focus");
      await page.locator("[data-testid=atlas-fit]").click();
      await page.waitForTimeout(500);

      // a group: click folds it into one cell with its count, the inspector says what it is
      const project = await where("Project");
      await page.mouse.click(project.x + project.r * 0.97, project.y);
      await page.waitForTimeout(900);
      await shot("04-group-folded-inspector");
      await page.mouse.click(project.x, project.y - 4);
      await page.waitForTimeout(900);

      await page.locator("[data-testid=atlas-search]").fill("Ember");
      await shot("05-search");
      await page.locator("[data-testid=atlas-search]").press("Enter");
      await shot("06-search-jump", 700);
      await page.locator("[data-testid=atlas-search]").fill("");
      await page.locator("[data-testid=atlas-fit]").click();

      await page.locator("[data-testid=atlas-view-list]").click();
      await shot("07-list-view");
      await page.locator("[data-testid=atlas-list-group]").first().click();
      await shot("08-list-expanded");
      await page.locator("[data-testid=atlas-view-graph]").click();
      await page.waitForTimeout(2500);

      // growth: replay the versions
      await page.locator("[data-testid=atlas-play]").click();
      for (const [name, ms] of [["a", 120], ["b", 240], ["c", 400], ["d", 700], ["e", 1500]]) await shot(`growth-mid-${name}`, ms);
      await frames("growth", 9000, 100);
      await shot("09-growth-done", 800);

      // live: a new version arrives with new entities, one entity less and one changed
      await page.evaluate(() => {
        const lab = window.__lab;
        const next = structuredClone(lab.data);
        const first = next.members.find((m) => m.ref.kind === "entity");
        next.members = next.members.filter((m) => m.id !== first.id);
        next.links = next.links.filter((l) => l.a !== first.id && l.b !== first.id);
        const bud = next.members.filter((m) => m.ref.kind === "entity").slice(0, 3);
        for (let i = 0; i < 18; i += 1) next.members.push({ ...bud[i % 3], id: `e:fresh-${i}`, label: `Fresh ${String(i).padStart(2, "0")}`, ref: { kind: "entity", key: `fresh-${i}` }, weight: 1, born: null });
        next.members.find((m) => m.ref.kind === "entity" && m.weight > 2).weight += 4;
        next.versions.push({ n: 99, asOf: "2026-12-01T00:00:00Z" });
        lab.data.members = next.members;
        lab.data.links = next.links;
        lab.data.versions = next.versions;
        lab.replace(next);
        lab.push([{ type: "ontology.version_published", at: new Date().toISOString(), data: { n: 99 } }]);
      });
      await shot("live-change-a", 90);
      await shot("live-change-b", 120);
      await frames("live-update", 5000, 100);
      await shot("10-live-updated", 400);
      await page.locator("[data-testid=atlas-mode-source]").click();
      await page.waitForTimeout(4000);
      await shot("02-overview-source");
      await page.locator("[data-testid=atlas-view-list]").click();
      await page.locator("[data-testid=atlas-list-group]").first().click();
      await shot("13-source-list");
      await context.close();
    };
    await run(1);
    await run(2);

    // reduced motion: the layout is static from the first frame
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2, colorScheme: "light", reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto(`${base}/atlas-lab.html`);
    await page.waitForSelector("[data-testid=app-root]");
    await page.locator("[data-testid=nav-ontology]").click();
    await page.locator("[data-testid=ontology-tab-atlas]").click();
    await page.locator("[data-testid=atlas-canvas]").waitFor();
    await page.waitForTimeout(800);
    await page.screenshot({ path: path.join(outDir, "11-reduced-motion@2x.png") });
    console.log("shot 11-reduced-motion@2x");
    await context.close();
    await browser.close();
  }
  await done(0);
} catch (error) {
  console.error(error);
  await done(1);
}
