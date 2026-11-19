import type { SuperOntologyBridge, SuperOntologyPush, EngineEvent } from "../../../../shared/super-ontology/bridge";
import type { AtlasData } from "../model";
import { fakeAtlasSource } from "./fixtures";

/**
 * A stand-in for the preload bridge that serves a synthetic corpus (lab/fixtures.ts) through the real channels the atlas reads. Only the
 * measuring and screenshot pages install it; the shipped renderer never imports this file.
 */
export type LabBridge = { bridge: SuperOntologyBridge; push(events: EngineEvent[]): void; replace(data: AtlasData): void; calls: Record<string, number> };

const snapshot = { revision: 1, status: "healthy", reason: null, code: null, preflight: null, health: { events_head: 0 }, restarts: { count: 0, nextAttemptAt: null }, ownedByDesktop: true, keepEngineOnQuit: false, startedAt: null, mode: "local", remote: null, connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 } };

export function createLabBridge(initial: AtlasData): LabBridge {
  let source = fakeAtlasSource(initial);
  const listeners = new Set<(push: SuperOntologyPush) => void>();
  const calls: Record<string, number> = {};
  let seq = 0;
  const ok = (value: unknown) => ({ ok: true as const, value });
  const empty = { items: [], next_cursor: null, as_of_seq: 0 };
  const sql = async (text: string) => {
    const catalog = await source.catalog();
    if (!catalog.ok) return catalog;
    const rows = /table_constraints/u.test(text)
      ? catalog.value.foreignKeys.map((fk) => [fk.from.schema, fk.from.table, fk.to.schema, fk.to.table])
      : catalog.value.tables.map((t) => [t.schema, t.table]);
    return ok({ columns: [], rows, protocol: "oo.ontology.query/v1" });
  };
  const bridge: SuperOntologyBridge = {
    invoke: (async (channel: string, request: Record<string, unknown> = {}) => {
      calls[channel] = (calls[channel] ?? 0) + 1;
      switch (channel) {
        case "engine-read": case "engine-preflight": case "engine-start": return ok(snapshot);
        case "drives-list": return source.drives();
        case "files-list": return source.files({ driveId: String(request.driveId), limit: Number(request.limit ?? 200), cursor: request.cursor as string | undefined });
        case "events-subscribe": return ok({ instanceId: "lab", lastSeq: 0, connection: snapshot.connection });
        case "entities-search": return source.entities({ limit: Number(request.limit ?? 100), cursor: request.cursor as string | undefined });
        case "entity-detail": return source.entity(String(request.entityIdOrKey));
        case "ontology-versions": return source.versions();
        case "atlas-links": return source.links();
        case "atlas-sql": return sql(String(request.sql));
        default: return ok(empty);
      }
    }) as SuperOntologyBridge["invoke"],
    subscribe: (callback) => { listeners.add(callback); return () => { listeners.delete(callback); }; },
    pathsForFiles: () => ({ paths: [], unreadable: 0 }),
    openExternal: async () => undefined,
  };
  return {
    bridge,
    calls,
    push: (events) => { seq += events.length; for (const listener of [...listeners]) listener({ channel: "events", payload: { instanceId: "lab", events: events.map((event, i) => ({ seq: seq - events.length + i + 1, ...event })), lastSeq: seq } }); },
    replace: (data) => { source = fakeAtlasSource(data); },
  };
}
