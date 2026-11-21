#!/usr/bin/env node
// Reads what the owner sees under Ontology > atlas > Not connected (list view) from a running packaged app (started by run_packaged.mjs --hold):
// every member with the reason the app gives. Writes unlinked-ui-raw.json, unlinked-ui.json ({project-relative path: reason kind}) and a screenshot.
//   node scrape_unlinked.mjs --profile <HOME>/profile --run packaged-run.json --out <dir>
import fs from "node:fs/promises";
import path from "node:path";

const args = process.argv.slice(2);
const opt = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const sleep = (ms) => new Promise((r) => { setTimeout(r, ms); });
const here = path.dirname(new URL(import.meta.url).pathname);
const { connectToPage, readDevToolsPort } = await import(path.resolve(here, "../../../apps/super-ontology-desktop/tests/e2e/harness/cdp.mjs"));
const cdp = await connectToPage(await readDevToolsPort(opt("--profile")), { urlPattern: /^app:/u });
const click = (id) => cdp.evaluate(`(() => { const e = document.querySelector('[data-testid="${id}"]'); if (!e) return false; e.click(); return true; })()`);
for (const id of ["nav-ontology", "ontology-tab-atlas", "atlas-unlinked-filter", "atlas-view-list"]) {
  const ok = await click(id);
  console.error("click", id, ok);
  await sleep(id === "atlas-view-list" ? 2500 : 1500);
}
// The list shows 100 rows per group and keeps the rest behind a "Show more" button (AtlasList PAGE); the Not connected group holds the entity
// islands as well as the files, so the files are often past the first page. Press it until every row is in the document.
for (let i = 0; i < 200; i += 1) {
  const pressed = await cdp.evaluate(`(() => { const b = [...document.querySelectorAll('[data-testid="atlas-list"] .so-atlas-list-members > li > button:not([data-testid])')].find((e) => /\\d/u.test(e.innerText)); if (!b) return false; b.click(); return true; })()`);
  if (!pressed) break;
  await sleep(300);
}
const raw = JSON.parse(await cdp.evaluate(`JSON.stringify([...document.querySelectorAll('[data-testid="atlas-list-member"]')].map((e) => ({ name: e.dataset.name, kind: e.querySelector('[data-testid="atlas-reason"]')?.dataset.reason ?? null, text: (e.querySelector('[data-testid="atlas-reason"]')?.innerText ?? '').trim() })))`));
const run = JSON.parse(await fs.readFile(opt("--run"), "utf8"));
const byBase = {};
for (const key of Object.keys(run.files)) byBase[key.split("/").pop()] = key.replace(/^project\//u, "");
const files = {};
for (const m of raw) if (byBase[m.name] && m.kind) files[byBase[m.name]] = m.kind;
await fs.mkdir(opt("--out"), { recursive: true });
await fs.writeFile(path.join(opt("--out"), "unlinked-ui-raw.json"), JSON.stringify(raw, null, 1));
await fs.writeFile(path.join(opt("--out"), "unlinked-ui.json"), JSON.stringify(files, null, 1));
const shot = await cdp.send("Page.captureScreenshot", { format: "png" }).catch(() => null);
if (shot?.data) await fs.writeFile(path.join(opt("--out"), "unlinked-ui.png"), Buffer.from(shot.data, "base64"));
console.error(`members ${raw.length}, files ${Object.keys(files).length}`);
cdp.close();
