import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { EXPECTED_LOCALES, messageArguments, runChecks } from "../../../scripts/i18n/core.mjs";

const appRoot = path.resolve(import.meta.dirname, "..", "..", "..");
const temporary = [];
afterEach(() => { for (const dir of temporary.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });

/** A tiny but complete tree: 8 locales, one renderer message, one native message, one component that uses the first. */
function fixture(change = () => {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "so-i18n-"));
  temporary.push(root);
  const write = (relative, content) => { fs.mkdirSync(path.dirname(path.join(root, relative)), { recursive: true }); fs.writeFileSync(path.join(root, relative), typeof content === "string" ? content : `${JSON.stringify(content, null, 2)}\n`); };
  fs.mkdirSync(path.join(root, "src/locales"), { recursive: true });
  fs.copyFileSync(path.join(appRoot, "src/locales/manifest.json"), path.join(root, "src/locales/manifest.json"));
  const words = { en: "Hello {name}", es: "Hola {name}", "pt-BR": "Olá {name}", fr: "Bonjour {name}", de: "Hallo {name}", ja: "こんにちは {name}", ko: "안녕하세요 {name}", "zh-Hans": "你好 {name}" };
  const native = { en: "Quit", es: "Salir", "pt-BR": "Sair", fr: "Quitter", de: "Beenden", ja: "終了", ko: "종료", "zh-Hans": "退出" };
  for (const locale of EXPECTED_LOCALES) {
    write(`src/locales/${locale}/app.json`, { "greet.title": words[locale] });
    write(`src/locales/${locale}/super-drive.json`, {});
    write(`src/shared/locales/native/${locale}.json`, { "native.app.quit": native[locale] });
  }
  write("src/renderer/Greeting.tsx", 'export const Greeting = () => <p>{t("app.greet.title", { name: "x" })}</p>;\n');
  write("electron/main/menu.mjs", 'export const menu = (t) => [{ label: t("native.app.quit") }];\n');
  const api = { write, root, locale: (loc, ns, edit) => { const file = path.join(root, `src/locales/${loc}/${ns}.json`); const data = JSON.parse(fs.readFileSync(file, "utf8")); edit(data); fs.writeFileSync(file, JSON.stringify(data, null, 2)); } };
  change(api);
  return api;
}
const problemsOf = (api) => runChecks({ root: api.root }).problems;

