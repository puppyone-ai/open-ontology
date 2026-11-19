#!/usr/bin/env node
// Fails when a literal `t("app.…")` key used in the renderer is missing from the English catalog, when a catalog key is unused
// by code (dead text), or when another language has a key English lacks.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const locales = path.join(root, "src/locales");
const used = new Set();
const dynamic = new Set();
(function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (/\.tsx?$/.test(entry.name)) {
      const text = fs.readFileSync(full, "utf8");
      for (const match of text.matchAll(/\bt\(\s*["'`](app\.[A-Za-z0-9_.${}-]+)["'`]/g)) {
        if (match[1].includes("${")) dynamic.add(match[1].split("${")[0]);
        else used.add(match[1].slice(4));
      }
      for (const match of text.matchAll(/["'`](app\.[A-Za-z0-9_.]+)["'`]/g)) used.add(match[1].slice(4));
    }
  }
})(path.join(root, "src/renderer"));

const read = (lang) => JSON.parse(fs.readFileSync(path.join(locales, lang, "app.json"), "utf8"));
const en = read("en");
const problems = [];
for (const key of used) if (!(key in en)) problems.push(`missing in en: app.${key}`);
for (const key of Object.keys(en)) {
  if (used.has(key) || [...dynamic].some((prefix) => `app.${key}`.startsWith(prefix))) continue;
  problems.push(`unused: app.${key}`);
}
for (const lang of fs.readdirSync(locales).filter((name) => fs.statSync(path.join(locales, name)).isDirectory())) {
  if (lang === "en" || !fs.existsSync(path.join(locales, lang, "app.json"))) continue;
  const other = read(lang);
  for (const key of Object.keys(other)) if (!(key in en)) problems.push(`${lang}: extra key app.${key}`);
  if (process.argv.includes("--complete")) for (const key of Object.keys(en)) if (!(key in other)) problems.push(`${lang}: missing app.${key}`);
}
if (problems.length > 0) { console.error(problems.join("\n")); process.exit(1); }
console.log(`i18n OK: ${used.size} keys used, ${Object.keys(en).length} in en.`);
