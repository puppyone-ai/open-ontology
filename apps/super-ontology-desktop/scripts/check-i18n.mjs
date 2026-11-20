#!/usr/bin/env node
// `check:i18n`: catalogs complete, ICU valid, placeholders identical, no English left, no unknown or unused ids, no hard-coded UI text,
// numbers and dates formatted by the localization layer, short strings within the length budget. Native (main process) catalogs included.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runChecks } from "./i18n/core.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const { problems, stats } = runChecks({ root });
if (problems.length > 0) { console.error(`${problems.join("\n")}\n\n${problems.length} i18n problem(s).`); process.exit(1); }
console.log(`i18n OK: ${stats.renderer} renderer and ${stats.native} native messages per locale, ${stats.short} short strings within budget.`);
