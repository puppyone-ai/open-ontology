import path from "node:path";

/**
 * In the Docker runtime the engine reads a folder Drive through a bind mount, so its Drive documents say `config.root: /drives/<id>`: a path
 * inside the container. Everything the app does with the original (preview, watching for changes, upload, write-back, host decode) needs the
 * folder on this Mac. `oo serve` records which host folder it mounted for which id in `<state>/drive-mounts.json`; this wrapper puts that
 * host path into the Drive documents the engine answers, and leaves everything else (host runtime, object stores) as it is.
 */
const CONTAINER_ROOT = /^\/drives\/([0-9a-f]{16})$/u;
const FILES = ["drive-mounts.json", "mounts.json"];

export function createDriveHostRoots({ stateDir, fsPromises }) {
  async function table() {
    for (const name of FILES) {
      try {
        const parsed = JSON.parse(await fsPromises.readFile(path.join(stateDir, name), "utf8"));
        if (parsed && typeof parsed.mounts === "object" && parsed.mounts !== null) return parsed.mounts;
      } catch { /* Try the next file; no table means no rewriting. */ }
    }
    return {};
  }
  async function resolve(drive) {
    const root = drive?.config?.root;
    const match = typeof root === "string" ? CONTAINER_ROOT.exec(root) : null;
    if (!match) return drive;
    const host = (await table())[match[1]];
    return typeof host === "string" && path.isAbsolute(host) ? { ...drive, config: { ...drive.config, root: host } } : drive;
  }
  return Object.freeze({
    resolve,
    async resolveResult(name, result) {
      if (!result || typeof result !== "object") return result;
      if (["driveGet", "driveCreate", "driveUpdate"].includes(name) && result.value) return { ...result, value: await resolve(result.value) };
      if (name === "drivesList" && Array.isArray(result.value?.items)) return { ...result, value: { ...result.value, items: await Promise.all(result.value.items.map(resolve)) } };
      return result;
    },
  });
}

/** The control client with Drive documents carrying host folders (see above). */
export function withHostDriveRoots(client, { stateDir, fsPromises }) {
  if (!stateDir) return client;
  const roots = createDriveHostRoots({ stateDir, fsPromises });
  return Object.freeze({ ...client, request: async (name, options) => roots.resolveResult(name, await client.request(name, options)) });
}
