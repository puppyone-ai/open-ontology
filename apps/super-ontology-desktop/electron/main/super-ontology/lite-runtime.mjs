import fs from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import path from "node:path";

/**
 * The Docker-free runtime shipped with the app (F-S14-01, ADR-S14): an embedded PostgreSQL 17 and, for Office and PDF Drives, the
 * parser sidecar. This module only asks whether the pieces are there; the engine itself starts them (`oo serve --runtime host`).
 *
 * Layout of the directory (an assumption the build has to meet, in one place):
 *   postgres/bin/{postgres,initdb,pg_ctl}   executables of the pinned PostgreSQL build
 *   sidecar/                               the parser environment (optional: Markdown, text and CSV Drives need none)
 *   runtime.json                           `{ "protocol": "oo.lite-runtime/v1", "postgres": "17.x" }` (version for the status line)
 */
export const LITE_RUNTIME_MANIFEST = "runtime.json";
const POSTGRES_BINARIES = Object.freeze(["postgres", "initdb", "pg_ctl"]);

async function isExecutable(fsPromises, file) {
  try {
    if (!(await fsPromises.stat(file)).isFile()) return false;
    await fsPromises.access(file, fsConstants.X_OK);
    return true;
  } catch {
    return false;
  }
}

async function isDirectory(fsPromises, directory) {
  try { return (await fsPromises.stat(directory)).isDirectory(); } catch { return false; }
}

/** @param {{ directory?: string | null, fsPromises?: typeof fs }} options `directory`: the app's embedded runtime folder, or `null` when this build has none. */
export function createLiteRuntimeProbe({ directory = null, fsPromises = fs } = {}) {
  return Object.freeze({
    directory,
    /** @returns {Promise<{ found: boolean, postgres: { found: boolean, version: string | null }, sidecar: { found: boolean } }>} */
    async probe() {
      const none = { found: false, postgres: { found: false, version: null }, sidecar: { found: false } };
      if (typeof directory !== "string" || !path.isAbsolute(directory)) return none;
      const binaries = await Promise.all(POSTGRES_BINARIES.map((name) => isExecutable(fsPromises, path.join(directory, "postgres", "bin", name))));
      const postgresFound = binaries.every(Boolean);
      let version = null;
      try {
        const manifest = JSON.parse(await fsPromises.readFile(path.join(directory, LITE_RUNTIME_MANIFEST), "utf8"));
        if (manifest?.protocol === "oo.lite-runtime/v1" && typeof manifest.postgres === "string" && /^[0-9][0-9A-Za-z.+_-]{0,31}$/u.test(manifest.postgres)) version = manifest.postgres;
      } catch { /* No manifest: the version stays unknown. */ }
      const sidecarFound = await isDirectory(fsPromises, path.join(directory, "sidecar"));
      return { found: postgresFound, postgres: { found: postgresFound, version: postgresFound ? version : null }, sidecar: { found: sidecarFound } };
    },
  });
}
