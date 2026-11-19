import type { DbSource } from "../../shared/super-ontology/bridge";
import { invoke, type ApiResult } from "./bridgeInvoke";

export type DbKind = "postgres" | "mysql";
export type DbSourceInput = { kind: DbKind; name: string; host: string; port: number; database: string; user: string; password: string; caPem: string; include: string[] };

export const DEFAULT_PORT: Record<DbKind, number> = { postgres: 5432, mysql: 3306 };
const NAME = /^[A-Za-z0-9_-]{1,64}$/u;
const HOST = /^[A-Za-z0-9._:-]{1,253}$/u;
const TABLE = /^[A-Za-z0-9_$]{1,63}\.(?:[A-Za-z0-9_$]{1,63}|\*)$/u;
const PEM = /^-----BEGIN CERTIFICATE-----[A-Za-z0-9+/=\s-]{16,60000}-----END CERTIFICATE-----\s*$/u;

export type DbField = "name" | "host" | "port" | "database" | "user" | "password" | "caPem" | "include";

/** The fields that cannot be sent as they stand; the form shows the same words under each (`required` when empty, `invalid` when malformed). */
export function invalidDbFields(values: { name: string; host: string; port: string; database: string; user: string; password: string; caPem: string; tables: string }): Partial<Record<DbField, "required" | "invalid">> {
  const out: Partial<Record<DbField, "required" | "invalid">> = {};
  const check = (field: DbField, value: string, pattern?: RegExp) => { const v = value.trim(); if (v === "") out[field] = "required"; else if (pattern && !pattern.test(v)) out[field] = "invalid"; };
  check("name", values.name, NAME);
  check("host", values.host, HOST);
  const port = Number(values.port);
  if (values.port.trim() === "") out.port = "required"; else if (!Number.isInteger(port) || port < 1 || port > 65535) out.port = "invalid";
  check("database", values.database);
  check("user", values.user);
  if (values.password === "") out.password = "required";
  check("caPem", values.caPem, PEM);
  const tables = parseTables(values.tables);
  if (tables.length === 0) out.include = "required"; else if (tables.some((entry) => !TABLE.test(entry))) out.include = "invalid";
  return out;
}

/** One table pattern per line (commas also separate), as `schema.table` or `schema.*`. */
export function parseTables(text: string): string[] {
  return [...new Set(text.split(/[\n,]/u).map((entry) => entry.trim()).filter(Boolean))];
}

export const databaseApi = {
  list: (): Promise<ApiResult<{ items: DbSource[] }>> => invoke("db-sources-list", {}),
  add: (input: DbSourceInput): Promise<ApiResult<DbSource>> => invoke("db-source-add", input),
  test: (name: string): Promise<ApiResult<{ reachable: boolean }>> => invoke("db-source-test", { name }),
  remove: (name: string): Promise<ApiResult<{ removed: true }>> => invoke("db-source-remove", { name }),
};
