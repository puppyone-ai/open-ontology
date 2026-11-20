#!/usr/bin/env node
// The vocabulary this app shares with the sibling desktop app (Cancel, Save, Retry, Copy, Settings, General, Close, ...): wherever the English
// text of a message here is exactly the English text of a message there, and the sibling has one translation of it, both apps use that
// translation. `src/locales/shared-vocabulary.json` is the recorded result, checked by a unit test so the terms cannot drift apart silently.
//
//   node scripts/i18n/shared-vocabulary.mjs <sibling>/locales            report where this app differs
//   node scripts/i18n/shared-vocabulary.mjs <sibling>/locales --write    refresh the recorded file (only entries that are identical)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const LOCALES = ["es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];
const recorded = path.join(root, "src/locales/shared-vocabulary.json");
const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));

export function siblingTranslations(siblingLocales) {
  const table = new Map(); // English text -> locale -> Set of translations
  const add = (english, locale, value) => { const byLocale = table.get(english) ?? new Map(); const set = byLocale.get(locale) ?? new Set(); set.add(value); byLocale.set(locale, set); table.set(english, byLocale); };
  const renderer = path.join(siblingLocales, "renderer");
  for (const file of fs.readdirSync(path.join(renderer, "en"))) {
    const english = read(path.join(renderer, "en", file));
    for (const locale of LOCALES) { const own = read(path.join(renderer, locale, file)); for (const key of Object.keys(english)) add(english[key], locale, own[key]); }
  }
  return table;
}

export function compare(siblingLocales) {
  const table = siblingTranslations(siblingLocales);
  const same = {}; const different = [];
  for (const namespace of ["app", "super-drive"]) {
    const english = read(path.join(root, "src/locales/en", `${namespace}.json`));
    for (const locale of LOCALES) {
      const own = read(path.join(root, "src/locales", locale, `${namespace}.json`));
      for (const key of Object.keys(english)) {
        const theirs = table.get(english[key])?.get(locale);
        if (!theirs || theirs.size !== 1) continue;
        const value = [...theirs][0];
        if (own[key] === value) ((same[`${namespace}.${key}`] ??= { en: english[key] })[locale] = value);
        else different.push({ id: `${namespace}.${key}`, locale, en: english[key], ours: own[key], theirs: value });
      }
    }
  }
  return { same, different };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const sibling = process.argv[2];
  if (!sibling) { console.error("usage: shared-vocabulary.mjs <sibling>/locales [--write]"); process.exit(2); }
  const { same, different } = compare(path.resolve(sibling));
  if (process.argv.includes("--write")) {
    const entries = Object.fromEntries(Object.entries(same).sort(([a], [b]) => a.localeCompare(b)));
    fs.writeFileSync(recorded, `${JSON.stringify({ comment: "Terms identical in both desktop apps, recorded from the sibling app's catalogs. Only exact English matches with a single translation are listed.", entries }, null, 2)}\n`);
    console.log(`recorded ${Object.keys(entries).length} messages`);
  } else {
    for (const row of different) console.log(`${row.id} [${row.locale}] "${row.en}": ours "${row.ours}" / sibling "${row.theirs}"`);
    console.log(`${Object.keys(same).length} messages identical, ${different.length} differ (each difference is a review item, not an error).`);
  }
}
