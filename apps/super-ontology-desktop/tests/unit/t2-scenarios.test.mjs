import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { LOCALES, expandIds, loadCatalogs, loadScenarios, nameFor, validateScenario } from "../e2e/t2/run-packaged.mjs";

const assertionNames = new Set(fs.readdirSync(path.join(import.meta.dirname, "..", "e2e", "t2", "assertions")).map((name) => name.replace(/\.mjs$/u, "")));
const localesDirectory = process.env.SO_LOCALES_DIR ?? path.join(import.meta.dirname, "..", "..", "src", "locales");
const haveCatalogs = fs.existsSync(path.join(localesDirectory, "en", "app.json"));

describe("T2 scenarios", () => {
  it("are well formed: ids, steps, assertions, locales", async () => {
    const scenarios = await loadScenarios();
    expect(scenarios.map((scenario) => scenario.id)).toEqual(["P00", "P01", "P02", "P03", "P04", "P05", "P06"]);
    const catalogs = Object.fromEntries(LOCALES.map((locale) => [locale, {}]));
    for (const scenario of scenarios) expect(validateScenario(scenario, { catalogs, assertionNames, allowUnresolved: true }), scenario.id).toEqual([]);
  });
  it("reject test ids, unknown actions and names that are not message keys", () => {
    const base = { id: "P99", title: "t", locales: ["en"], closes: "x" };
    const check = (steps) => validateScenario({ ...base, steps }, { catalogs: { en: { "a.b": "B" } }, assertionNames, allowUnresolved: false });
    expect(check([{ do: "click", name: "plain words" }]).join()).toMatch(/not a message key/u);
    expect(check([{ do: "dance" }]).join()).toMatch(/unknown action/u);
    expect(check([{ do: "click", name: "app.a.b" }])).toEqual([]);
    expect(check([{ do: "click", name: "app.missing" }]).join()).toMatch(/no message/u);
    expect(check([{ do: "assert", assertion: "dom", args: { "data-testid": "x" } }]).join()).toMatch(/testid/u);
  });
  it("name an element by the catalog text up to its first placeholder", () => {
    expect(nameFor({ "nav.withCount": "{label}, {count} waiting", "sources.add": "Add source" }, "app.sources.add")).toBe("Add source");
    expect(expandIds("P01-P03,P05")).toEqual(["P01", "P02", "P03", "P05"]);
  });
  it.skipIf(!haveCatalogs)("every message key resolves in en and zh-Hans once the renderer catalogs exist", async () => {
    const catalogs = await loadCatalogs();
    for (const scenario of await loadScenarios()) expect(validateScenario(scenario, { catalogs, assertionNames }), scenario.id).toEqual([]);
  });
});
