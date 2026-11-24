import { isLinkList } from "../../../shared/super-ontology/docs-api.generated";
import { failure, invoke, validated, type ApiResult } from "../../model/bridgeInvoke";
import { ontologyApi } from "../../model/superDriveOntologyApi";
import { superDriveApi } from "../../model/superDriveApi";
import type { AtlasSource, DbCatalog, DbForeignKey, DbTable } from "./adapter";

/**
 * The atlas source on the real bridge. Tables and foreign keys come from the schema surface (`oo ontology schema`, docs-api 1.12):
 * the Gateway's tables with their columns and the published relationships, one page at a time. Nothing is cached here and
 * nothing is written; the database is never read through `information_schema`, which the Gateway refuses.
 */
const PAGE = 100;
/** At most this many pages (a few thousand tables); a larger database is shown as far as it was read. */
const MAX_PAGES = 30;

const text = (value: unknown): string | null => (typeof value === "string" && value.length > 0 ? value : null);
const record = (value: unknown): Record<string, unknown> => (typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {});

const split = (qualified: string): DbTable | null => {
  const dot = qualified.indexOf(".");
  return dot > 0 && dot < qualified.length - 1 ? { schema: qualified.slice(0, dot), table: qualified.slice(dot + 1) } : null;
};

/** Reads the `tables` of one schema page; a row that is not a qualified name is skipped, not trusted. */
export function parseTables(rows: unknown[]): DbTable[] {
  const out: DbTable[] = [];
  for (const row of rows) {
    const item = record(row);
    const table = split(text(item.qualified_name) ?? "");
    if (!table) continue;
    out.push(Array.isArray(item.columns) ? { ...table, columns: item.columns.length } : table);
  }
  return out;
}

/** Reads the `relationships` of one schema page into table to table foreign keys (a relationship over several columns is one edge). */
export function parseForeignKeys(rows: unknown[]): DbForeignKey[] {
  const out: DbForeignKey[] = [];
  for (const row of rows) {
    const item = record(row);
    const from = split(text(record(item.from).table) ?? "");
    const to = split(text(record(item.to).table) ?? "");
    if (from && to) out.push({ from, to });
  }
  return out;
}

async function catalog(): Promise<ApiResult<DbCatalog>> {
  const tables: DbTable[] = [];
  const keys: DbForeignKey[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_PAGES; page += 1) {
    const read = await invoke("ontology-schema", { limit: PAGE, ...(cursor ? { cursor } : {}) });
    // The first page decides: no database side at all is the caller's partial state; a later page failing keeps what was read.
    if (!read.ok) return page === 0 ? read : { ok: true, value: { tables, foreignKeys: keys } };
    tables.push(...parseTables(read.value.tables as unknown[]));
    keys.push(...parseForeignKeys(read.value.relationships as unknown[]));
    cursor = text(read.value.next_cursor) ?? undefined;
    if (!cursor) break;
  }
  return { ok: true, value: { tables, foreignKeys: keys } };
}

export const atlasSource: AtlasSource = {
  entities: ({ limit, cursor }) => ontologyApi.searchEntities({ q: "", limit, cursor }),
  entity: (key) => ontologyApi.entityDetail({ entityIdOrKey: key }),
  versions: () => ontologyApi.ontologyVersions({ limit: 200 }),
  drives: async () => { const result = await superDriveApi.listDrives(); return result.ok ? { ...result, value: result.value.filter((drive) => drive.name !== "My files (earlier CLI folder)") } : result; },
  files: ({ driveId, limit, cursor }) => superDriveApi.listFiles({ driveId, recursive: true, limit, cursor }),
  links: async () => validated(await invoke("atlas-links", {}), (value) => (isLinkList(value) ? value : null)),
  databaseName: async () => {
    const result = await invoke("cli-db-list", {});
    const sources = result.ok && result.value.ok ? result.value.data?.sources ?? [] : [];
    return sources.length === 1 ? sources[0]!.name : null;
  },
  catalog: () => catalog().catch(() => failure("SO_ENGINE_UNREACHABLE", true)),
};
