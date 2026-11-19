import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";

const SECRET_FIELDS = Object.freeze(["access_key_id", "secret_access_key", "session_token"]);
const FILE_MODE = 0o600;
const DIRECTORY_MODE = 0o700;

/**
 * Object-store credentials (20-frontend §1.2.3, §3.9). A secret is written once to a private file and only a
 * `file:` secret_ref travels on: to the engine in the Drive config, never back to the renderer. Names are derived from the
 * request's operation id, so a repeated request writes the same files and sends the same body (the engine replays it
 * instead of rejecting a changed one). Nothing here ever reads a secret back or puts its value in an error.
 */
export function createSecretFiles({ directory, fsPromises = fs.promises } = {}) {
  const unavailable = () => new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");

  async function ensureDirectory() {
    if (typeof directory !== "string" || !path.isAbsolute(directory)) throw unavailable();
    try {
      await fsPromises.mkdir(directory, { recursive: true, mode: DIRECTORY_MODE });
      const stat = await fsPromises.lstat(directory);
      if (!stat.isDirectory() || stat.isSymbolicLink()) throw unavailable();
      if ((stat.mode & 0o077) !== 0) await fsPromises.chmod(directory, DIRECTORY_MODE);
    } catch (error) {
      if (error instanceof SuperOntologyError) throw error;
      throw unavailable();
    }
  }

  const fileFor = (key, field) => path.join(directory, `s3-${createHash("sha256").update(String(key)).digest("hex").slice(0, 32)}-${field}`);
  const refOf = (file) => `file:${file}`;

  /** The path a `file:` ref points at when it lies directly inside the secrets directory; otherwise `null` (not ours, never touched). */
  function ownFile(ref) {
    if (typeof directory !== "string" || typeof ref !== "string" || !ref.startsWith("file:")) return null;
    const file = ref.slice("file:".length);
    return path.isAbsolute(file) && path.dirname(file) === directory ? file : null;
  }

  async function writeOne(file, value) {
    const temporary = `${file}.${process.pid}.tmp`;
    try {
      const handle = await fsPromises.open(temporary, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_TRUNC | fs.constants.O_NOFOLLOW, FILE_MODE);
      try {
        await handle.writeFile(value, "utf8");
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fsPromises.chmod(temporary, FILE_MODE);
      await fsPromises.rename(temporary, file);
    } catch {
      await fsPromises.rm(temporary, { force: true }).catch(() => undefined);
      throw unavailable();
    }
  }

  return Object.freeze({
    /**
     * Writes the given fields (`access_key_id`, `secret_access_key`, optional `session_token`) and returns
     * `{ field: { secret_ref } }` for the Drive config. On any failure the files written so far are removed.
     */
    async writeSet(key, values) {
      await ensureDirectory();
      const written = [];
      const refs = {};
      try {
        for (const field of SECRET_FIELDS) {
          const value = values[field];
          if (value === undefined || value === null || value === "") continue;
          if (typeof value !== "string") throw new SuperOntologyError("SO_INVALID_REQUEST");
          const file = fileFor(key, field);
          await writeOne(file, value);
          written.push(file);
          refs[field] = { secret_ref: refOf(file) };
        }
      } catch (error) {
        await Promise.all(written.map((file) => fsPromises.rm(file, { force: true }).catch(() => undefined)));
        throw error;
      }
      return refs;
    },

    /** Removes the files a Drive config references, but only those inside the secrets directory. */
    async removeConfigSecrets(config) {
      if (!config || typeof config !== "object") return 0;
      let removed = 0;
      for (const field of SECRET_FIELDS) {
        const file = ownFile(config[field]?.secret_ref);
        if (!file) continue;
        try {
          await fsPromises.rm(file, { force: true });
          removed += 1;
        } catch {
          // A file that cannot be removed stays private (0600) in a private directory; the Drive is gone either way.
        }
      }
      return removed;
    },

    /** Removes the files `writeSet(key, …)` would have written (a create that failed or was replayed). */
    async removeSet(key) {
      if (typeof directory !== "string") return;
      await Promise.all(SECRET_FIELDS.map((field) => fsPromises.rm(fileFor(key, field), { force: true }).catch(() => undefined)));
    },

    secretFields: SECRET_FIELDS,
  });
}
