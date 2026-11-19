import type { DriveCreateInput, ObjectStoreBackend } from "../superDriveApi";

/**
 * Form values of an object-store Drive; the two keys exist only while the dialog is open and are never read back from the engine.
 * `allow` and `deny` are the access policy, one whole-segment path prefix per line.
 */
export type S3FormValues = {
  endpoint: string; region: string; bucket: string; prefix: string; pathStyle: boolean;
  accessKeyId: string; secretAccessKey: string; sessionToken: string;
  accountId: string; internal: boolean; allow: string; deny: string;
};

export const EMPTY_S3_FORM: S3FormValues = {
  endpoint: "", region: "", bucket: "", prefix: "", pathStyle: false, accessKeyId: "", secretAccessKey: "", sessionToken: "",
  accountId: "", internal: false, allow: "", deny: "",
};

export type S3FieldName = "endpoint" | "region" | "bucket" | "prefix" | "accessKeyId" | "secretAccessKey" | "sessionToken" | "accountId" | "allow" | "deny";

export const OBJECT_STORE_BACKENDS: readonly ObjectStoreBackend[] = ["s3", "oss", "cos", "r2"];
const ACCOUNT_ID = /^[0-9a-f]{32}$/iu;

/** Which fields a backend shows. OSS and COS are addressed by region, R2 by account id; only s3 needs the endpoint. */
export function objectStoreFields(backend: ObjectStoreBackend): { endpoint: "required" | "optional"; region: "required" | "optional" | "none"; accountId: boolean; internal: boolean; pathStyle: boolean } {
  switch (backend) {
    case "oss":
    case "cos": return { endpoint: "optional", region: "required", accountId: false, internal: true, pathStyle: false };
    case "r2": return { endpoint: "optional", region: "none", accountId: true, internal: false, pathStyle: false };
    default: return { endpoint: "required", region: "optional", accountId: false, internal: false, pathStyle: true };
  }
}

/** Prefixes of the policy boxes: one per line (commas too), trailing slash dropped, empty lines and repeats removed. */
export function parsePrefixList(text: string): string[] {
  const seen = new Set<string>();
  for (const part of text.split(/[\n,]/u)) {
    const prefix = part.trim().replace(/\/+$/u, "");
    if (prefix !== "") seen.add(prefix);
  }
  return [...seen];
}

/** A prefix is a relative path with no empty, `.` or `..` segment (the main process checks the same rule). */
export function isValidPrefix(prefix: string): boolean {
  return !prefix.startsWith("/") && prefix.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..") && !/[\u0000-\u001f\u007f]/u.test(prefix);
}

export function invalidPolicyFields(values: S3FormValues): ("allow" | "deny")[] {
  return (["allow", "deny"] as const).filter((name) => parsePrefixList(values[name]).some((prefix) => !isValidPrefix(prefix)));
}

export function isValidAccountId(value: string): boolean {
  return ACCOUNT_ID.test(value.trim());
}

const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** `https://` anywhere, `http://` only to this computer. The main process checks the same rule again. */
export function isAllowedEndpoint(value: string): boolean {
  try {
    const url = new URL(value.trim());
    if (url.username !== "" || url.password !== "") return false;
    return url.protocol === "https:" || (url.protocol === "http:" && LOOPBACK_HOSTS.has(url.hostname));
  } catch {
    return false;
  }
}

/** Fields a user still has to fill, in form order (the secret fields count as missing when empty). */
export function missingS3Fields(values: S3FormValues, backend: ObjectStoreBackend = "s3"): S3FieldName[] {
  const missing: S3FieldName[] = [];
  const shown = objectStoreFields(backend);
  if (shown.endpoint === "required" && values.endpoint.trim() === "") missing.push("endpoint");
  // A cloud with an endpoint override needs no region: the override says where the store is.
  if (shown.region === "required" && values.region.trim() === "" && values.endpoint.trim() === "") missing.push("region");
  if (shown.accountId && values.accountId.trim() === "") missing.push("accountId");
  if (values.bucket.trim().length < 3) missing.push("bucket");
  if (values.accessKeyId.trim() === "") missing.push("accessKeyId");
  if (values.secretAccessKey === "") missing.push("secretAccessKey");
  return missing;
}

/** The engine's source errors that belong to one field (§3.9); everything else is shown for the dialog as a whole. */
export function s3FieldOfError(code: string): S3FieldName | null {
  switch (code) {
    case "DOCS_SOURCE_ENDPOINT_INVALID": return "endpoint";
    case "DOCS_SOURCE_BUCKET_NOT_FOUND": return "bucket";
    case "DOCS_SOURCE_AUTH_FAILED": return "accessKeyId";
    case "DOCS_SOURCE_CREDENTIAL_MISSING": return "secretAccessKey";
    default: return null;
  }
}

export function toS3CreateInput(name: string, values: S3FormValues, operationId: string, backend: ObjectStoreBackend = "s3"): Extract<DriveCreateInput, { backend: ObjectStoreBackend }> {
  const shown = objectStoreFields(backend);
  const optional = (value: string) => (value.trim() === "" ? {} : { value: value.trim() });
  const endpoint = optional(values.endpoint).value;
  const region = shown.region === "none" ? undefined : optional(values.region).value;
  const prefix = optional(values.prefix).value;
  const sessionToken = values.sessionToken === "" ? undefined : values.sessionToken;
  const allowPrefixes = parsePrefixList(values.allow);
  const denyPrefixes = parsePrefixList(values.deny);
  return {
    operationId, name, backend, bucket: values.bucket.trim(),
    ...(endpoint ? { endpoint } : {}),
    ...(region ? { region } : {}), ...(prefix ? { prefix } : {}), ...(shown.pathStyle && values.pathStyle ? { pathStyle: true } : {}),
    ...(shown.accountId ? { accountId: values.accountId.trim().toLowerCase() } : {}), ...(shown.internal && values.internal ? { internal: true } : {}),
    accessKeyId: values.accessKeyId.trim(), secretAccessKey: values.secretAccessKey, ...(sessionToken ? { sessionToken } : {}),
    ...(allowPrefixes.length > 0 ? { allowPrefixes } : {}), ...(denyPrefixes.length > 0 ? { denyPrefixes } : {}),
  };
}
