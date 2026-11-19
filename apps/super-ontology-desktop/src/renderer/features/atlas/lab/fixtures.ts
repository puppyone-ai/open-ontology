import type { Drive, FileSummary, LinkList, Page } from "../../../../shared/super-ontology/docs-api.generated";
import { ENTITY_TYPE_VALUES } from "../../../../shared/super-ontology/docs-api.generated";
import type { ApiResult } from "../../../model/bridgeInvoke";
import type { EntityDetail, EntitySearchRow } from "../../../model/superDriveOntologyTypes";
import { dbGroupId, entityId, tableId, typeGroupId, type AtlasSource, type DbTable } from "../adapter";
import type { AtlasData, AtlasGroup, AtlasLink, AtlasMember, AtlasTone } from "../model";

/**
 * Synthetic corpora for tests, the measuring page and the screenshots. Everything here is invented: neutral names, numbers from a
 * seeded generator, no customer data and no real documents. The same seed gives the same atlas.
 */
export type SyntheticOptions = {
  /** Entity types (document side) and database schemas. */
  groups: number;
  /** Total members across the groups (entities and tables). */
  members: number;
  /** Links per member, on average. */
  linkRatio?: number;
  /** Ontology versions the members are spread over. */
  versions?: number;
  /** Share of the groups that are database schemas holding tables. */
  dbShare?: number;
  seed?: number;
  /** Labels of the document side groups, in order (entity types when the data goes through the adapter). */
  names?: readonly string[];
};

/** mulberry32: a tiny seeded generator, so a fixture never changes between runs. */
export function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const TONES: AtlasTone[] = ["ink", "steel", "dusk", "mist", "sand", "stone"];
const WORDS = ["Alder", "Birch", "Cedar", "Dune", "Ember", "Fjord", "Grove", "Harbor", "Iris", "Juniper", "Kelp", "Lumen", "Moss", "Nectar", "Orchid", "Pebble", "Quartz", "Reed", "Sage", "Tundra", "Umber", "Vale", "Willow", "Yarrow", "Zephyr"];

