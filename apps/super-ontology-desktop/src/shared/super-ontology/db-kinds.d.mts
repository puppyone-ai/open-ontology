export type DbKind = { readonly id: string; readonly label: string; readonly port: number; readonly nameField: "database" | "service"; readonly tls: "required" | "optional"; readonly wired: boolean; readonly preflight: boolean; readonly family?: string };
export const DB_KINDS: readonly DbKind[];
export const DB_WIRED_KIND_IDS: readonly string[];
export const DB_KIND_IDS: readonly string[];
export function dbKind(id: string): DbKind | null;
export const TABLE_ENTRY: RegExp;
export const SOURCE_NAME: RegExp;
export const HOST_NAME: RegExp;