describe("the localization checks", () => {
  it("pass on the real catalogs and sources", () => {
    expect(runChecks({ root: appRoot }).problems).toEqual([]);
  });

  it("pass on the baseline fixture", () => {
    expect(problemsOf(fixture())).toEqual([]);
  });

  it("fail on a key a locale lacks, and on a key English lacks", () => {
    expect(problemsOf(fixture((f) => f.locale("de", "app", (d) => { delete d["greet.title"]; })))).toContain("de/app.json: missing greet.title");
    expect(problemsOf(fixture((f) => f.locale("de", "app", (d) => { d["extra.key"] = "Extra"; })))).toContain("de/app.json: extra key extra.key");
  });

  it("fail on invalid ICU and on a changed placeholder set", () => {
    expect(problemsOf(fixture((f) => f.locale("fr", "app", (d) => { d["greet.title"] = "Bonjour {name"; }))).join("\n")).toMatch(/fr\/app\.json: greet\.title is not valid ICU/u);
    expect(problemsOf(fixture((f) => f.locale("fr", "app", (d) => { d["greet.title"] = "Bonjour {nom}"; }))).join("\n")).toMatch(/placeholders of greet\.title differ/u);
  });

  it("fail on English left in another locale, until the allowlist records it for that exact text", () => {
    const left = fixture((f) => f.locale("es", "app", (d) => { d["greet.title"] = "Hello {name}"; }));
    expect(problemsOf(left).join("\n")).toMatch(/es\/app\.json: greet\.title is still English/u);
    left.write("scripts/i18n/allowlist.json", { values: {}, identical: { es: { "app:greet.title": "Hello {name}" } } });
    expect(problemsOf(left)).toEqual([]);
    left.write("scripts/i18n/allowlist.json", { values: {}, identical: { es: { "app:greet.title": "Hello there" } } });
    expect(problemsOf(left).join("\n")).toMatch(/allowlist: es app:greet\.title is stale/u);
  });

  it("fail on English inside an ICU plural branch", () => {
    const left = fixture((f) => {
      for (const loc of EXPECTED_LOCALES) f.locale(loc, "app", (d) => { d["files.count"] = loc === "es" ? "{count, plural, one {# file} other {# files}}" : loc === "en" ? "{count, plural, one {# file} other {# files}}" : "{count, plural, other {#}}"; });
      f.write("src/renderer/Files.tsx", 'export const Files = () => <p>{t("app.files.count", { count: 2 })}</p>;\n');
    });
    expect(problemsOf(left).join("\n")).toMatch(/es\/app\.json: files\.count is still English/u);
  });

  it("fail when a catalog names another product (D87)", () => {
    expect(problemsOf(fixture((f) => f.locale("ja", "app", (d) => { d["greet.title"] = `${["Pup", "pyOne"].join("")} {name}`; }))).join("\n")).toMatch(/names another product/u);
  });

  it("fail on an unused app message and on a reference to a message that does not exist", () => {
    const unused = fixture((f) => { for (const loc of EXPECTED_LOCALES) f.locale(loc, "app", (d) => { d["dead.key"] = loc === "en" ? "Dead" : `${loc}-dead`; }); });
    expect(problemsOf(unused)).toContain("en/app.json: dead.key is unused");
    const unknown = fixture((f) => f.write("src/renderer/Bad.tsx", 'export const Bad = () => <p>{t("app.nope.here")}</p>;\n'));
    expect(problemsOf(unknown).join("\n")).toMatch(/references unknown message app\.nope\.here/u);
  });

  it("check the native catalogs: simple placeholders only, complete, referenced", () => {
    expect(problemsOf(fixture((f) => f.write("src/shared/locales/native/de.json", { "native.app.quit": "{n, plural, other {Beenden}}" }))).join("\n")).toMatch(/native\/de\.json: native\.app\.quit uses ICU syntax/u);
    expect(problemsOf(fixture((f) => f.write("src/shared/locales/native/ko.json", {}))).join("\n")).toMatch(/native\/ko\.json: missing native\.app\.quit/u);
    expect(problemsOf(fixture((f) => f.write("electron/main/menu.mjs", 'export const menu = (t) => [{ label: t("native.app.nothing") }, { label: t("native.app.quit") }];\n'))).join("\n")).toMatch(/unknown native message native\.app\.nothing/u);
    expect(problemsOf(fixture((f) => f.write("electron/main/menu.mjs", "export const menu = () => [];\n"))).join("\n")).toMatch(/native\/en\.json: native\.app\.quit is unused/u);
  });

  it("fail on hard-coded text in JSX and in native menu labels", () => {
    const jsx = fixture((f) => f.write("src/renderer/Bad.tsx", 'export const Bad = () => <button aria-label="Close the panel" title={"Save it"}>Press here</button>;\n'));
    const text = problemsOf(jsx).join("\n");
    expect(text).toMatch(/hard-coded text "Press here"/u);
    expect(text).toMatch(/hard-coded aria-label attribute "Close the panel"/u);
    expect(text).toMatch(/hard-coded title attribute "Save it"/u);
    expect(problemsOf(fixture((f) => f.write("electron/main/menu.mjs", 'export const menu = (t) => [{ label: "Quit now" }, { label: t("native.app.quit") }];\n'))).join("\n")).toMatch(/hard-coded native label "Quit now"/u);
  });

  it("allow brand names and code identifiers in JSX", () => {
    expect(problemsOf(fixture((f) => f.write("src/renderer/Brand.tsx", 'export const Brand = () => <p placeholder="https://">Super Ontology</p>;\n')))).toEqual([]);
  });

  it("fail on numbers and dates formatted outside the localization layer", () => {
    const raw = fixture((f) => f.write("src/renderer/Raw.ts", "export const a = (n: number) => n.toFixed(2);\nexport const b = (d: Date) => d.toLocaleString();\n"));
    expect(problemsOf(raw).filter((line) => /outside the localization layer/u.test(line))).toHaveLength(2);
  });

  it("fail on a short string that outgrows its control", () => {
    const long = fixture((f) => {
      for (const loc of EXPECTED_LOCALES) f.locale(loc, "app", (d) => { d["ok.label"] = loc === "en" ? "Save" : loc === "de" ? "Speichern und dann noch einmal prüfen" : `${loc}`; });
      f.write("src/renderer/Ok.tsx", 'export const Ok = () => <p>{t("app.ok.label")}</p>;\n');
    });
    expect(problemsOf(long).join("\n")).toMatch(/de\/app\.json: ok\.label is \d+ wide for English 4/u);
  });
});

describe("messageArguments", () => {
  it("collects the arguments of nested ICU messages and flags rich tags", () => {
    expect(messageArguments("{count, plural, one {# of {kind}} other {# of {kind}}} for {who}", "en")).toEqual({ names: ["count", "kind", "who"], rich: false });
    expect(messageArguments("a <b>bold</b> word", "en").rich).toBe(true);
  });
});
