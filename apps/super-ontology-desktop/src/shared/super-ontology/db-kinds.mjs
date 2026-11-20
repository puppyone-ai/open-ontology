/**
 * Database kinds of Add source > Database (D88, volume 25 section 7.1). One table for the main process (argument building,
 * validation) and the renderer (the picker and the per-kind fields), so the two never disagree. It mirrors the table in
 * `crates/oo-cli/src/docs/source_db.rs` (kind, stack engine, default port, TLS rule, scope unit): the CLI stays the one
 * implementation, and every kind goes through `oo source add <kind>`, `oo source test` and `oo doctor`.
 *
 * `tls` is the CLI's rule for the trust anchor:
 *   required  verified TLS only: a CA is required
 *   loopback  a CA is required unless the host is this machine (127.0.0.1, ::1, localhost); plain text is allowed there
 *   driver    the transport owns TLS: the stack takes no CA (the CLI refuses one)
 *   optional  a CA may be given
 * `unit` is what an include entry names: a table (`schema.table`), a collection (`database.collection`) or an index (`schema.index`).
 * `preflight` kinds can be checked before they are added (`oo init --engine <kind>`); the others are checked right after they are added.
 */
const k = (id, label, rest) => Object.freeze({ id, label, engine: id, port: 0, nameField: "database", tls: "required", unit: "table", wired: true, preflight: false, ...rest });

export const DB_KINDS = Object.freeze([
  k("postgres", "PostgreSQL", { port: 5432, preflight: true }),
  k("mysql", "MySQL", { port: 3306, preflight: true }),
  k("mariadb", "MariaDB", { port: 3306 }),
  k("tidb", "TiDB", { port: 4000 }),
  k("oceanbase", "OceanBase", { port: 2881 }),
  k("polardb", "PolarDB", { port: 3306, engine: "mysql-polardb" }),
  k("opengauss", "openGauss", { port: 5432 }),
  k("kingbase", "Kingbase", { port: 54321 }),
  k("sqlserver", "SQL Server", { port: 1433 }),
  k("oracle", "Oracle", { port: 1521, nameField: "service", tls: "driver", preflight: true }),
  k("db2", "IBM Db2", { port: 50000, tls: "optional" }),
  k("dm8", "DM8 (Dameng)", { port: 5236, tls: "driver" }),
  k("clickhouse", "ClickHouse", { port: 9004, tls: "loopback" }),
  k("doris", "Apache Doris", { port: 9030, tls: "loopback" }),
  k("starrocks", "StarRocks", { port: 9030, tls: "loopback" }),
  k("mongodb", "MongoDB", { port: 27017, tls: "loopback", unit: "collection" }),
  k("opensearch", "OpenSearch", { port: 9200, tls: "loopback", unit: "index" }),
]);

export const DB_WIRED_KIND_IDS = Object.freeze(DB_KINDS.filter((kind) => kind.wired).map((kind) => kind.id));
export const DB_KIND_IDS = Object.freeze(DB_KINDS.map((kind) => kind.id));
export const dbKind = (id) => DB_KINDS.find((kind) => kind.id === id) ?? null;

/** The hosts a `loopback` kind may reach without a CA (the CLI's own list). */
export const isLoopbackHost = (host) => ["127.0.0.1", "::1", "localhost"].includes(String(host).trim());
/** Whether a CA must be supplied for this kind and host (the CLI's rule). */
export const caRequired = (kind, host) => kind?.tls === "required" || (kind?.tls === "loopback" && !isLoopbackHost(host));
/** Whether the form offers a CA at all. */
export const caAccepted = (kind) => kind?.tls !== "driver";

/** An include/exclude entry: `schema.table`, optionally a trailing `.*` or a lone `*` on exclude. */
export const TABLE_ENTRY = /^[^\s,;'"`\\]{1,200}$/u;
export const SOURCE_NAME = /^[A-Za-z0-9_-]{1,64}$/u;
export const HOST_NAME = /^[A-Za-z0-9._:-]{1,253}$/u;
