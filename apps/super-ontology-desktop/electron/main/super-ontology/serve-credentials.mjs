import fs from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import path from "node:path";
import { SuperOntologyError } from "../../../shared/super-ontology/errors.mjs";

const SERVE_PROTOCOL = "oo.docs.serve/v1";
const TOKEN_PATTERN = /^oodt_[A-Za-z0-9_-]{43}$/u;
const MAX_FILE_BYTES = 16 * 1024;

const invalid = () => new SuperOntologyError("SO_SERVE_CREDENTIALS_INVALID");

function requirePrivateFile(stat, getUid) {
  const uid = getUid();
  if (!stat.isFile()) throw invalid();
  if (uid !== null && stat.uid !== uid) throw invalid();
  // Group and other must have no access at all (0600).
  if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) throw invalid();
}

/**
 * Reads `<stateDir>/serve.json` and the control token (20-frontend §1.4.4).
 * Returns `null` when no instance wrote `serve.json` yet. Any other problem
 * (symlink, foreign owner, loose permissions, bad protocol, token outside
 * the state directory, malformed token) is SO_SERVE_CREDENTIALS_INVALID.
 * The token is returned to the caller only; it never appears in an error.
 *
 * @returns {Promise<null | { info: { pid: number|null, port: number, apiMajor: number, apiVersion: string, startedAt: string|null }, token: string }>}
 */
export async function readServeCredentials({
  stateDir,
  fsPromises = fs,
  getUid = () => (typeof process.getuid === "function" ? process.getuid() : null),
} = {}) {
  if (typeof stateDir !== "string" || !path.isAbsolute(stateDir)) throw invalid();
  const serveJsonPath = path.join(stateDir, "serve.json");
  let linkStat;
  try {
    linkStat = await fsPromises.lstat(serveJsonPath);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw invalid();
  }
  if (linkStat.isSymbolicLink()) throw invalid();
  requirePrivateFile(linkStat, getUid);
  if (linkStat.size > MAX_FILE_BYTES) throw invalid();

  let document;
  try {
    document = JSON.parse(await fsPromises.readFile(serveJsonPath, "utf8"));
  } catch {
    throw invalid();
  }
  if (!document || document.protocol !== SERVE_PROTOCOL) throw invalid();
  const apiVersion = typeof document.api_version === "string" ? document.api_version : "";
  const apiMajor = Number.parseInt(apiVersion.split(".")[0], 10);
  if (!Number.isInteger(apiMajor)) throw invalid();
  const { port } = document;
  if (!Number.isInteger(port) || port < 1 || port > 65_535) throw invalid();
  const pid = Number.isInteger(document.pid) && document.pid > 0 ? document.pid : null;

  const tokenFile = document.token_file;
  if (typeof tokenFile !== "string" || !path.isAbsolute(tokenFile)) throw invalid();
  let realStateDir;
  let realTokenDir;
  try {
    realStateDir = await fsPromises.realpath(stateDir);
    realTokenDir = await fsPromises.realpath(path.dirname(tokenFile));
  } catch {
    throw invalid();
  }
  if (realTokenDir !== realStateDir && !realTokenDir.startsWith(`${realStateDir}${path.sep}`)) throw invalid();

  const token = await readTokenFile(path.join(realTokenDir, path.basename(tokenFile)), { fsPromises, getUid });
  return {
    info: {
      pid, port, apiMajor, apiVersion,
      startedAt: typeof document.started_at === "string" ? document.started_at : null,
    },
    token,
  };
}

export async function readTokenFile(tokenPath, { fsPromises = fs, getUid = () => (typeof process.getuid === "function" ? process.getuid() : null) } = {}) {
  // O_NOFOLLOW: a symlink in place of the token file fails the open.
  const noFollow = fsConstants.O_NOFOLLOW ?? 0;
  let handle;
  try {
    handle = await fsPromises.open(tokenPath, fsConstants.O_RDONLY | noFollow);
    const stat = await handle.stat();
    requirePrivateFile(stat, getUid);
    if (stat.size > MAX_FILE_BYTES) throw invalid();
    const token = (await handle.readFile({ encoding: "utf8" })).trim();
    if (!TOKEN_PATTERN.test(token)) throw invalid();
    return token;
  } catch (error) {
    throw error instanceof SuperOntologyError ? error : invalid();
  } finally {
    await handle?.close().catch(() => {});
  }
}
