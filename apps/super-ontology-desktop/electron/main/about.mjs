import fs from "node:fs/promises";
import path from "node:path";

const FILES = Object.freeze({ notice: "NOTICE", "third-party": "THIRD_PARTY_NOTICES.md", license: "LICENSE" });

/** Legal texts for the About page: `Resources/legal/` when packaged, the app folder in development. */
export function createLegalReader({ directories, fsPromises = fs }) {
  return async function readNotice(kind) {
    const name = FILES[kind];
    if (!name) return "";
    for (const directory of directories) {
      try { return await fsPromises.readFile(path.join(directory, name), "utf8"); } catch { /* try the next folder */ }
    }
    return "";
  };
}

/** `build/generated/release.json` (`{ "url": ... }`) is written by the build from the repository the release belongs to. */
export async function readReleaseUrl({ directories, fsPromises = fs }) {
  for (const directory of directories) {
    try {
      const parsed = JSON.parse(await fsPromises.readFile(path.join(directory, "release.json"), "utf8"));
      if (typeof parsed?.url === "string" && /^https:\/\//u.test(parsed.url)) return parsed.url;
    } catch { /* absent in development */ }
  }
  return null;
}
