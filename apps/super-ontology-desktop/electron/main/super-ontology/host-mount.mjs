// The physical mount of a Share (10 section 7.7): `oo serve` records the Share, the host writes `<dir>/.context` atomically as
// 0444 files in 0555 directories, exactly like `oo context mount`, and records the mount in `<state>/mounts.json` so the daemon
// reconciles revoked and expired Shares and appends UPDATES.md. The target path never reaches the engine except as `target_dir`.
import { createHash } from "node:crypto";
import fsDefault from "node:fs/promises";
import { constants } from "node:fs";
import os from "node:os";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";

export const MOUNT_DIR = ".context";
const SUGGEST_DIR = "suggest";
// Byte for byte the text of `oo context mount` (crates/oo-cli/src/docs/suggest.rs README, after Rust's line continuations).
const SUGGEST_README = "# suggest/\n\n这是本挂载里唯一可写的目录。Agent 学到了新东西，把建议写在这里（一个文件一条或多条，*.md 或 *.json），\n守护进程每 10 秒收集一次并把文件改名为 *.collected。\n\nJSON：\n\n```json\n{\"protocol\":\"oo.suggest/v1\",\"kind\":\"new_fact|supersede|correction|preference|type_change\",\n\"subject\":\"<实体名或 key>\",\"predicate\":\"<核心关系或 attr.*>\",\"object\":\"<实体名或 key>\",\"value\":\"…\",\n\"valid_from\":\"YYYY-MM-DD\",\"supersedes\":\"<BRIEF 中的 [src:…] 或 fact_key>\",\n\"rationale\":\"≤500 字\",\"evidence\":[{\"kind\":\"brief_line|ref|external\",\"ref\":\"…\"}]}\n```\n\nobject 与 value 二选一。Markdown：frontmatter 写同样的键（`evidence: ref:路径#位置` 可重复），正文是理由。\n\n限制：单文件 64 KiB，每条建议文件最多 50 条，每个共享最多 200 个文件；不要放符号链接。\n建议不会直接改动文件：新事实有范围内证据时自动接受并进入当日摘要，取代已被使用的事实、改偏好或类型结构时由人确认。\n";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const fail = (code) => new SuperOntologyError(code);
const writeFailed = () => fail("DOCS_SHARE_TARGET_WRITE_FAILED");

/** `rel` is a relative path of plain components only. */
export function safeRelative(rel) {
  return typeof rel === "string" && rel.length > 0 && !/[\\\0]/u.test(rel) && rel.split("/").every((part) => part && part !== "." && part !== "..");
}

/**
 * @param {{ stateDir: string, fetchPackFile: (packId: string, relativePath: string) => Promise<Uint8Array>, fs?: typeof fsDefault, homeDir?: string }} options
 */
