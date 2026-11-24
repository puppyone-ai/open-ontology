import fs from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import path from "node:path";
import { runBoundedProcessProbe } from "../platform/common/bounded-process-probe.mjs";

/** Every external probe of Super Drive is bounded; 4 s is the budget of 20-frontend §1.2.3. */
export const PROBE_TIMEOUT_MS = 4_000;

/** Docker Desktop on macOS does not always put its CLI on a GUI-launched PATH. */
const MACOS_FALLBACK_DIRECTORIES = Object.freeze([
  "/usr/local/bin", "/opt/homebrew/bin", "/Applications/Docker.app/Contents/Resources/bin",
]);

async function isExecutableFile(fsPromises, candidate) {
  try {
    const stat = await fsPromises.stat(candidate);
    if (!stat.isFile()) return false;
    await fsPromises.access(candidate, fsConstants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Finds `docker` and `oo` and reads the `oo` version (20-frontend §1.2.3).
 * Platform differences live here, never in `electron/main.mjs`: `platform` is
 * the product identifier from the platform host ("macos" | "windows" | "linux").
 *
 * @param {object} options
 * @param {"macos"|"windows"|"linux"} options.platform
 * @param {(input?: object) => Promise<{ environment: Record<string, string> }>} options.readEnvironment user command environment
 */
export function createEngineLocator({
  platform,
  readEnvironment,
  fsPromises = fs,
  runProbe = runBoundedProcessProbe,
  probeTimeoutMs = PROBE_TIMEOUT_MS,
  fallbackDirectories = MACOS_FALLBACK_DIRECTORIES,
  /** `Resources/oo/oo` of the app (or the folder a development run points at); `null` when this build carries none. */
  bundledExecutable = () => null,
  /** An `oo` on PATH older than this is skipped in favor of the bundled one. */
  minimumVersion = "0.2.0",
} = {}) {
  function searchDirectories(environment) {
    const fromPath = typeof environment?.PATH === "string" ? environment.PATH.split(path.delimiter) : [];
    const directories = [...fromPath, ...(platform === "macos" ? fallbackDirectories : [])];
    return [...new Set(directories.filter((directory) => path.isAbsolute(directory)))];
  }

  async function findInPath(name, environment) {
    for (const directory of searchDirectories(environment)) {
      const candidate = path.join(directory, name);
      if (await isExecutableFile(fsPromises, candidate)) return candidate;
    }
    return null;
  }

  async function readOoVersion(ooPath, environment) {
    try {
      const result = await runProbe(ooPath, ["--version", "--json"], {
        env: environment, timeoutMs: probeTimeoutMs, maxOutputBytes: 8 * 1024, label: "oo version probe",
      });
      if (result.code !== 0) return null;
      return parseOoVersion(result.stdout);
    } catch {
      return null;
    }
  }

  /** Some `oo` distributions (the npm one) have no `serve`: they answer `serve --status` with an "unsupported" document. */
  async function supportsServe(ooPath, environment) {
    try {
      const result = await runProbe(ooPath, ["serve", "--status", "--json"], { env: environment, timeoutMs: probeTimeoutMs, maxOutputBytes: 16 * 1024, label: "oo serve probe" });
      return !/"class"\s*:\s*"unsupported"|not implemented/iu.test(`${result.stdout}\n${result.stderr}`);
    } catch {
      return false;
    }
  }

  return Object.freeze({
    readOoVersion,
    async environment({ signal } = {}) { return (await readEnvironment({ signal })).environment; },
    /**
     * @returns {Promise<{ environment: object, dockerPath: string|null, oo: { path: string, version: string|null, source: "path"|"user" }|null, ooInvalid: boolean }>}
     */
    async locate({ userExecutable = null, signal } = {}) {
      const { environment } = await readEnvironment({ signal });
      const dockerPath = await findInPath("docker", environment);
      let ooPath = null;
      let source = null;
      let version = null;
      let invalid = false;
      // 1) the path the person chose, 2) `oo` on PATH (new enough), 3) the binary shipped inside the app.
      const candidates = [];
      if (userExecutable && await isExecutableFile(fsPromises, userExecutable)) candidates.push({ path: userExecutable, source: "user" });
      const onPath = await findInPath("oo", environment);
      if (onPath) candidates.push({ path: onPath, source: "path" });
      const bundled = bundledExecutable();
      if (bundled && await isExecutableFile(fsPromises, bundled)) candidates.push({ path: bundled, source: "bundled" });
      for (const candidate of candidates) {
        const read = await readOoVersion(candidate.path, environment);
        // A file that cannot report its version is not a usable `oo`.
        if (!read) { invalid = true; continue; }
        if (candidate.source === "path" && compareVersions(read, minimumVersion) < 0) continue;
        if (candidate.source !== "bundled" && !(await supportsServe(candidate.path, environment))) { invalid = invalid || candidate.source === "user"; continue; }
        ooPath = candidate.path; source = candidate.source; version = read; invalid = false;
        break;
      }
      if (!ooPath) return { environment, dockerPath, oo: null, ooInvalid: invalid };
      return { environment, dockerPath, oo: { path: ooPath, version, source }, ooInvalid: false };
    },
  });
}

/** `oo --version --json` prints `{"version":"x.y.z",...}`; a bare `x.y.z` line is accepted too. */
export function parseOoVersion(stdout) {
  const text = String(stdout ?? "").trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text);
    if (typeof parsed?.version === "string" && /^[0-9A-Za-z][0-9A-Za-z.+_-]{0,63}$/u.test(parsed.version)) return parsed.version;
    return null;
  } catch {
    const match = /(?:^|\s)v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)(?:\s|$)/u.exec(text);
    return match ? match[1] : null;
  }
}

/** Numeric `major.minor.patch` comparison; pre-release suffixes are ignored. */
export function compareVersions(left, right) {
  const parts = (value) => String(value).split(/[-+]/u)[0].split(".").map((part) => Number.parseInt(part, 10) || 0);
  const a = parts(left); const b = parts(right);
  for (let index = 0; index < 3; index += 1) {
    const delta = (a[index] ?? 0) - (b[index] ?? 0);
    if (delta !== 0) return delta < 0 ? -1 : 1;
  }
  return 0;
}
