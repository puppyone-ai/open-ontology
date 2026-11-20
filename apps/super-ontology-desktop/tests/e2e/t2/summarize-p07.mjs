// Merges the timings.json of each P07 run (one per language) into one JSON and a short markdown table.
//   node tests/e2e/t2/summarize-p07.mjs <evidence-dir> [out-prefix]
import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { percentile } from "../harness/autosync-measure.mjs";

const cell = (value) => (typeof value === "number" && Number.isFinite(value) ? String(value) : "-");

/** One row per step and language: the slowest row, publish and settle times of its files, and how the freshness moved. */
export function summarize(timingsByLocale) {
  const rows = [];
  for (const [locale, timings] of Object.entries(timingsByLocale)) {
    for (const [step, measurement] of Object.entries(timings.steps ?? {})) {
      const files = measurement.files ?? [];
      const worst = (key) => (files.length === 0 || files.some((file) => file[key] === null || file[key] === undefined) ? null : Math.max(...files.map((file) => file[key])));
      rows.push({
        locale, step, files: files.length, rowMs: worst("rowMs"), publishedMs: worst("settledMs"), pageMs: worst("uiSettledMs"),
        p95PublishedMs: percentile(files.map((file) => file.settledMs), 95), upToDateMs: measurement.freshMs ?? null,
        freshness: (measurement.freshness?.api ?? []).map((entry) => entry.state).join(" > "), pageFreshness: (measurement.freshness?.ui ?? []).map((entry) => entry.state).join(" > "),
        viaPoll: (measurement.syncs ?? []).some((sync) => sync.trigger === "poll" && (sync.added ?? 0) + (sync.changed ?? 0) + (sync.removed ?? 0) + (sync.renamed ?? 0) > 0),
        marks: measurement.marks ?? null, notRun: measurement.notRun ?? null, ok: (measurement.problems ?? []).length === 0, problems: measurement.problems ?? [],
      });
    }
  }
  return rows;
}

export function table(rows) {
  const head = "| lang | step | files | row ms | published ms | page ms | up_to_date ms | freshness (engine) | via poll | ok |\n|---|---|---|---|---|---|---|---|---|---|";
  return `${head}\n${rows.map((row) => `| ${row.locale} | ${row.step} | ${row.files} | ${cell(row.rowMs)} | ${cell(row.publishedMs)} | ${cell(row.pageMs)} | ${cell(row.upToDateMs)} | ${row.freshness || "-"} | ${row.viaPoll ? "yes" : "no"} | ${row.notRun ? "not run" : row.ok ? "pass" : "FAIL"} |`).join("\n")}\n`;
}

async function main() {
  const evidence = process.argv[2];
  if (!evidence) { console.error("usage: summarize-p07.mjs <evidence-dir> [out-prefix]"); return 2; }
  const prefix = process.argv[3] ?? path.join(evidence, "p07-timings");
  const byLocale = {};
  for (const name of await fs.readdir(evidence)) {
    const match = name.match(/^P07-(.+)$/u);
    if (!match) continue;
    try { byLocale[match[1]] = JSON.parse(await fs.readFile(path.join(evidence, name, "timings.json"), "utf8")); } catch { /* not run */ }
  }
  const rows = summarize(byLocale);
  await fs.writeFile(`${prefix}.json`, JSON.stringify({ rows, raw: byLocale }, null, 2));
  await fs.writeFile(`${prefix}.md`, table(rows));
  console.log(table(rows));
  return rows.every((row) => row.ok || row.notRun) ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) process.exit(await main());
