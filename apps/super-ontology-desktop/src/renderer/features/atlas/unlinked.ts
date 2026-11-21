import { type AtlasData, type AtlasGroup, type AtlasMember } from "./model";
import type { UnlinkedReport } from "../../model/unlinked";
import { entityId, fileId } from "./adapter";

/** The id of the side group. The label is set by the screen, in the language of the app. */
export const UNLINKED_GROUP_ID = "g:unlinked";
/** The one entry that stands for all unsupported files of a big drop (a code folder). Its label is set by the screen (it carries a count). */
export const UNSUPPORTED_SUMMARY_ID = "f:unsupported-summary";
/** More unsupported files than this become one entry with a count; a few stay as rows. */
export const UNSUPPORTED_ROWS_MAX = 8;
const FORMATS_NAMED = 3;

/**
 * Moves what is not connected into one side group (25 section 7.3): isolated entities (type view) and files with a reason (both views).
 * A file the view did not list (type view shows entities only) is added to it. Groups left empty are dropped; nothing else changes.
 */
export function applyUnlinked(data: AtlasData, report: UnlinkedReport | null): AtlasData {
  if (!report || (report.files.length === 0 && report.entities.length === 0)) return data;
  const members = new Map(data.members.map((member) => [member.id, member]));
  const moved: AtlasMember[] = [];
  const typeView = data.members.some((member) => member.ref.kind === "entity");
  if (typeView) {
    for (const entity of report.entities) {
      const member = members.get(entityId(entity.key));
      if (member) { members.delete(member.id); moved.push({ ...member, group: UNLINKED_GROUP_ID, reason: { kind: "entity", ...(entity.fileNames?.length ? { file: entity.fileNames.slice(0, 2).join(", ") } : {}) } }); }
    }
  }
  // A code folder is mostly files the engine does not read: one entry with a count and the biggest formats, not one row per file.
  const unsupported = report.files.filter((file) => file.reason.kind === "unsupported");
  const collapse = unsupported.length > UNSUPPORTED_ROWS_MAX;
  if (collapse) {
    const formats = new Map<string, number>();
    for (const file of unsupported) { const format = file.reason.kind === "unsupported" ? file.reason.format : "?"; formats.set(format, (formats.get(format) ?? 0) + 1); }
    const named = [...formats.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, FORMATS_NAMED).map(([format, count]) => `${format} ×${count}`).join(", ");
    const first = unsupported[0]!;
    for (const file of unsupported) members.delete(fileId(file.fileId));
    moved.push({ id: UNSUPPORTED_SUMMARY_ID, label: named, ref: { kind: "file", fileId: first.fileId, path: first.path }, weight: 0, born: null, group: UNLINKED_GROUP_ID, reason: { kind: "unsupported_group", count: unsupported.length, formats: named } });
  }
  for (const file of report.files) {
    if (collapse && file.reason.kind === "unsupported") continue;
    const existing = members.get(fileId(file.fileId));
    if (existing) members.delete(existing.id);
    moved.push({ ...(existing ?? { id: fileId(file.fileId), label: file.name, ref: { kind: "file" as const, fileId: file.fileId, path: file.path }, weight: 0, born: null }), group: UNLINKED_GROUP_ID, reason: file.reason });
  }
  if (moved.length === 0) return data;
  const rest = [...members.values()];
  const live = new Set(rest.map((member) => member.group));
  const group: AtlasGroup = { id: UNLINKED_GROUP_ID, kind: "unlinked", label: "", tone: "stone", source: undefined };
  return { ...data, groups: [...data.groups.filter((g) => live.has(g.id) || g.kind === "db"), group], members: [...rest, ...moved] };
}

/** How many things the side group holds. */
export const unlinkedCount = (data: AtlasData): number => data.members.reduce((n, m) => n + (m.group === UNLINKED_GROUP_ID ? (m.reason?.kind === "unsupported_group" ? m.reason.count : 1) : 0), 0);
