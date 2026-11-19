/**
 * Database kinds of Add source > Database (D88, volume 25 section 7.1). One table for the main process (argument building,
 * validation) and the renderer (the picker and the per-kind fields), so the two never disagree.
 *
 * `wired: true` kinds go through `oo source add <kind>` and the doctor probe. The others have a connector in `oo-connectors` but no
 * `oo source add` / `oo doctor` path in the CLI yet; the app lists them, says so, and never fakes a second implementation.
 */
const k = (id, label, rest) => Object.freeze({ id, label, port: 0, nameField: "database", tls: "optional", wired: true, preflight: false, ...rest });

export const DB_KINDS = Object.freeze([
  k("postgres", "PostgreSQL", { port: 5432, tls: "required", preflight: true }),
  k("mysql", "MySQL", { port: 3306, tls: "required", preflight: true }),
  k("oracle", "Oracle", { port: 1521, nameField: "service", tls: "optional", preflight: true }),
  k("sqlserver", "SQL Server", { port: 1433 }),
  k("db2", "IBM Db2", { port: 50000 }),
  k("kingbase", "Kingbase", { port: 54321, tls: "required" }),
  k("dm8", "DM8 (Dameng)", { port: 5236 }),
  k("mariadb", "MariaDB", { port: 3306, wired: false, family: "mariadb" }),
  k("tidb", "TiDB", { port: 4000, wired: false, family: "tidb" }),
  k("oceanbase", "OceanBase", { port: 2881, wired: false, family: "oceanbase" }),
  k("opengauss", "openGauss", { port: 5432, wired: false, family: "opengauss" }),
  k("polardb", "PolarDB", { port: 3306, wired: false, family: "polardb" }),
  k("doris", "Apache Doris", { port: 9030, wired: false, family: "doris" }),
  k("starrocks", "StarRocks", { port: 9030, wired: false, family: "starrocks" }),
  k("clickhouse", "ClickHouse", { port: 9000, wired: false, family: "clickhouse" }),
  k("mongodb", "MongoDB", { port: 27017, wired: false, family: "mongodb" }),
  k("opensearch", "OpenSearch", { port: 9200, wired: false, family: "opensearch" }),
]);

export const DB_WIRED_KIND_IDS = Object.freeze(DB_KINDS.filter((kind) => kind.wired).map((kind) => kind.id));
export const DB_KIND_IDS = Object.freeze(DB_KINDS.map((kind) => kind.id));
export const dbKind = (id) => DB_KINDS.find((kind) => kind.id === id) ?? null;

/** An include/exclude entry: `schema.table`, optionally a trailing `.*` or a lone `*` on exclude. */
export const TABLE_ENTRY = /^[^\s,;'"`\\]{1,200}$/u;
export const SOURCE_NAME = /^[A-Za-z0-9_-]{1,64}$/u;
export const HOST_NAME = /^[A-Za-z0-9._:-]{1,253}$/u;
