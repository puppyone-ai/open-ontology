import {
  isSelectableAppLanguagePreference,
  mergeCatalogNamespaces,
  resolveLocaleState,
  type AppLanguagePreference,
  type AppLocale,
  type LocaleCatalog,
  type LocaleState,
} from "@open-ontology/design-system/localization/core";
import type { LocaleCatalogBundle, LocaleClient } from "@open-ontology/design-system/localization/react";

const NAMESPACES = ["app", "super-drive"] as const;
const modules = import.meta.glob<{ default: Record<string, string> }>("../../locales/*/*.json", { eager: false });

const STORAGE_KEY = "so.language";
const catalogs = new Map<AppLocale, Promise<LocaleCatalog>>();

function loadCatalog(locale: AppLocale): Promise<LocaleCatalog> {
  const cached = catalogs.get(locale);
  if (cached) return cached;
  const pending = Promise.all(NAMESPACES.map(async (namespace) => {
    const load = modules[`../../locales/${locale}/${namespace}.json`];
    return [namespace, load ? (await load()).default : {}] as const;
  })).then((entries) => mergeCatalogNamespaces(Object.fromEntries(entries)));
  catalogs.set(locale, pending);
  return pending;
}

export async function loadCatalogBundle(locale: AppLocale): Promise<LocaleCatalogBundle> {
  const catalog = await loadCatalog(locale);
  const fallbackCatalog = locale === "en" ? catalog : await loadCatalog("en");
  return Object.freeze({ catalog, fallbackCatalog });
}

const systemLanguages = (): readonly string[] => (navigator.languages?.length ? navigator.languages : [navigator.language || "en"]);
/** Inside the Electron shell the language is the shell's setting (it also drives the native menu); a plain browser keeps it in localStorage. */
type ShellLanguage = { readSettings(): Promise<{ language: string }>; writeSettings(patch: { language: string }): Promise<unknown> };
const shellOf = (): ShellLanguage | null => (window as unknown as { superOntology?: { shell?: ShellLanguage } }).superOntology?.shell ?? null;
let hostedPreference: AppLanguagePreference | null = null;
export async function adoptShellLanguage(): Promise<void> {
  const shell = shellOf();
  if (!shell) return;
  try { const value = (await shell.readSettings()).language; if (isSelectableAppLanguagePreference(value)) hostedPreference = value; } catch { /* keep the stored preference */ }
}
const readPreference = (): AppLanguagePreference => {
  if (hostedPreference) return hostedPreference;
  const value = window.localStorage.getItem(STORAGE_KEY);
  return isSelectableAppLanguagePreference(value) ? value : "system";
};

/** Language is a renderer preference: it lives in localStorage and applies at once; the main process never needs it. */
export function createLocaleClient(): LocaleClient & { getState: () => LocaleState } {
  const listeners = new Set<(state: LocaleState) => void>();
  const getState = () => resolveLocaleState(readPreference(), systemLanguages());
  return Object.freeze({
    getState,
    async setLanguagePreference(preference: AppLanguagePreference) {
      window.localStorage.setItem(STORAGE_KEY, preference);
      if (shellOf()) { hostedPreference = preference; void shellOf()?.writeSettings({ language: preference }).catch(() => {}); }
      const state = getState();
      for (const listener of listeners) listener(state);
      return state;
    },
    onLocaleChanged(callback: (state: LocaleState) => void) {
      listeners.add(callback);
      return () => { listeners.delete(callback); };
    },
  });
}