export function createHostMount({ stateDir, fetchPackFile, fs = fsDefault, homeDir = os.homedir() }) {
  const mountsFile = path.join(stateDir, "mounts.json");

  async function removeTree(directory) {
    const open = async (target) => {
      let stat;
      try { stat = await fs.lstat(target); } catch { return; }
      if (stat.isSymbolicLink() || !stat.isDirectory()) return;
      await fs.chmod(target, 0o700).catch(() => {});
      for (const entry of await fs.readdir(target).catch(() => [])) await open(path.join(target, entry));
    };
    await open(directory);
    await fs.rm(directory, { recursive: true, force: true }).catch(() => {});
  }

  async function checkTarget(to) {
    if (typeof to !== "string" || !path.isAbsolute(to)) throw fail("DOCS_SHARE_TARGET_INVALID");
    let real;
    try { real = await fs.realpath(to); } catch { throw fail("DOCS_SHARE_TARGET_INVALID"); }
    if (!(await fs.stat(real)).isDirectory() || real === path.parse(real).root) throw fail("DOCS_SHARE_TARGET_INVALID");
    const resolved = async (candidate) => { try { return await fs.realpath(candidate); } catch { return null; } };
    if (real === (await resolved(homeDir))) throw fail("DOCS_SHARE_TARGET_INVALID");
    const state = await resolved(stateDir);
    if (state && (real === state || real.startsWith(state + path.sep))) throw fail("DOCS_SHARE_TARGET_INVALID");
    return real;
  }

  async function exists(target) {
    try { await fs.lstat(target); return true; } catch { return false; }
  }

  async function syncDirectory(directory) {
    const handle = await fs.open(directory, "r");
    try { await handle.sync(); } finally { await handle.close(); }
  }

  async function writeTree(temporary, files) {
    await fs.mkdir(temporary, { mode: 0o700 });
    const directories = new Set();
    for (const [relative, bytes] of files) {
      const file = path.join(temporary, relative);
      await fs.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
      const handle = await fs.open(file, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (constants.O_NOFOLLOW ?? 0), 0o600);
      try { await handle.writeFile(bytes); await handle.sync(); } finally { await handle.close(); }
      await fs.chmod(file, 0o444);
      for (let directory = path.dirname(file); directory !== temporary; directory = path.dirname(directory)) directories.add(directory);
    }
    // Directories last, deepest first: 0555 would block writing into them.
    const nested = [...directories].sort((a, b) => b.split(path.sep).length - a.split(path.sep).length);
    for (const directory of nested) { await syncDirectory(directory); await fs.chmod(directory, 0o555); }
  }

  async function buildInto(target, tag, files, { replacing }) {
    if (files.some(([relative]) => !safeRelative(relative))) throw writeFailed();
    const finalPath = path.join(target, MOUNT_DIR);
    const temporary = path.join(target, `${MOUNT_DIR}.tmp-${tag}`);
    const old = path.join(target, `${MOUNT_DIR}.old-${tag}`);
    await removeTree(temporary);
    if (replacing) await removeTree(old);
    try {
      await writeTree(temporary, files);
      await fs.chmod(temporary, 0o555);
      if (!replacing) {
        await syncDirectory(target);
        if (await exists(finalPath)) throw fail("DOCS_SHARE_TARGET_OCCUPIED");
        await fs.rename(temporary, finalPath);
      } else {
        await fs.rename(finalPath, old);
        try { await fs.rename(temporary, finalPath); } catch (error) { await fs.rename(old, finalPath).catch(() => {}); throw error; }
        await removeTree(old);
      }
      await syncDirectory(target);
    } catch (error) {
      await removeTree(temporary);
      if (error instanceof SuperOntologyError) throw error;
      throw writeFailed();
    }
    // The mount builder leaves every directory 0555; the suggestion directory is the one place an agent may write.
    const suggest = path.join(finalPath, SUGGEST_DIR);
    try { if ((await fs.lstat(suggest)).isDirectory()) await fs.chmod(suggest, 0o755); } catch { /* no suggest directory for a viewer */ }
    return finalPath;
  }

  /** Every file of the pack, checked against its listed sha256 and size, plus SHARE.json, sorted by path. */
  async function packFiles(share) {
    if (typeof share.pack_id !== "string" || typeof share.share_json !== "string") throw writeFailed();
    const files = [];
    for (const entry of Array.isArray(share.files) ? share.files : []) {
      if (!isObject(entry) || typeof entry.path !== "string" || !safeRelative(entry.path)) throw writeFailed();
      const bytes = await fetchPackFile(share.pack_id, entry.path);
      if (sha256(bytes) !== entry.sha256 || entry.bytes !== bytes.byteLength) throw writeFailed();
      files.push([entry.path, Buffer.from(bytes)]);
    }
    files.push(["SHARE.json", Buffer.from(share.share_json, "utf8")]);
    if (["suggester", "editor"].includes(share.role) && !files.some(([relative]) => relative.startsWith(`${SUGGEST_DIR}/`))) files.push([`${SUGGEST_DIR}/README.md`, Buffer.from(SUGGEST_README, "utf8")]);
    files.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
    return files;
  }

  async function loadMounts() {
    try {
      const parsed = JSON.parse(await fs.readFile(mountsFile, "utf8"));
      return isObject(parsed) ? parsed : {};
    } catch { return {}; }
  }

  async function saveMounts(mounts) {
    await fs.mkdir(stateDir, { recursive: true, mode: 0o700 });
    const temporary = `${mountsFile}.tmp`;
    await fs.writeFile(temporary, JSON.stringify(mounts), { mode: 0o600 });
    await fs.rename(temporary, mountsFile);
  }

  async function register(mountRef, shareId, directory) {
    if (typeof mountRef !== "string") return;
    const mounts = await loadMounts();
    for (const [key, value] of Object.entries(mounts)) if (value?.path === directory) delete mounts[key];
    mounts[mountRef] = { share_id: shareId, path: directory, created_at: new Date().toISOString().replace(/\.\d+Z$/u, "Z"), updates_seq: 0 };
    await saveMounts(mounts).catch(() => {});
  }

  async function shareIdIn(target) {
    try { return JSON.parse(await fs.readFile(path.join(target, MOUNT_DIR, "SHARE.json"), "utf8"))?.share_id ?? null; } catch { return null; }
  }

  return Object.freeze({
    checkTarget,
    /** The engine never knows the host path; fills `mount` of each Share from the mount record and the folder itself (present = its SHARE.json is this Share's). */
    async annotate(shares) {
      const mounts = Object.values(await loadMounts());
      return Promise.all(shares.map(async (share) => {
        const record = mounts.find((value) => value?.share_id === share?.id && typeof value.path === "string");
        if (!record) return share;
        return { ...share, mount: { ...(isObject(share.mount) ? share.mount : {}), target_dir: record.path, present: (await shareIdIn(record.path)) === share.id } };
      }));
    },
    /** Validates the target before the engine creates anything (a mount that cannot happen must not leave a Share). */
    async preflight(to) {
      const real = await checkTarget(to);
      if (await exists(path.join(real, MOUNT_DIR))) throw fail("DOCS_SHARE_TARGET_OCCUPIED");
      return real;
    },
    /** Writes the folder of a newly created Share and records it. */
    async install(share, target) {
      const files = await packFiles(share);
      const mounted = await buildInto(target, share.id, files, { replacing: false });
      await register(share.mount_ref, share.id, target);
      return mounted;
    },
    /** The folder a Share was mounted into, from the mount record. */
    async directoryOf(shareId) {
      for (const value of Object.values(await loadMounts())) if (value?.share_id === shareId && typeof value.path === "string") return value.path;
      return null;
    },
    /** Mounts a refreshed Share over the folder of the one it replaces; nothing happens when that folder is not known. */
    async replace(oldShareId, share) {
      const target = await this.directoryOf(oldShareId);
      if (!target) return null;
      if ((await shareIdIn(target)) !== oldShareId) throw fail("DOCS_SHARE_TARGET_OCCUPIED");
      const files = await packFiles(share);
      const mounted = await buildInto(target, share.id, files, { replacing: true });
      await register(share.mount_ref, share.id, target);
      return mounted;
    },
    /** Deletes the folder of a revoked Share when its SHARE.json is ours, and its record. */
    async remove(shareId) {
      const target = await this.directoryOf(shareId);
      let removed = false;
      if (target) {
        const directory = path.join(target, MOUNT_DIR);
        let link = false;
        try { link = (await fs.lstat(directory)).isSymbolicLink(); } catch { /* gone already */ }
        if (!link && (await shareIdIn(target)) === shareId) { await removeTree(directory); removed = !(await exists(directory)); }
      }
      const mounts = await loadMounts();
      const kept = Object.fromEntries(Object.entries(mounts).filter(([, value]) => value?.share_id !== shareId));
      if (Object.keys(kept).length !== Object.keys(mounts).length) await saveMounts(kept).catch(() => {});
      return removed;
    },
  });
}
