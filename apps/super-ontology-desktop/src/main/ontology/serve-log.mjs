import fs from "node:fs/promises";
import path from "node:path";

export const SERVE_LOG_NAME = "super-ontology-serve.log";
const DEFAULT_MAX_BYTES = 5 * 1024 * 1024;
const DEFAULT_KEEP = 3;

/**
 * The engine daemon writes straight to this file (an O_APPEND descriptor, so
 * it keeps working when the Desktop process is gone). Rotation is
 * copy-and-truncate: 5 MiB x 3 files (20-frontend §1.4.1). Desktop never
 * writes daemon output through its own logger.
 */
export function createServeLog({ directory, name = SERVE_LOG_NAME, maxBytes = DEFAULT_MAX_BYTES, keep = DEFAULT_KEEP, fsPromises = fs } = {}) {
  const file = path.join(directory, name);

  async function rotateIfNeeded() {
    let size;
    try { size = (await fsPromises.stat(file)).size; } catch { return false; }
    if (size < maxBytes) return false;
    for (let index = keep - 1; index >= 1; index -= 1) {
      await fsPromises.rename(`${file}.${index}`, `${file}.${index + 1}`).catch(() => {});
    }
    await fsPromises.copyFile(file, `${file}.1`);
    await fsPromises.truncate(file, 0);
    // Anything beyond `keep` rotations is dropped.
    await fsPromises.rm(`${file}.${keep + 1}`, { force: true });
    return true;
  }

  return Object.freeze({
    file,
    rotateIfNeeded,
    /** Opens the append descriptor handed to the child; the caller closes it after spawn. */
    async open() {
      await fsPromises.mkdir(directory, { recursive: true, mode: 0o700 });
      await rotateIfNeeded();
      return fsPromises.open(file, "a", 0o600);
    },
  });
}
