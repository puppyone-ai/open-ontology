import fs from "node:fs/promises";
import path from "node:path";

export const LANGUAGES = Object.freeze(["system", "en", "zh-Hans", "es", "pt-BR", "fr", "de", "ja", "ko"]);
export const RUNTIMES = Object.freeze(["auto", "docker", "lite"]);
/** Keys the renderer may write through `shell:write-settings`; everything else is owned by main. */
export const RENDERER_WRITABLE = Object.freeze(["language"]);

export const DEFAULT_SETTINGS = Object.freeze({
  language: "system", ooExecutable: null, runtime: "auto", keepEngineOnQuit: false, remote: null,
});

/** Returns a clean settings object; anything invalid falls back to the default of its key. */
export function sanitizeSettings(input) {
  const source = input && typeof input === "object" ? input : {};
  const remote = source.remote && typeof source.remote === "object"
    && typeof source.remote.url === "string" && typeof source.remote.tokenFile === "string" && path.isAbsolute(source.remote.tokenFile)
    ? { url: source.remote.url, tokenFile: source.remote.tokenFile } : null;
  return {
    language: LANGUAGES.includes(source.language) ? source.language : DEFAULT_SETTINGS.language,
    ooExecutable: typeof source.ooExecutable === "string" && path.isAbsolute(source.ooExecutable) ? source.ooExecutable : null,
    runtime: RUNTIMES.includes(source.runtime) ? source.runtime : DEFAULT_SETTINGS.runtime,
    keepEngineOnQuit: source.keepEngineOnQuit === true,
    remote,
  };
}

/** One JSON file (0600), written atomically; reads are cached, writes are serialized. */
export function createSettingsStore({ file, fsPromises = fs, logger = console } = {}) {
  let cache = null;
  let queue = Promise.resolve();
  const listeners = new Set();

  async function load() {
    if (cache) return cache;
    try { cache = sanitizeSettings(JSON.parse(await fsPromises.readFile(file, "utf8"))); } catch { cache = sanitizeSettings({}); }
    return cache;
  }

  async function persist(next) {
    await fsPromises.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
    const temporary = `${file}.${process.pid}.tmp`;
    await fsPromises.writeFile(temporary, JSON.stringify(next, null, 2), { mode: 0o600 });
    await fsPromises.rename(temporary, file);
  }

  return Object.freeze({
    file,
    read: async () => ({ ...(await load()) }),
    /** Merges known keys only; unknown keys are ignored here (the IPC layer rejects them first). */
    update(patch) {
      const run = queue.then(async () => {
        const merged = sanitizeSettings({ ...(await load()), ...(patch ?? {}) });
        cache = merged;
        try { await persist(merged); } catch (error) { logger.warn?.("Settings could not be saved.", { code: error?.code }); throw error; }
        for (const listener of [...listeners]) { try { listener({ ...merged }); } catch { /* observers must not break writes */ } }
        return { ...merged };
      });
      queue = run.catch(() => {});
      return run;
    },
    onChange(listener) { listeners.add(listener); return () => listeners.delete(listener); },
  });
}
