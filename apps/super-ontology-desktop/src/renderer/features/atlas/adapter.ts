import type { Drive, FileSummary, LinkList, OntologyVersion, Page } from "../../../shared/super-ontology/docs-api.generated";
import type { ApiResult } from "../../model/bridgeInvoke";
import type { EntityDetail, EntitySearchRow } from "../../model/superDriveOntologyTypes";
import type { AtlasData, AtlasGroup, AtlasLink, AtlasMember, AtlasMode, AtlasOp, AtlasPartial, AtlasTone } from "./model";

/**
 * The data adapter of the atlas: turns what the engine and the database side already answer into groups, members and links.
 * The engine is asked for nothing new: the entity listing, the entity page (facts whose object is another entity), the
 * document to table links, the Drive and file listings, and the read-only query surface for tables and foreign keys.
 * A side that fails is left out and named in `partial`; the rest still shows (part of the "partial success" state of D86).
 */
export type DbTable = { schema: string; table: string };
export type DbForeignKey = { from: DbTable; to: DbTable };
export type DbCatalog = { tables: DbTable[]; foreignKeys: DbForeignKey[] };

export type AtlasSource = {
  entities(input: { limit: number; cursor?: string }): Promise<ApiResult<Page<EntitySearchRow>>>;
  entity(key: string): Promise<ApiResult<EntityDetail>>;
  versions(): Promise<ApiResult<Page<OntologyVersion>>>;
  drives(): Promise<ApiResult<Drive[]>>;
  files(input: { driveId: string; limit: number; cursor?: string }): Promise<ApiResult<Page<FileSummary>>>;
  links(): Promise<ApiResult<LinkList>>;
  /** `null` when the database side is not reachable or not configured. */
  catalog(): Promise<ApiResult<DbCatalog>>;
};

export type LoadOptions = {
  mode: AtlasMode;
  /** Upper bounds, so a very large ontology still loads in bounded time; hitting one is reported as `truncated`. */
  maxEntities?: number;
  maxFiles?: number;
  /** Entities whose page is read to find relations (the most connected first), and how many are read at once. */
  relationBudget?: number;
  concurrency?: number;
  signal?: { aborted: boolean };
  onProgress?: (data: AtlasData) => void;
};

export type LoadResult = { data: AtlasData; truncated: boolean };

const TYPE_TONE: Record<string, AtlasTone> = { Person: "ink", Organization: "moss", Project: "sand", Decision: "clay", Document: "stone", Topic: "ink", Event: "moss", Term: "sand", Thing: "stone" };
const DB_TONES: AtlasTone[] = ["moss", "stone"];
const PAGE = 200;

export const entityId = (key: string) => `e:${key}`;
export const fileId = (id: string) => `f:${id}`;
export const tableId = (table: DbTable) => `t:${table.schema}.${table.table}`;
export const typeGroupId = (type: string) => `g:type:${type}`;
export const driveGroupId = (id: string) => `g:drive:${id}`;
export const dbGroupId = (schema: string) => `g:db:${schema}`;

const factsOf = (row: EntitySearchRow): number => row.counts?.facts ?? (typeof row.facts === "number" ? row.facts : 0);
const sinceOf = (row: EntitySearchRow): number | null => {
  const value = (row as { since_version?: unknown }).since_version;
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
};

async function pages<T>(read: (cursor?: string) => Promise<ApiResult<Page<T>>>, limit: number, signal?: { aborted: boolean }): Promise<{ items: T[]; ok: boolean; truncated: boolean }> {
  const items: T[] = [];
  let cursor: string | undefined;
  for (;;) {
    if (signal?.aborted) return { items, ok: true, truncated: false };
    const result = await read(cursor);
    if (!result.ok) return { items, ok: items.length > 0, truncated: false };
    items.push(...result.value.items);
    if (!result.value.next_cursor) return { items, ok: true, truncated: false };
    if (items.length >= limit) return { items: items.slice(0, limit), ok: true, truncated: true };
    cursor = result.value.next_cursor;
  }
}

/** Runs `work` over `items` with at most `width` in flight; order of completion does not matter to the caller. */
export async function pool<T>(items: readonly T[], width: number, work: (item: T) => Promise<void>, signal?: { aborted: boolean }): Promise<void> {
  let next = 0;
  const lane = async () => { while (next < items.length && !signal?.aborted) { const item = items[next]!; next += 1; await work(item); } };
  await Promise.all(Array.from({ length: Math.min(width, items.length) }, lane));
}

