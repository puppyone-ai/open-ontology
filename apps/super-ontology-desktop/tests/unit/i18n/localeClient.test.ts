// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createLocaleClient, loadCatalogBundle } from "../../../src/renderer/i18n";

/** The shell's language is module state (one boot per window), so each shell test starts from a fresh module. */
const fresh = async () => { vi.resetModules(); return import("../../../src/renderer/i18n"); };

type Settings = { language?: string };
function installShell(initial: string) {
  let listener: ((settings: Settings) => void) | null = null;
  const written: string[] = [];
  (window as unknown as { superOntology: unknown }).superOntology = {
    shell: {
      readSettings: async () => ({ language: initial }),
      writeSettings: async (patch: { language: string }) => { written.push(patch.language); },
      onSettingsChanged: (callback: (settings: Settings) => void) => { listener = callback; return () => { listener = null; }; },
    },
  };
  return { written, push: (settings: Settings) => listener?.(settings) };
}
const setSystem = (languages: string[]) => Object.defineProperty(window.navigator, "languages", { value: languages, configurable: true });

beforeEach(() => { window.localStorage.clear(); delete (window as unknown as { superOntology?: unknown }).superOntology; setSystem(["en-US"]); });

describe("the language setting in a plain browser", () => {
  it("follows the system until a language is chosen, and keeps the choice", async () => {
    setSystem(["ja-JP", "en"]);
    const client = createLocaleClient();
    expect(client.getState()).toMatchObject({ preference: "system", locale: "ja" });
    const next = await client.setLanguagePreference("de");
    expect(next).toMatchObject({ preference: "de", locale: "de" });
    expect(window.localStorage.getItem("so.language")).toBe("de");
    expect(createLocaleClient().getState().locale).toBe("de");
  });

  it("tells listeners at once, without a restart, and stops when they leave", async () => {
    const client = createLocaleClient();
    const seen: string[] = [];
    const stop = client.onLocaleChanged((state) => seen.push(state.locale));
    await client.setLanguagePreference("fr");
    stop();
    await client.setLanguagePreference("ko");
    expect(seen).toEqual(["fr"]);
  });

  it("follows a change of the system language while the preference is system, and only then", async () => {
    const client = createLocaleClient();
    const seen: string[] = [];
    client.onLocaleChanged((state) => seen.push(state.locale));
    setSystem(["pt-BR"]);
    window.dispatchEvent(new Event("languagechange"));
    await client.setLanguagePreference("es");
    setSystem(["zh-CN"]);
    window.dispatchEvent(new Event("languagechange"));
    expect(seen).toEqual(["pt-BR", "es"]);
  });
});

describe("the language setting inside the app shell", () => {
  it("adopts the saved language, writes a change back for the native menu, and follows another window", async () => {
    const shell = installShell("zh-Hans");
    const { adoptShellLanguage, createLocaleClient } = await fresh();
    await adoptShellLanguage();
    const client = createLocaleClient();
    expect(client.getState().locale).toBe("zh-Hans");
    const seen: string[] = [];
    client.onLocaleChanged((state) => seen.push(`${state.preference}:${state.locale}`));
    await client.setLanguagePreference("de");
    expect(shell.written).toEqual(["de"]);
    shell.push({ language: "ja" });
    shell.push({ language: "ja" });
    shell.push({ language: "not-a-language" });
    expect(seen).toEqual(["de:de", "ja:ja"]);
  });

  it("ignores a shell that cannot be read", async () => {
    (window as unknown as { superOntology: unknown }).superOntology = { shell: { readSettings: async () => { throw new Error("no"); }, writeSettings: async () => {} } };
    const { adoptShellLanguage, createLocaleClient } = await fresh();
    await adoptShellLanguage();
    expect(createLocaleClient().getState().preference).toBe("system");
  });
});

describe("catalog loading", () => {
  it("loads every locale with the English fallback beside it", async () => {
    for (const locale of ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"] as const) {
      const bundle = await loadCatalogBundle(locale);
      expect(Object.keys(bundle.catalog).length).toBeGreaterThan(1500);
      expect(Object.keys(bundle.fallbackCatalog).length).toBe(Object.keys((await loadCatalogBundle("en")).catalog).length);
    }
  });
});
