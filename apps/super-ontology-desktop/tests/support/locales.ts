import { readFileSync } from "node:fs";
import path from "node:path";

const appRoot = path.resolve(import.meta.dirname, "..", "..");
const localesDirectory = process.env.SO_LOCALES_DIR ?? path.join(appRoot, "src", "locales");

/** The flat message catalog of one locale and namespace ("super-drive", "app"). */
export const catalog = (locale: string, namespace = "super-drive"): Record<string, string> => (
  JSON.parse(readFileSync(path.join(localesDirectory, locale, `${namespace}.json`), "utf8")) as Record<string, string>
);
