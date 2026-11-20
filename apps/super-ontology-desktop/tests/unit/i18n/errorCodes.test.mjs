import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { SO_ERROR_CODES } from "@so-shared/errors.mjs";
import { DOCS_ERROR_DETAIL_KEYS } from "@so-shared/docs-contract.generated.mjs";
import { APP_OWNED_MESSAGE_CODES, KNOWN_CLI_CODES, describeCliError } from "@so-model/cliApi";

const root = path.resolve(import.meta.dirname, "..", "..", "..");
const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];
const read = (locale, namespace) => JSON.parse(fs.readFileSync(path.join(root, "src/locales", locale, `${namespace}.json`), "utf8"));
const bridge = Object.fromEntries(LOCALES.map((locale) => [locale, read(locale, "super-drive")]));
const app = Object.fromEntries(LOCALES.map((locale) => [locale, read(locale, "app")]));
const bridgeKnows = (code) => `errors.${code}` in bridge.en;

/** Codes the main process puts in a CLI result itself (`code: "SO_…"` and the refusal helpers). */
function codesOfMainProcess() {
  const found = new Set();
  const walk = (dir) => { for (const entry of fs.readdirSync(dir, { withFileTypes: true })) { const full = path.join(dir, entry.name); if (entry.isDirectory()) walk(full); else if (entry.name.endsWith(".mjs")) { const text = fs.readFileSync(full, "utf8"); for (const match of text.matchAll(/code: "(SO_[A-Z_]+)"|caRefusal\("(SO_[A-Z_]+)"/gu)) found.add(match[1] ?? match[2]); } } };
  walk(path.join(root, "electron/main/super-ontology"));
  found.add("SO_MODEL_REQUIRED");
  return [...found].sort();
}

describe("error codes shown to people", () => {
  it("every bridge and engine code has its own sentence in every language", () => {
    const codes = [...SO_ERROR_CODES, ...Object.keys(DOCS_ERROR_DETAIL_KEYS)];
    expect(codes.length).toBeGreaterThan(100);
    for (const locale of LOCALES) {
      const missing = codes.filter((code) => !bridge[locale][`errors.${code}`]);
      expect(missing, locale).toEqual([]);
    }
  });

  it("every CLI code with a next step has it written in every language", () => {
    for (const locale of LOCALES) expect(KNOWN_CLI_CODES.filter((code) => !app[locale][`cli.next.${code}`]), locale).toEqual([]);
  });

  it("no code the main process writes falls to the generic sentence", () => {
    const codes = codesOfMainProcess();
    expect(codes.length).toBeGreaterThan(8);
    for (const code of codes) expect(describeCliError({ code, message: "x", hint: null }, bridgeKnows).sentence, code).not.toBe("generic");
  });
});

describe("describeCliError", () => {
  it("never shows an English sentence the app itself wrote", () => {
    for (const code of APP_OWNED_MESSAGE_CODES) expect(describeCliError({ code, message: "The command did not finish in time.", hint: "Retry" }, bridgeKnows).raw, code).toBe("");
  });

  it("keeps what oo printed for a code it owns, as a details line", () => {
    expect(describeCliError({ code: "BUILD_LOAD_FAILED", message: "no such table: orders", hint: "check grants" }, bridgeKnows)).toEqual({ sentence: "next", raw: "no such table: orders" });
    expect(describeCliError({ code: "SOMETHING_NEW", message: "boom", hint: "try this" }, bridgeKnows)).toEqual({ sentence: "generic", raw: "boom try this" });
  });

  it("uses the bridge's catalog for its own codes and the generic sentence for the rest", () => {
    expect(describeCliError({ code: "SO_ENGINE_NOT_RUNNING", message: "", hint: null }, bridgeKnows).sentence).toBe("next");
    expect(describeCliError({ code: "SO_ENGINE_UNREACHABLE", message: "", hint: null }, bridgeKnows).sentence).toBe("bridge");
    expect(describeCliError({ code: "X_UNKNOWN", message: "", hint: null }, bridgeKnows)).toEqual({ sentence: "generic", raw: "" });
  });
});