export async function loadAtlas(source: AtlasSource, options: LoadOptions): Promise<LoadResult> {
  const { mode, signal } = options;
  const maxEntities = options.maxEntities ?? 3000;
  const maxFiles = options.maxFiles ?? 5000;
  const partial = new Set<AtlasPartial>();
  const groups = new Map<string, AtlasGroup>();
  const members: AtlasMember[] = [];
  const links = new Map<string, AtlasLink>();
  let truncated = false;
  const emit = () => options.onProgress?.({ groups: [...groups.values()], members: [...members], links: [...links.values()], versions, partial: [...partial] });
  const versions: { n: number; asOf: string }[] = [];

  const versionResult = await source.versions();
  if (versionResult.ok) for (const version of versionResult.value.items) versions.push({ n: version.n, asOf: version.as_of });
  versions.sort((a, b) => a.n - b.n);

  // Documents side.
  if (mode === "type") {
    const found = await pages((cursor) => source.entities({ limit: PAGE, cursor }), maxEntities, signal);
    if (!found.ok) partial.add("entities");
    truncated ||= found.truncated;
    for (const row of found.items) {
      const gid = typeGroupId(row.type);
      if (!groups.has(gid)) groups.set(gid, { id: gid, kind: "type", label: row.type, tone: TYPE_TONE[row.type] ?? "stone" });
      members.push({ id: entityId(row.key), group: gid, label: row.name, ref: { kind: "entity", key: row.key }, weight: factsOf(row), born: sinceOf(row) });
    }
  } else {
    const drives = await source.drives();
    if (!drives.ok) partial.add("files");
    else {
      let budget = maxFiles;
      for (const drive of drives.value) {
        if (drive.status === "deleted" || signal?.aborted) continue;
        const gid = driveGroupId(drive.id);
        groups.set(gid, { id: gid, kind: "drive", label: drive.name, tone: "ink" });
        const found = await pages((cursor) => source.files({ driveId: drive.id, limit: PAGE, cursor }), budget, signal);
        if (!found.ok) partial.add("files");
        truncated ||= found.truncated;
        budget -= found.items.length;
        for (const file of found.items) {
          if (file.state === "deleted" || file.state === "superseded") continue;
          members.push({ id: fileId(file.id), group: gid, label: file.path.split("/").pop() || file.path, ref: { kind: "file", fileId: file.id, path: file.path }, weight: 0, born: null });
        }
      }
    }
  }
  emit();

  // Database side: tables are members of their schema, foreign keys are links.
  const tableIds = new Map<string, string>();
  const catalog = await source.catalog();
  if (catalog.ok) {
    const schemas = [...new Set(catalog.value.tables.map((t) => t.schema))].sort();
    schemas.forEach((schema, index) => groups.set(dbGroupId(schema), { id: dbGroupId(schema), kind: "db", label: schema, tone: DB_TONES[index % DB_TONES.length]! }));
    for (const table of catalog.value.tables) {
      const id = tableId(table);
      tableIds.set(`${table.schema}.${table.table}`, id);
      members.push({ id, group: dbGroupId(table.schema), label: table.table, ref: { kind: "table", table: `${table.schema}.${table.table}` }, weight: 0, born: null });
    }
    for (const fk of catalog.value.foreignKeys) addLink(links, tableId(fk.from), tableId(fk.to), "foreign_key");
  } else if (catalog.error.code !== "ONTOLOGY_NOT_CONFIGURED") partial.add("tables");
  emit();

  // Document entity to table row links.
  if (mode === "type") {
    const linkList = await source.links();
    if (linkList.ok) {
      const bySuffix = new Map<string, string | null>();
      for (const [name, id] of tableIds) { const short = name.slice(name.indexOf(".") + 1); bySuffix.set(short, bySuffix.has(short) ? null : id); }
      for (const item of linkList.value.items) {
        if (item.status === "rejected") continue;
        const target = tableIds.get(item.db_table) ?? bySuffix.get(item.db_table) ?? null;
        if (target) addLink(links, entityId(item.doc_entity_key), target, "link");
      }
    } else partial.add("links");
    emit();

    // Relations: facts whose object is another entity. Only the most connected entities are read, a few at a time.
    const known = new Set(members.map((m) => m.id));
    const queue = members.filter((m) => m.ref.kind === "entity" && m.weight > 0).sort((a, b) => b.weight - a.weight).slice(0, options.relationBudget ?? 150);
    let failed = 0;
    await pool(queue, options.concurrency ?? 6, async (member) => {
      const detail = await source.entity((member.ref as { key: string }).key);
      if (!detail.ok) { failed += 1; return; }
      for (const fact of detail.value.facts ?? []) {
        if (fact.status === "retracted" || fact.status === "rejected" || !fact.object) continue;
        const subject = fact.subject?.key;
        const object = fact.object.key;
        if (!subject || !object) continue;
        const a = entityId(subject);
        const b = entityId(object);
        if (known.has(a) && known.has(b)) addLink(links, a, b, "fact");
      }
    }, signal);
    if (failed > 0) partial.add("relations");
  }
  emit();
  return { data: { groups: [...groups.values()], members, links: [...links.values()], versions, partial: [...partial] }, truncated };
}

function addLink(links: Map<string, AtlasLink>, a: string, b: string, kind: AtlasLink["kind"]): void {
  if (a === b) return;
  const [lo, hi] = a < b ? [a, b] : [b, a];
  const key = `${kind}|${lo}|${hi}`;
  const existing = links.get(key);
  if (existing) existing.weight += 1; else links.set(key, { a: lo, b: hi, kind, weight: 1 });
}

/** What changed between two loads, as scene operations: new cells bud, vanished ones retract, a changed weight pulses. */
export function diffAtlas(prev: AtlasData, next: AtlasData): AtlasOp[] {
  const ops: AtlasOp[] = [];
  const before = new Map(prev.members.map((m) => [m.id, m]));
  const after = new Set(next.members.map((m) => m.id));
  const groupsBefore = new Set(prev.groups.map((g) => g.id));
  const groupsAfter = new Map(next.groups.map((g) => [g.id, g]));
  for (const member of next.members) {
    const old = before.get(member.id);
    if (!old) ops.push({ type: "bud", member, group: groupsBefore.has(member.group) ? undefined : groupsAfter.get(member.group) });
    else if (old.weight !== member.weight) ops.push({ type: "pulse", id: member.id });
  }
  for (const member of prev.members) if (!after.has(member.id)) ops.push({ type: "retract", id: member.id });
  const linksBefore = new Set(prev.links.map((l) => `${l.kind}|${l.a}|${l.b}`));
  for (const link of next.links) if (!linksBefore.has(`${link.kind}|${link.a}|${link.b}`)) ops.push({ type: "link", link });
  if (next.versions.length > prev.versions.length) ops.push({ type: "breathe" });
  return ops;
}
