import fs from "node:fs/promises";
import path from "node:path";
import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";

/** File types whose lines Desktop edits as text. Everything else is refused: a suggestion must not corrupt a binary. */
const TEXT_FILE = /\.(md|markdown|txt)$/iu;
const MAX_WRITEBACK_BYTES = 8 * 1024 * 1024;
const TOLERANCE_MS = 2_000;

const isInside = (parent, child) => {
  const relative = path.relative(parent, child);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
};

/** The 1-based inclusive line range a change points at, from its anchor or its `#L3-L5` fragment. */
export function lineRangeOf(change) {
  const anchor = change.anchor;
  if (anchor && Number.isSafeInteger(anchor.line_start) && anchor.line_start >= 1) {
    const end = Number.isSafeInteger(anchor.line_end) && anchor.line_end >= anchor.line_start ? anchor.line_end : anchor.line_start;
    return { start: anchor.line_start, end };
  }
  const fragment = /#L(\d+)(?:-L?(\d+))?$/u.exec(String(change.frag ?? ""));
  if (fragment) {
    const start = Number(fragment[1]);
    return { start, end: fragment[2] ? Number(fragment[2]) : start };
  }
  return null;
}

/**
 * Applies the changes of one file to its text and returns the new text. `replace` swaps the anchored lines for `after`,
 * `insert_after` puts `after` as a new line behind the anchored lines; changes are applied from the bottom so that earlier line
 * numbers stay valid. `annotate` has no place in a file and is left to the ontology (reported as skipped).
 */
export function applyChanges(text, changes) {
  const newline = text.includes("\r\n") ? "\r\n" : "\n";
  const endsWithNewline = text.endsWith("\n");
  const lines = text.split(/\r?\n/u);
  if (endsWithNewline) lines.pop();
  const skipped = [];
  const planned = [];
  for (const change of changes) {
    if (change.operation === "annotate") { skipped.push({ reason: "annotate", block_id: change.block_id ?? null }); continue; }
    if (change.operation !== "replace" && change.operation !== "insert_after") throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    const range = lineRangeOf(change);
    if (!range || range.end > lines.length || typeof change.after !== "string" || /[\r\n]/u.test(change.after)) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    planned.push({ ...change, range });
  }
  planned.sort((left, right) => right.range.start - left.range.start);
  for (let index = 1; index < planned.length; index += 1) {
    if (planned[index].range.end >= planned[index - 1].range.start) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED"); // Overlapping edits: refuse, a person decides.
  }
  const touched = [];
  for (const change of planned) {
    if (change.operation === "replace") lines.splice(change.range.start - 1, change.range.end - change.range.start + 1, change.after);
    else lines.splice(change.range.end, 0, change.after);
    touched.push({ block_id: change.block_id ?? null, operation: change.operation, first_line: change.range.start });
  }
  return { text: lines.join(newline) + (endsWithNewline || lines.length === 0 ? newline : ""), touched, skipped };
}

/**
 * Writes accepted suggestions of an `editor` Share back into the files of a folder Drive (F-S8-03, 20-frontend §3.5.4, 10 §11.1:
 * "OO never writes the Drive"). `prepare` checks everything and builds the new contents without touching a file; `commit` writes
 * each file through a temporary name the engine ignores and an atomic rename, then asks for a targeted sync. A file that changed
 * since the engine read it is a conflict: nothing is written and the person is asked to review again.
 * `onWritten` is the hook for the Git auto-commit flow (the workspace watcher sees the change; the host may also nudge it).
 */
