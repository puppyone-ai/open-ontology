import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

/**
 * Where the local engine keeps its state (D83: one shared engine per user). The `oo` CLI and this app use the same directory, so an
 * `oo serve` that either one started serves both: `OO_DOCS_STATE_DIR`, else `$HOME/.local/state/open-ontology/docs` (the CLI's own default).
 * An install that already holds data in its private directory keeps using it until the person picks "Use the CLI's data" (nothing is moved or deleted).
 */
export const ENGINE_DATA_MODES = Object.freeze(["cli", "app"]);

export function cliStateDirectory({ env = process.env, homedir = os.homedir } = {}) {
  const fromEnv = env.OO_DOCS_STATE_DIR;
  if (typeof fromEnv === "string" && path.isAbsolute(fromEnv)) return fromEnv;
  return path.join(homedir(), ".local", "state", "open-ontology", "docs");
}

/** The directory `oo` reads profiles from: `OO_CONFIG_DIR`, else `$HOME/.config/open-ontology`. */
export function cliConfigDirectory({ env = process.env, homedir = os.homedir } = {}) {
  const fromEnv = env.OO_CONFIG_DIR;
  if (typeof fromEnv === "string" && path.isAbsolute(fromEnv)) return fromEnv;
  return path.join(homedir(), ".config", "open-ontology");
}

async function hasEntries(directory, fsPromises) {
  try { return (await fsPromises.readdir(directory)).length > 0; } catch { return false; }
}

/**
 * Picks the state directory. `choice` is the saved setting (`cli`, `app` or null = not decided); `SUPER_ONTOLOGY_ENGINE_DATA` overrides it
 * (tests and support). Not decided: a private directory that already holds data wins, anything else uses the CLI's directory.
 */
export async function resolveEngineState({ legacyDir, choice = null, env = process.env, homedir = os.homedir, fsPromises = fs } = {}) {
  const cliDir = cliStateDirectory({ env, homedir });
  const override = ENGINE_DATA_MODES.includes(env.SUPER_ONTOLOGY_ENGINE_DATA) ? env.SUPER_ONTOLOGY_ENGINE_DATA : null;
  const decided = override ?? (ENGINE_DATA_MODES.includes(choice) ? choice : null);
  const legacyHasData = await hasEntries(legacyDir, fsPromises);
  const cliHasData = await hasEntries(cliDir, fsPromises);
  const mode = decided ?? (legacyHasData ? "app" : "cli");
  return {
    mode,
    decided: decided !== null,
    stateDir: mode === "cli" ? cliDir : legacyDir,
    cliDir,
    legacyDir,
    legacyHasData,
    cliHasData,
    configDir: cliConfigDirectory({ env, homedir }),
  };
}

/** The active `oo` profile, read-only and without any secret: its name and the Gateway host. Null when there is none. */
export async function readActiveProfile({ configDir, fsPromises = fs } = {}) {
  const readJson = async (file) => {
    try {
      const stat = await fsPromises.lstat(file);
      if (!stat.isFile() || stat.size > 65_536) return null;
      return JSON.parse(await fsPromises.readFile(file, "utf8"));
    } catch { return null; }
  };
  const pointer = await readJson(path.join(configDir, "active-profile.json"));
  const name = typeof pointer?.profile === "string" && /^[A-Za-z0-9_-]{1,64}$/u.test(pointer.profile) ? pointer.profile : null;
  if (!name) return null;
  const profile = await readJson(path.join(configDir, "profiles", `${name}.json`));
  let gatewayHost = null;
  try { gatewayHost = typeof profile?.gateway === "string" ? new URL(profile.gateway).host : null; } catch { gatewayHost = null; }
  return { name, gatewayHost, saved: profile !== null };
}
