import type { FileRecord } from "../superDriveState";
import type { FileDetailDoc } from "./types";

/** Path comparison that ignores what a case-insensitive or normalizing store would also ignore: Unicode form and case. */
export const collisionKey = (path: string) => path.normalize("NFC").toLowerCase();

/** The other file a `DOCS_SOURCE_PATH_COLLISION` file clashes with, found among the files on screen (the engine names it only in the code, not in the document). */
export function findPathTwin(files: Readonly<Record<string, FileRecord>>, detail: Pick<FileDetailDoc, "id" | "drive_id" | "path">): FileRecord | null {
  const key = collisionKey(detail.path);
  for (const record of Object.values(files)) {
    if (record.id !== detail.id && record.drive_id === detail.drive_id && record.path !== detail.path && collisionKey(record.path) === key) return record;
  }
  return null;
}

/** For a replaced file: the file at the same path that is not replaced, when it is on screen. */
export function findCurrentVersion(files: Readonly<Record<string, FileRecord>>, detail: Pick<FileDetailDoc, "id" | "drive_id" | "path">): FileRecord | null {
  for (const record of Object.values(files)) {
    if (record.id !== detail.id && record.drive_id === detail.drive_id && record.path === detail.path && record.state !== "superseded" && record.state !== "deleted") return record;
  }
  return null;
}

/** The state a removed file was in before it went: the entry before the last `deleted`. */
export function lastStateBeforeRemoval(history: FileDetailDoc["history"]): string | null {
  const entries = history ?? [];
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    if (entries[index]?.to === "deleted") return entries[index]?.from ?? entries[index - 1]?.to ?? null;
  }
  return null;
}
