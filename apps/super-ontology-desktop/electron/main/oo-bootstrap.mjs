import { execFile } from "node:child_process";
import path from "node:path";

/** Where the `oo` shipped inside the app lives: `Resources/oo/oo` when packaged; a development run may point elsewhere. */
export function bundledExecutablePath({ resourcesPath, env = process.env, appRoot }) {
  if (env.SUPER_ONTOLOGY_OO_BINARY && path.isAbsolute(env.SUPER_ONTOLOGY_OO_BINARY)) return env.SUPER_ONTOLOGY_OO_BINARY;
  if (resourcesPath) return path.join(resourcesPath, "oo", "oo");
  return path.join(appRoot, "build", "oo-bundle", "oo");
}

const FALLBACK_PATH = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/**
 * A GUI launch (Finder, Dock) has a minimal PATH. Ask the login shell for the one the person has in a Terminal; fall back to the
 * inherited one plus the usual Homebrew folders. Bounded: a slow shell profile never blocks start-up beyond the timeout.
 */
export function createEnvironmentReader({ env = process.env, shell = env.SHELL || "/bin/zsh", timeoutMs = 3000, exec = execFile } = {}) {
  let cached = null;
  const marker = "__SO_PATH__";
  async function loginPath() {
    return new Promise((resolve) => {
      try {
        exec(shell, ["-ilc", `printf '${marker}%s${marker}' "$PATH"`], { timeout: timeoutMs, maxBuffer: 64 * 1024, env: { ...env, TERM: "dumb" } }, (error, stdout) => {
          const match = new RegExp(`${marker}(.*)${marker}`, "su").exec(String(stdout ?? ""));
          resolve(!error && match ? match[1] : null);
        });
      } catch { resolve(null); }
    });
  }
  return async function readEnvironment() {
    if (!cached) {
      const fromShell = await loginPath();
      const merged = [...(fromShell ?? "").split(":"), ...(env.PATH ?? "").split(":"), ...FALLBACK_PATH.split(":")].filter((entry) => entry && path.isAbsolute(entry));
      cached = { ...env, PATH: [...new Set(merged)].join(":") };
    }
    return { environment: cached };
  };
}

/**
 * The `oo` bootstrap of the app: locate (setting -> PATH -> bundled) -> start `oo serve` (runtime auto, or the remote engine of the
 * settings) -> status -> stop. The work is done by the engine service; this module is the surface the shell and tests use.
 */
export function createOoBootstrap({ service }) {
  return Object.freeze({
    /** Which `oo` would be started, and where it came from (`user`, `path`, `bundled`). */
    async locate() {
      const snapshot = await service.handlers["engine-preflight"]({});
      return { found: snapshot.preflight?.oo?.found === true, version: snapshot.preflight?.oo?.version ?? null, source: snapshot.preflight?.oo?.source ?? null, status: snapshot.status, code: snapshot.code };
    },
    start: () => service.autoStart(),
    stop: () => service.handlers["engine-stop"]({}),
    status: () => service.handlers["engine-read"]({}),
    shutdown: () => service.shutdown(),
  });
}
