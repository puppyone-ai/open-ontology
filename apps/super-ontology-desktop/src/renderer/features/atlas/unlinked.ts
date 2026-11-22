import { type AtlasData, type AtlasGroup, type AtlasMember } from "./model";
import type { GroupedKind, UnlinkedReport } from "../../model/unlinked";
import { entityId, fileId } from "./adapter";

/** The id of the side group. The label is set by the screen, in the language of the app. */
export const UNLINKED_GROUP_ID = "g:unlinked";
/** The id prefix of the entries that stand for many files with one reason (a code folder); their label is set by the screen, they carry a count. */
export const GROUP_ENTRY_PREFIX = "f:group:";
/** More files than this with one reason become one entry with a count; a few stay as rows. */
export const GROUP_ROWS_MAX = 8;
const FORMATS_NAMED = 3;
const GROUPED: readonly GroupedKind[] = ["unsupported", "processing", "indexed_only", "review", "parse_failed", "too_large", "encrypted"];

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
  // A code folder is mostly files the engine does not read, is still working through, or finds nothing in: one entry per reason with a count, not one row each.
  const collapsed = new Set<string>();
  for (const kind of GROUPED) {
    const same = report.files.filter((file) => file.reason.kind === kind);
    if (same.length <= GROUP_ROWS_MAX) continue;
    let detail: string | undefined;
    if (kind === "unsupported") {
      const formats = new Map<string, number>();
      for (const file of same) { const format = file.reason.kind === "unsupported" ? file.reason.format : "?"; formats.set(format, (formats.get(format) ?? 0) + 1); }
      detail = [...formats.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, FORMATS_NAMED).map(([format, count]) => `${format} ×${count}`).join(", ");
    }
    const first = same[0]!;
    for (const file of same) { members.delete(fileId(file.fileId)); collapsed.add(file.fileId); }
    moved.push({ id: `${GROUP_ENTRY_PREFIX}${kind}`, label: detail ?? first.name, ref: { kind: "file", fileId: first.fileId, path: first.path }, weight: 0, born: null, group: UNLINKED_GROUP_ID, reason: { kind: "group", of: kind, count: same.length, ...(detail ? { detail } : {}) } });
  }
  for (const file of report.files) {
    if (collapsed.has(file.fileId)) continue;
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
export const unlinkedCount = (data: AtlasData): number => data.members.reduce((n, m) => n + (m.group === UNLINKED_GROUP_ID ? (m.reason?.kind === "group" ? m.reason.count : 1) : 0), 0);
