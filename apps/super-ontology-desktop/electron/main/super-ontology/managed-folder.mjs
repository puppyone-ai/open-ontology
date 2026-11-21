import fs from "node:fs/promises";
import path from "node:path";

/** What the CLI called its managed Drive before the CLI and the app shared one "My files" folder. */
export const LEGACY_MANAGED_NAME = "default";
export const MY_FILES = "My files";
/** A merged earlier folder keeps its Drive (paused, not deleted) under this name; the app lists it as a note, not as a source. */
export const MERGED_NAME = "My files (earlier CLI folder)";

const live = (drive) => drive && drive.backend === "fs" && drive.status !== "deleting" && drive.status !== "deleted";
const rootOf = (drive) => (typeof drive?.config?.root === "string" && path.isAbsolute(drive.config.root) ? drive.config.root : null);

async function* walk(directory, relative, fsPromises) {
  let entries;
  try { entries = await fsPromises.readdir(path.join(directory, relative), { withFileTypes: true }); } catch { return; }
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    const next = path.join(relative, entry.name);
    if (entry.isDirectory()) yield* walk(directory, next, fsPromises);
    else if (entry.isFile()) yield next;
  }
}

/** The first free name beside an existing different file: `a.md` -> `a (2).md`. */
async function freeName(target, fsPromises) {
  const { dir, name, ext } = path.parse(target);
  for (let n = 2; n < 1000; n += 1) {
    const candidate = path.join(dir, `${name} (${n})${ext}`);
    try { await fsPromises.lstat(candidate); } catch { return candidate; }
  }
  return null;
}

/**
 * One managed folder (D83): the CLI's `default` Drive and the app's "My files" are the same thing to a person.
 * No "My files" yet: the earlier Drive is renamed. Both exist: the files of the earlier folder are copied (never over a file that is
 * there, never deleted from the earlier folder), "My files" is synced, and the earlier Drive is paused and renamed so the app
 * shows one folder and says what happened. Idempotent: once renamed, nothing matches again.
 */
export function createManagedFolder({ client, fsPromises = fs, logger = console } = {}) {
  let running = null;
  async function run() {
    const { value: listed } = await client.request("drivesList", { query: { limit: 200 } });
    const items = Array.isArray(listed?.items) ? listed.items : [];
    const legacy = items.find((d) => live(d) && d.name === LEGACY_MANAGED_NAME && /(^|\/)drives\/default\/?$/u.test(rootOf(d) ?? ""));
    if (!legacy) return { action: "none" };
    const mine = items.find((d) => live(d) && d.name === MY_FILES);
    if (!mine) {
      await client.request("driveUpdate", { params: { id: legacy.id }, body: { name: MY_FILES }, revision: legacy.revision });
      return { action: "renamed", driveId: legacy.id };
    }
    const from = rootOf(legacy);
    const to = rootOf(mine);
    if (!from || !to) return { action: "none" };
    let copied = 0;
    let present = 0;
    for await (const relative of walk(from, "", fsPromises)) {
      const source = path.join(from, relative);
      let target = path.join(to, relative);
      await fsPromises.mkdir(path.dirname(target), { recursive: true, mode: 0o700 });
      try {
        await fsPromises.copyFile(source, target, fs.constants.COPYFILE_EXCL);
        copied += 1;
      } catch (error) {
        if (error?.code !== "EEXIST") throw error;
        const [a, b] = await Promise.all([fsPromises.stat(source), fsPromises.stat(target)]);
        if (a.size === b.size) { present += 1; continue; }
        target = await freeName(target, fsPromises);
        if (!target) continue;
        await fsPromises.copyFile(source, target, fs.constants.COPYFILE_EXCL);
        copied += 1;
      }
    }
    await client.request("driveSync", { params: { id: mine.id }, body: {}, operationId: `merge-${legacy.id}`.replace(/[^A-Za-z0-9_-]/gu, "_").padEnd(16, "0") }).catch(() => {});
    await client.request("driveUpdate", { params: { id: legacy.id }, body: { name: MERGED_NAME, status: "paused" }, revision: legacy.revision });
    return { action: "merged", driveId: mine.id, copied, present };
  }
  return Object.freeze({
    merge() {
      running ??= run().catch((error) => { logger.warn?.("Merging the CLI's managed folder failed.", { code: error?.code }); return { action: "failed" }; }).finally(() => { running = null; });
      return running;
    },
  });
}
