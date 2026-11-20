import { describe, expect, it } from "vitest";
import { NATIVE_LANGUAGES, createTranslator, resolveLanguage } from "../../../electron/main/strings.mjs";
import { buildMenuTemplate } from "../../../electron/main/menu.mjs";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];
const labels = (template) => template.flatMap((item) => [item.label, ...(item.submenu ?? []).map((sub) => sub.label)]).filter(Boolean);
const build = (language) => buildMenuTemplate({ t: createTranslator(() => language), onCommand() {}, openReleasePage() {}, revealLogs() {}, isDev: true });

describe("native catalogs", () => {
  it("are loaded for every locale from JSON", () => {
    expect([...NATIVE_LANGUAGES].sort()).toEqual([...LOCALES].sort());
  });

  it("follow the setting, then the system, and map Chinese and Portuguese", () => {
    expect(resolveLanguage("de", "en-US")).toBe("de");
    expect(resolveLanguage("system", "zh-TW")).toBe("zh-Hans");
    expect(resolveLanguage("system", "pt-PT")).toBe("pt-BR");
    expect(resolveLanguage("system", "fr-CA")).toBe("fr");
    expect(resolveLanguage("system", "it-IT")).toBe("en");
  });

  it("fall back to English for an unknown language and to the key for an unknown message", () => {
    expect(createTranslator(() => "xx")("native.app.quit")).toBe("Quit Super Ontology");
    expect(createTranslator(() => "de")("native.nothing")).toBe("native.nothing");
  });

  it("build a menu with no English label left in another language, role items included", () => {
    const english = labels(build("en"));
    expect(english).toContain("Undo");
    expect(english).toContain("Bring All to Front");
    for (const locale of LOCALES.filter((loc) => loc !== "en")) {
      const own = labels(build(locale));
      expect(own).toHaveLength(english.length);
      // The app name and "Zoom"/"Services" are the system's own in some languages; everything else must differ.
      const shared = own.filter((label, index) => label === english[index]);
      expect(shared.filter((label) => !["Super Ontology", "Zoom", "Services"].includes(label)), locale).toEqual([]);
    }
  });

  it("rebuild the menu in the new language at once", () => {
    expect(labels(build("zh-Hans"))).toContain("设置…");
    expect(labels(build("de"))).toContain("Einstellungen…");
  });
});