export function syntheticAtlas(options: SyntheticOptions): AtlasData {
  const random = seeded(options.seed ?? 7);
  const versions = Math.max(1, options.versions ?? 8);
  const dbGroups = Math.round(options.groups * (options.dbShare ?? 0.25));
  const groups: AtlasGroup[] = [];
  for (let i = 0; i < options.groups; i += 1) {
    const db = i >= options.groups - dbGroups;
    const name = !db && options.names ? options.names[i]! : `${WORDS[i % WORDS.length]}${i >= WORDS.length ? ` ${Math.floor(i / WORDS.length) + 1}` : ""}`;
    groups.push({ id: db ? dbGroupId(name) : `g:type:${name}`, kind: db ? "db" : "type", label: name, tone: db ? (i % 2 ? "stone" : "steel") : TONES[i % 5]! });
  }
  // A skewed split, so a few groups are big and many are small, as real ontologies are.
  const weights = groups.map((_, i) => 1 / (i + 1.5) ** 0.9);
  const total = weights.reduce((a, b) => a + b, 0);
  const members: AtlasMember[] = [];
  groups.forEach((group, gi) => {
    const share = Math.max(1, Math.round((options.members * weights[gi]!) / total));
    const first = Math.min(versions, 1 + Math.floor((gi / groups.length) * (versions - 1)));
    for (let m = 0; m < share && members.length < options.members; m += 1) {
      const born = Math.min(versions, first + Math.floor(random() * (versions - first + 1)));
      const index = members.length;
      const label = group.kind === "db" ? `${WORDS[(index * 7) % WORDS.length]!.toLowerCase()}_${index}` : `${WORDS[(index * 3) % WORDS.length]} ${String(index).padStart(5, "0")}`;
      members.push({
        id: group.kind === "db" ? `t:${group.label}.${label}` : `e:${group.label.toLowerCase()}-${index}`,
        group: group.id, label, ref: group.kind === "db" ? { kind: "table", table: `${group.label}.${label}` } : { kind: "entity", key: `${group.label.toLowerCase()}-${index}` },
        weight: group.kind === "db" ? 0 : Math.floor(random() ** 2 * 40), born,
      });
    }
  });
  const links: AtlasLink[] = [];
  const seen = new Set<string>();
  const wanted = Math.floor(members.length * (options.linkRatio ?? 0.8));
  for (let i = 0; i < wanted * 3 && links.length < wanted; i += 1) {
    const a = members[Math.floor(random() * members.length)]!;
    // Mostly near neighbours in the list (the same or an adjacent group), sometimes far: a believable mix of local and global links.
    const offset = random() < 0.7 ? Math.floor((random() - 0.5) * 40) : Math.floor(random() * members.length);
    const b = members[(((members.indexOf(a) + offset) % members.length) + members.length) % members.length]!;
    if (a.id === b.id) continue;
    const [lo, hi] = a.id < b.id ? [a, b] : [b, a];
    const kind: AtlasLink["kind"] = lo.ref.kind === "table" && hi.ref.kind === "table" ? "foreign_key" : lo.ref.kind === "table" || hi.ref.kind === "table" ? "link" : "fact";
    const key = `${kind}|${lo.id}|${hi.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    links.push({ a: lo.id, b: hi.id, kind, weight: 1 });
  }
  // An entity that has relations has facts: weights never fall below the number of fact links, as in a real ontology.
  const degree = new Map<string, number>();
  for (const link of links) if (link.kind === "fact") { degree.set(link.a, (degree.get(link.a) ?? 0) + 1); degree.set(link.b, (degree.get(link.b) ?? 0) + 1); }
  for (const member of members) member.weight = Math.max(member.weight, degree.get(member.id) ?? 0);
  return { groups, members, links, versions: Array.from({ length: versions }, (_, i) => ({ n: i + 1, asOf: `2026-11-${String(1 + i).padStart(2, "0")}T00:00:00Z` })), partial: [] };
}

// ---- a fake engine that serves an AtlasData through the same surface the real one does --------------------------

const ok = <T,>(value: T): ApiResult<T> => ({ ok: true, value });
const uuid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;

/**
 * Serves `data` as the engine would: entity listing pages, entity pages whose facts name other entities, the document to table
 * links and the table catalog. Only for the entity types and the schemas in `data`; `files` come from `drives`.
 */
export function fakeAtlasSource(data: AtlasData, extra: { drives?: Drive[]; files?: FileSummary[]; failCatalog?: boolean; failLinks?: boolean } = {}): AtlasSource & { calls: Record<string, number> } {
  const calls: Record<string, number> = {};
  const count = (name: string) => { calls[name] = (calls[name] ?? 0) + 1; };
  const entities = data.members.filter((m) => m.ref.kind === "entity");
  const typeOf = (groupId: string) => (data.groups.find((g) => g.id === groupId)?.label ?? "Thing");
  const idOf = new Map(entities.map((m, i) => [m.id, uuid(i + 1)]));
  const rows: EntitySearchRow[] = entities.map((m) => ({ id: idOf.get(m.id)!, key: (m.ref as { key: string }).key, type: typeOf(m.group) as EntitySearchRow["type"], name: m.label, counts: { facts: m.weight }, since_version: m.born } as EntitySearchRow));
  const byKey = new Map(rows.map((r) => [r.key, r]));
  const factLinks = data.links.filter((l) => l.kind === "fact");
  // Without explicit files, every document-to-table link gets a file of its own ("notes/<entity>.md") in one Drive, so source mode has them to link from.
  const docLinks = data.links.filter((l) => l.kind === "link").map((l) => (l.a.startsWith("e:") ? [l.a.slice(2), l.b.slice(2)] : [l.b.slice(2), l.a.slice(2)]) as [string, string]);
  const driveId = uuid(900);
  const driveNames = ["Team docs", "Research", "Archive"];
  const drives = extra.drives ?? driveNames.map((name, i) => ({ id: i === 0 ? driveId : uuid(901 + i), name, backend: "fs", config: {}, status: "active", revision: 1, counts: {}, created_at: "2026-11-01T00:00:00Z" } as Drive));
  const fileOf = (id: number, drive: string, path: string) => ({ id: uuid(id), drive_id: drive, path, state: "published", state_since: "2026-11-01T00:00:00Z", retryable: false, pending_review: 0 }) as FileSummary;
  const fillers = extra.files || drives.length < 3 ? [] : entities.slice(0, 240).map((m, i) => fileOf(8000 + i, drives[i % 3 === 0 ? 1 : i % 3 === 1 ? 2 : 0]!.id, `${m.label.toLowerCase().replace(/ /g, "-")}.md`));
  const fileRows = extra.files ?? [...docLinks.map(([key], i) => fileOf(5000 + i, driveId, `notes/${key}.md`)), ...fillers];
  return {
    calls,
    entities: async ({ limit, cursor }) => {
      count("entities");
      const start = cursor ? Number(cursor) : 0;
      const items = rows.slice(start, start + limit);
      return ok<Page<EntitySearchRow>>({ items, next_cursor: start + limit < rows.length ? String(start + limit) : null, as_of_seq: 1 });
    },
    entity: async (key) => {
      count("entity");
      const row = byKey.get(key);
      if (!row) return { ok: false, error: { code: "DOCS_NOT_FOUND", retryable: false, details: {} } };
      const facts = factLinks.filter((l) => l.a === entityId(key) || l.b === entityId(key)).map((l, i) => {
        const subjectKey = l.a.slice(2);
        const objectKey = l.b.slice(2);
        return { id: uuid(100000 + i), fact_key: "0".repeat(64), subject: byKey.get(subjectKey)!, predicate: "related_to", object: byKey.get(objectKey) ?? null, confidence_bp: 9000, origin: "rule", status: "published" };
      });
      return ok({ ...row, facts } as unknown as EntityDetail);
    },
    versions: async () => { count("versions"); return ok({ items: data.versions.map((v) => ({ n: v.n, as_of: v.asOf, published_at: v.asOf })), next_cursor: null, as_of_seq: 1 }); },
    drives: async () => { count("drives"); return ok(drives); },
    files: async ({ driveId }) => { count("files"); return ok({ items: fileRows.filter((f) => f.drive_id === driveId), next_cursor: null, as_of_seq: 1 }); },
    links: async () => {
      count("links");
      if (extra.failLinks) return { ok: false, error: { code: "SO_ENGINE_UNREACHABLE", retryable: true, details: {} } };
      const items = data.links.filter((l) => l.kind === "link").map((l) => {
        const [doc, table] = l.a.startsWith("e:") ? [l.a, l.b] : [l.b, l.a];
        return { drive_id: driveId, doc_entity_key: doc.slice(2), db_table: table.slice(2), db_key: "1", status: "auto", provenance: { file: [{ path: `notes/${doc.slice(2)}.md`, file_version: "v1", block_id: "b1", anchor: {} }], table_row: { table: table.slice(2), key: "1" } } };
      });
      return ok({ items, agent_db: "bound" } as unknown as LinkList);
    },
    catalog: async () => {
      count("catalog");
      if (extra.failCatalog) return { ok: false, error: { code: "ONTOLOGY_SCOPE_DENIED", retryable: false, details: {} } };
      const tables: DbTable[] = data.members.filter((m) => m.ref.kind === "table").map((m) => { const name = (m.ref as { table: string }).table; const dot = name.indexOf("."); return { schema: name.slice(0, dot), table: name.slice(dot + 1) }; });
      const fk = data.links.filter((l) => l.kind === "foreign_key").map((l) => { const a = l.a.slice(2), b = l.b.slice(2); const split = (n: string) => ({ schema: n.slice(0, n.indexOf(".")), table: n.slice(n.indexOf(".") + 1) }); return { from: split(a), to: split(b) }; });
      return ok({ tables, foreignKeys: fk });
    },
  };
}

export { ENTITY_TYPE_VALUES, tableId, typeGroupId };
