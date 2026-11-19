import type { EngineEvent } from "../../../shared/super-ontology/bridge";
import { driveGroupId, fileId } from "./adapter";
import type { AtlasMember, AtlasMode, AtlasOp } from "./model";

/**
 * Engine events to atlas operations (25 section 7.3): a new file buds a cell in its source, a changed one pulses once, a removed
 * one shrinks and fades, a new ontology version makes the whole picture breathe. Events that mean "the data moved" ask for a reload;
 * the reload is diffed against what is drawn, so entities and tables that appeared behind it bud as well.
 */
export type EventContext = {
  mode: AtlasMode;
  hasMember(id: string): boolean;
  hasGroup(id: string): boolean;
};

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const str = (value: unknown): string | null => (typeof value === "string" && value.length > 0 ? value : null);

/** States in which a file is no longer part of the picture. */
const GONE = new Set(["deleted", "superseded", "duplicate"]);

export function opsForEvent(event: EngineEvent, context: EventContext): AtlasOp[] {
  const data = isRecord(event.data) ? event.data : {};
  switch (event.type) {
    case "file.state_changed": {
      const file = isRecord(data.file) ? data.file : null;
      const id = file ? str(file.id) : null;
      const drive = file ? str(file.drive_id) : null;
      const path = file ? str(file.path) : null;
      if (!file || !id || !drive || !path || context.mode !== "source") return [];
      const member = fileId(id);
      if (GONE.has(String(file.state))) return context.hasMember(member) ? [{ type: "retract", id: member }] : [];
      if (context.hasMember(member)) return [{ type: "pulse", id: member }];
      const group = driveGroupId(drive);
      const cell: AtlasMember = { id: member, group, label: path.split("/").pop() || path, ref: { kind: "file", fileId: id, path }, weight: 0, born: null };
      return [{ type: "bud", member: cell, group: context.hasGroup(group) ? undefined : { id: group, kind: "drive", label: drive.slice(0, 8), tone: "ink" } }];
    }
    case "file.removed": {
      const id = str(data.file_id);
      return id && context.mode === "source" && context.hasMember(fileId(id)) ? [{ type: "retract", id: fileId(id) }] : [];
    }
    case "file.renamed": {
      const id = str(data.file_id);
      return id && context.mode === "source" && context.hasMember(fileId(id)) ? [{ type: "pulse", id: fileId(id) }] : [];
    }
    // The picture changed in ways the event does not describe: read it again and let the diff say what budded or faded.
    case "files.bulk_changed":
    case "drive.sync_completed":
    case "review.decided":
    case "drive.created":
    case "drive.deleted":
      return [{ type: "refresh" }];
    case "ontology.version_published":
      return [{ type: "breathe" }, { type: "refresh" }];
    default:
      return [];
  }
}

/** More cell changes than this in one batch is a flood, not a story. */
export const FLOOD = 150;

/** Collapses many events into the operations to run now: a refresh is asked once, however many events wanted it. */
export function opsForEvents(events: readonly EngineEvent[], context: EventContext): AtlasOp[] {
  const ops: AtlasOp[] = [];
  let refresh = false;
  let breathe = false;
  for (const event of events) {
    for (const op of opsForEvent(event, context)) {
      if (op.type === "refresh") refresh = true;
      else if (op.type === "breathe") breathe = true;
      else ops.push(op);
    }
  }
  // A cold start reports thousands of files at once: budding them one by one would be noise, so the reload and its diff take over.
  if (ops.length > FLOOD) return [{ type: "breathe" }, { type: "refresh" }];
  if (breathe) ops.push({ type: "breathe" });
  if (refresh) ops.push({ type: "refresh" });
  return ops;
}
