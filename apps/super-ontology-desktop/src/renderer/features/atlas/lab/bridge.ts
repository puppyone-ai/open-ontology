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
  let source = fakeAtlasSource(initial, initial.groups.length === 1 && initial.groups[0]!.kind === "db" ? { databaseName: "shop", drives: [] } : {});
  const listeners = new Set<(push: SuperOntologyPush) => void>();
  const calls: Record<string, number> = {};
  let seq = 0;
  const ok = (value: unknown) => ({ ok: true as const, value });
  const empty = { items: [], next_cursor: null, as_of_seq: 0 };
  /** The schema answer (docs-api 1.12) of the fake catalog, paged like the real route. */
  const schema = async (limit: number, cursor: string | undefined) => {
    const catalog = await source.catalog();
    if (!catalog.ok) return catalog;
    const start = cursor?.startsWith("t") ? Number(cursor.slice(1)) : 0;
    const page = catalog.value.tables.slice(start, start + limit);
    const names = new Set(page.map((t) => `${t.schema}.${t.table}`));
    const end = (t: { schema: string; table: string }) => ({ table: `${t.schema}.${t.table}`, urn: `oo:db/lab/${t.schema}.${t.table}`, columns: ["id"] });
    return ok({
      protocol: "oo.ontology.schema/v1", source: { kind: "database", profile: "lab" },
      tables: page.map((t) => ({ urn: `oo:db/lab/${t.schema}.${t.table}`, qualified_name: `${t.schema}.${t.table}`, schema: t.schema, name: t.table, column_count: 1, columns_status: "ok", columns: [{ urn: `oo:db/lab/${t.schema}.${t.table}#id`, name: "id", position: 1, type: "int8", type_oid: 20 }] })),
      relationships: catalog.value.foreignKeys.filter((fk) => names.has(`${fk.from.schema}.${fk.from.table}`)).map((fk, i) => ({ urn: `oo:db/lab/relationship/fk_${start}_${i}`, id: `fk_${start}_${i}`, from: end(fk.from), to: end(fk.to), cardinality: "many_to_one", provenance: "declared_fk" })),
      doc_links: [], total_tables: catalog.value.tables.length, next_cursor: start + limit < catalog.value.tables.length ? `t${start + limit}` : null,
      sides: { db: { status: "ok", count: page.length }, relationships: { status: "ok", count: 0 }, docs: { status: "skipped", count: 0 } }, warnings: [], provenance: { kind: "gateway_schema", source: "lab" },
    });
  };
  const bridge: SuperOntologyBridge = {
    invoke: (async (channel: string, request: Record<string, unknown> = {}) => {
      calls[channel] = (calls[channel] ?? 0) + 1;
      switch (channel) {
        case "engine-read": case "engine-preflight": case "engine-start": return ok(snapshot);
        case "drives-list": return source.drives();
        case "files-list": return source.files({ driveId: String(request.driveId), limit: Number(request.limit ?? 200), cursor: request.cursor as string | undefined });
        case "cli-db-list": return ok({ ok: true, data: { sources: initial.groups.length === 1 && initial.groups[0]!.kind === "db" ? [{ name: "shop" }] : [] } });
        case "events-subscribe": return ok({ instanceId: "lab", lastSeq: 0, connection: snapshot.connection });
        case "entities-search": return source.entities({ limit: Number(request.limit ?? 100), cursor: request.cursor as string | undefined });
        case "entity-detail": return source.entity(String(request.entityIdOrKey));
        case "ontology-versions": return source.versions();
        case "atlas-links": return source.links();
        case "ontology-schema": return schema(Number(request.limit ?? 50), request.cursor as string | undefined);
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
