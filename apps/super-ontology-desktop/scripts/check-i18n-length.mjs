#!/usr/bin/env node
// Short strings (buttons, labels, tabs, menu items, badges: English up to 30 characters) must stay short in every language, or they
// truncate in the controls that hold them. Width counts a CJK character as two. Also fails on a missing key, a changed placeholder
// set, or an empty string. `--list` prints the longest ratios.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const locales = path.join(root, "src/locales");
const read = (lang) => JSON.parse(fs.readFileSync(path.join(locales, lang, "app.json"), "utf8"));
const en = read("en");
const width = (text) => [...text].reduce((sum, ch) => sum + (/[ᄀ-ᇿ⺀-鿿가-힯＀-￯]/u.test(ch) ? 2 : 1), 0);
const placeholders = (text) => [...text.matchAll(/\{\s*([A-Za-z0-9_]+)\s*[,}]/gu)].map((m) => m[1]).sort().join(",");
const problems = [];
const rows = [];
for (const lang of fs.readdirSync(locales).filter((name) => name !== "en" && fs.existsSync(path.join(locales, name, "app.json")))) {
  const data = read(lang);
  for (const [key, source] of Object.entries(en)) {
    const text = data[key];
    if (typeof text !== "string" || text.trim() === "") { problems.push(`${lang}: missing or empty app.${key}`); continue; }
    if (placeholders(text) !== placeholders(source)) problems.push(`${lang}: placeholders of app.${key} differ (${placeholders(source)} vs ${placeholders(text)})`);
    if (source.length <= 30) {
      const w = width(text);
      rows.push({ lang, key, ratio: w / source.length });
      if (w > source.length * 1.45 + 4) problems.push(`${lang}: app.${key} is ${w} wide for English ${source.length} ("${text}")`);
    }
  }
}
if (process.argv.includes("--list")) for (const row of rows.sort((a, b) => b.ratio - a.ratio).slice(0, 25)) console.log(row.ratio.toFixed(2), row.lang, row.key);
if (problems.length > 0) { console.error(problems.join("\n")); process.exit(1); }
console.log(`i18n length OK: ${rows.length} short strings checked.`);