export function createWriteback({ client, fsPromises = fs, journalFile = null, onWritten = async () => undefined, now = () => new Date(), logger = console } = {}) {
  const call = (name, options) => client.request(name, options);

  async function journal(entry) {
    if (!journalFile) return;
    try {
      await fsPromises.mkdir(path.dirname(journalFile), { recursive: true, mode: 0o700 });
      await fsPromises.appendFile(journalFile, `${JSON.stringify({ at: now().toISOString(), ...entry })}\n`, { mode: 0o600 });
    } catch (error) {
      logger.debug?.("Super Drive write-back journal could not be written.", { code: error?.code });
    }
  }

  async function prepare({ suggestion, diff }) {
    if (diff.writable !== true || diff.role !== "editor" || diff.writer !== "desktop") throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    const driveId = suggestion.drive_id;
    const { value: drive } = await call("driveGet", { params: { id: driveId } });
    const root = drive?.config?.root;
    if (drive?.backend !== "fs" || typeof root !== "string" || !path.isAbsolute(root)) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    const realRoot = await fsPromises.realpath(root).catch(() => { throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED"); });
    const byPath = new Map();
    for (const change of diff.changes ?? []) {
      if (typeof change.path !== "string" || !TEXT_FILE.test(change.path)) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
      byPath.set(change.path, [...(byPath.get(change.path) ?? []), change]);
    }
    if (byPath.size === 0) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    const files = [];
    for (const [relative, changes] of byPath) {
      const absolute = path.join(realRoot, ...relative.split("/"));
      if (!isInside(realRoot, absolute)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const real = await fsPromises.realpath(absolute).catch(() => { throw new SuperOntologyError("SO_WRITEBACK_CONFLICT"); });
      if (!isInside(realRoot, real)) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
      const stat = await fsPromises.stat(real);
      if (!stat.isFile() || stat.size > MAX_WRITEBACK_BYTES) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
      // The engine must still hold the version the suggestion was made against, and the file on disk must not be newer than what the engine read.
      const { value: file } = await call("fileDetail", { params: { fid: changes[0].file_id }, query: {} });
      const expected = changes[0].source_b3;
      if (typeof expected === "string" && typeof file?.b3 === "string" && file.b3 !== expected) throw new SuperOntologyError("SO_WRITEBACK_CONFLICT");
      const modified = Date.parse(file?.modified_at ?? "");
      if (Number.isFinite(modified) && stat.mtimeMs > modified + TOLERANCE_MS) throw new SuperOntologyError("SO_WRITEBACK_CONFLICT");
      const before = await fsPromises.readFile(real, "utf8");
      const result = applyChanges(before, changes);
      if (result.touched.length === 0) continue;
      files.push({ relative, real, mode: stat.mode & 0o777, mtimeMs: stat.mtimeMs, size: stat.size, before, ...result });
    }
    if (files.length === 0) throw new SuperOntologyError("SO_WRITEBACK_UNSUPPORTED");
    return { driveId, root: realRoot, files };
  }

  async function commit(plan, { suggestion, diff }) {
    const written = [];
    try {
      for (const file of plan.files) {
        const current = await fsPromises.stat(file.real);
        if (current.mtimeMs !== file.mtimeMs || current.size !== file.size) throw new SuperOntologyError("SO_WRITEBACK_CONFLICT"); // Changed between the check and the write.
        const temporary = path.join(path.dirname(file.real), `.so-writeback-${suggestion.id.slice(0, 8)}.part`);
        try {
          await fsPromises.writeFile(temporary, file.text, { mode: file.mode, flag: "wx" });
          await fsPromises.rename(temporary, file.real);
        } catch (error) {
          await fsPromises.rm(temporary, { force: true }).catch(() => undefined);
          if (error instanceof SuperOntologyError) throw error;
          throw new SuperOntologyError(error?.code === "ENOSPC" ? "SO_UPLOAD_DISK_FULL" : error?.code === "EACCES" || error?.code === "EPERM" ? "SO_UPLOAD_PERMISSION_DENIED" : "SO_WRITEBACK_FAILED");
        }
        written.push(file);
      }
    } catch (error) {
      // Put back what was already replaced, so a half-applied suggestion never stays on disk.
      await Promise.all(written.map((file) => fsPromises.writeFile(file.real, file.before, { mode: file.mode }).catch(() => undefined)));
      throw error;
    }
    const evidence = (diff.changes ?? []).map((change) => change.before_ref).filter(Boolean);
    for (const file of written) {
      await journal({ event: "written", suggestion_id: suggestion.id, share_id: diff.share_id, drive_id: plan.driveId, path: file.relative, kind: diff.kind, rule: diff.rule, operations: file.touched, skipped: file.skipped, evidence, credential_id: suggestion.credential_id ?? null, submitted_by: suggestion.submitted_by ?? null, role: diff.role });
      await onWritten({ driveId: plan.driveId, root: plan.root, relativePath: file.relative, suggestionId: suggestion.id }).catch((error) => logger.debug?.("Super Drive write-back hook failed.", { code: error?.code }));
    }
    try {
      await call("driveSync", { params: { id: plan.driveId }, body: { paths: written.map((file) => file.relative) }, operationId: `writeback-${suggestion.id.replaceAll("-", "")}` });
    } catch (error) {
      logger.debug?.("Super Drive write-back sync request failed.", { code: error?.code }); // The watcher and the poll find the change.
    }
    return { written: written.map((file) => ({ path: file.relative, operations: file.touched, skipped: file.skipped })), evidence };
  }

  async function revert(plan) {
    await Promise.all(plan.files.map((file) => fsPromises.writeFile(file.real, file.before, { mode: file.mode }).catch(() => undefined)));
  }

  return Object.freeze({ prepare, commit, revert, journal });
}
