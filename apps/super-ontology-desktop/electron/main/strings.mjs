import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Native (main process) strings: menus and dialog titles. They live in JSON catalogs (`src/shared/locales/native/<locale>.json`, shipped
 * with the app) so `check:i18n` covers them like the renderer's. Messages take simple `{name}` placeholders only: the main process has no ICU runtime.
 */
const CATALOG_DIRECTORY = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../src/shared/locales/native");

function loadCatalogs(directory) {
  const catalogs = {};
  for (const name of fs.readdirSync(directory).filter((file) => file.endsWith(".json")).sort()) {
    catalogs[name.slice(0, -".json".length)] = Object.freeze(JSON.parse(fs.readFileSync(path.join(directory, name), "utf8")));
  }
  if (!catalogs.en) throw new Error("The English native catalog is missing.");
  return Object.freeze(catalogs);
}

const CATALOG = loadCatalogs(CATALOG_DIRECTORY);

export const NATIVE_LANGUAGES = Object.freeze(Object.keys(CATALOG));

/** The language the native menus and dialogs use: the setting when it names one we have, otherwise the system's (Chinese maps to zh-Hans). */
export function resolveLanguage(setting, systemLocale = "en") {
  if (NATIVE_LANGUAGES.includes(setting)) return setting;
  const system = String(systemLocale);
  if (/^zh\b/iu.test(system)) return "zh-Hans";
  if (/^pt\b/iu.test(system)) return "pt-BR";
  const base = system.split(/[-_]/u)[0].toLowerCase();
  return NATIVE_LANGUAGES.includes(base) ? base : "en";
}

export function createTranslator(getLanguage, catalogs = CATALOG) {
  return (key, values = {}) => {
    const table = catalogs[getLanguage()] ?? catalogs.en;
    const text = table[key] ?? catalogs.en[key] ?? key;
    return text.replace(/\{(\w+)\}/gu, (_match, name) => String(values[name] ?? ""));
  };
}
