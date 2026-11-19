import { isLinkList } from "../../../shared/super-ontology/docs-api.generated";
import { failure, invoke, validated, type ApiResult } from "../../model/bridgeInvoke";
import { ontologyApi } from "../../model/superDriveOntologyApi";
import { superDriveApi } from "../../model/superDriveApi";
import type { AtlasSource, DbCatalog, DbForeignKey, DbTable } from "./adapter";

/**
 * The atlas source on the real bridge. Tables and foreign keys come from the read-only query surface (`oo ontology query`), so a
 * database is described by its own `information_schema`; nothing is cached here and nothing is written.
 */
const TABLES_SQL = "SELECT table_schema, table_name FROM information_schema.tables WHERE table_type = 'BASE TABLE' AND table_schema NOT IN ('information_schema', 'pg_catalog') ORDER BY 1, 2";
const FOREIGN_KEYS_SQL = "SELECT tc.table_schema, tc.table_name, ccu.table_schema, ccu.table_name FROM information_schema.table_constraints tc JOIN information_schema.constraint_column_usage ccu ON tc.constraint_name = ccu.constraint_name AND tc.constraint_schema = ccu.constraint_schema WHERE tc.constraint_type = 'FOREIGN KEY'";
const MAX_ROWS = 2000;

const text = (value: unknown): string | null => (typeof value === "string" && value.length > 0 ? value : null);

/** Reads `[schema, table]` rows; a row that is not two strings is skipped, not trusted. */
export function parseTables(rows: unknown[][]): DbTable[] {
  const out: DbTable[] = [];
  for (const row of rows) { const schema = text(row[0]); const table = text(row[1]); if (schema && table) out.push({ schema, table }); }
  return out;
}

export function parseForeignKeys(rows: unknown[][]): DbForeignKey[] {
  const out: DbForeignKey[] = [];
  for (const row of rows) {
    const [fs, ft, ts, tt] = [text(row[0]), text(row[1]), text(row[2]), text(row[3])];
    if (fs && ft && ts && tt) out.push({ from: { schema: fs, table: ft }, to: { schema: ts, table: tt } });
  }
  return out;
}

async function catalog(): Promise<ApiResult<DbCatalog>> {
  const tables = await invoke("atlas-sql", { sql: TABLES_SQL, maxRows: MAX_ROWS });
  if (!tables.ok) return tables;
  // Foreign keys are a refinement: when only they fail the tables still show.
  const keys = await invoke("atlas-sql", { sql: FOREIGN_KEYS_SQL, maxRows: MAX_ROWS });
  return { ok: true, value: { tables: parseTables(tables.value.rows), foreignKeys: keys.ok ? parseForeignKeys(keys.value.rows) : [] } };
}

export const atlasSource: AtlasSource = {
  entities: ({ limit, cursor }) => ontologyApi.searchEntities({ q: "", limit, cursor }),
  entity: (key) => ontologyApi.entityDetail({ entityIdOrKey: key }),
  versions: () => ontologyApi.ontologyVersions({ limit: 200 }),
  drives: () => superDriveApi.listDrives(),
  files: ({ driveId, limit, cursor }) => superDriveApi.listFiles({ driveId, recursive: true, limit, cursor }),
  links: async () => validated(await invoke("atlas-links", {}), (value) => (isLinkList(value) ? value : null)),
  catalog: () => catalog().catch(() => failure("SO_ENGINE_UNREACHABLE", true)),
};
