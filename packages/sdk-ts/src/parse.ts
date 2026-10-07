/** Parsers for the frozen gateway JSON. They do not call the network. */

export const PACK_METADATA_PROTOCOL = "oo.builder.role-pack/v1";
export const MAX_SAFE_U64 = 9007199254740991;

export const GATEWAY_ERROR_TEXT = {
  UNAUTHENTICATED: {
    status: 401,
    message: "The bearer token was not accepted.",
    hint: "Present a current client token for this gateway audience.",
    retryable: false,
  },
  FORBIDDEN: {
    status: 403,
    message: "The token does not allow this metadata.",
    hint: "The project, role, and scope are taken from the token record.",
    retryable: false,
  },
  QUERY_NOT_ALLOWED: {
    status: 400,
    message: "Query strings are not accepted.",
    hint: "Put the client token in the Authorization header.",
    retryable: false,
  },
  NOT_FOUND: {
    status: 404,
    message: "That path is not served.",
    hint: "Use one of the fixed routes.",
    retryable: false,
  },
  METHOD_NOT_ALLOWED: {
    status: 405,
    message: "That method is not served on this path.",
    hint: "Use the method declared for that fixed route.",
    retryable: false,
  },
  SQL_UNSUPPORTED: {
    status: 501,
    message: "SQL execution is not served by this process.",
    hint: "Untrusted SQL stays unavailable until a kernel sandbox admits it.",
    retryable: false,
  },
  INTERNAL: {
    status: 500,
    message: "The request could not be completed.",
    hint: "Retry later. This response has no driver detail.",
    retryable: true,
  },
} as const;

export type GatewayErrorCode = keyof typeof GATEWAY_ERROR_TEXT;

export type ParseFailure = {
  ok: false;
  reason: "invalid_schema" | "incompatible_enum" | "unsafe_integer";
  path: string;
};

export type PackFileWire = {
  path: string;
  bytes: number;
  sha256: string;
};

export type PackColumnWire = {
  table: string;
  name: string;
};

export type PackMetadataWire = {
  authentication: "loopback-http" | "verified-certificate";
  external_mtls: boolean;
  oauth: false;
  idp: false;
  peer_workload?: string;
  peer_source?: "verified-certificate";
  project_id: string;
  principal_id: string;
  role: string;
  policy_version: string;
  knowledge_version: string;
  audience_url: string;
  scope: string;
  sandbox_id: string | null;
  session_id: string | null;
  pack_digest: string;
  protocol: typeof PACK_METADATA_PROTOCOL;
  files: PackFileWire[];
  columns: PackColumnWire[];
};

export type GatewayErrorWire = {
  error: {
    code: GatewayErrorCode;
    message: string;
    hint: string;
    retryable: boolean;
  };
};

type ParseOk<T> = {
  ok: true;
  value: T;
  unrecognizedKeys: readonly string[];
};

const PACK_KEYS = [
  "authentication",
  "external_mtls",
  "oauth",
  "idp",
  "peer_workload",
  "peer_source",
  "project_id",
  "principal_id",
  "role",
  "policy_version",
  "knowledge_version",
  "audience_url",
  "scope",
  "sandbox_id",
  "session_id",
  "pack_digest",
  "protocol",
  "files",
  "columns",
] as const;

function fail(reason: ParseFailure["reason"], path: string): ParseFailure {
  return { ok: false, reason, path };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requiredString(record: Record<string, unknown>, key: string, path: string): string | ParseFailure {
  const value = record[key];
  if (typeof value !== "string" || value.length === 0) {
    return fail("invalid_schema", path);
  }
  return value;
}

function nullableString(record: Record<string, unknown>, key: string, path: string): string | null | ParseFailure {
  const value = record[key];
  if (value === null) {
    return null;
  }
  if (typeof value !== "string") {
    return fail("invalid_schema", path);
  }
  return value;
}

function unrecognized(record: Record<string, unknown>, known: readonly string[], prefix: string): string[] {
  const allowed = new Set(known);
  return Object.keys(record)
    .filter((key) => !allowed.has(key))
    .map((key) => (prefix.length === 0 ? key : `${prefix}.${key}`));
}

function parseBytes(value: unknown, path: string): number | ParseFailure {
  if (typeof value !== "number" || !Number.isInteger(value)) {
    return fail("invalid_schema", path);
  }
  if (value < 0 || value > MAX_SAFE_U64) {
    return fail("unsafe_integer", path);
  }
  return value;
}

export function parsePackMetadata(input: unknown): ParseOk<PackMetadataWire> | ParseFailure {
  if (!isRecord(input)) {
    return fail("invalid_schema", "$");
  }
  const authentication = input.authentication;
  if (authentication !== "loopback-http" && authentication !== "verified-certificate") {
    return fail("incompatible_enum", "$.authentication");
  }
  if (typeof input.external_mtls !== "boolean") {
    return fail("invalid_schema", "$.external_mtls");
  }
  if (input.oauth !== false) {
    return fail("incompatible_enum", "$.oauth");
  }
  if (input.idp !== false) {
    return fail("incompatible_enum", "$.idp");
  }
  if (input.protocol !== PACK_METADATA_PROTOCOL) {
    return fail("incompatible_enum", "$.protocol");
  }
  const projectId = requiredString(input, "project_id", "$.project_id");
  const principalId = requiredString(input, "principal_id", "$.principal_id");
  const role = requiredString(input, "role", "$.role");
  const policyVersion = requiredString(input, "policy_version", "$.policy_version");
  const knowledgeVersion = requiredString(input, "knowledge_version", "$.knowledge_version");
  const audienceUrl = requiredString(input, "audience_url", "$.audience_url");
  const scope = requiredString(input, "scope", "$.scope");
  const packDigest = requiredString(input, "pack_digest", "$.pack_digest");
  const sandboxId = nullableString(input, "sandbox_id", "$.sandbox_id");
  const sessionId = nullableString(input, "session_id", "$.session_id");
  const scalars = [projectId, principalId, role, policyVersion, knowledgeVersion, audienceUrl, scope, packDigest, sandboxId, sessionId];
  for (const item of scalars) {
    if (typeof item === "object" && item !== null && "ok" in item) {
      return item;
    }
  }
  if (!Array.isArray(input.files)) {
    return fail("invalid_schema", "$.files");
  }
  if (!Array.isArray(input.columns)) {
    return fail("invalid_schema", "$.columns");
  }
  const extra = unrecognized(input, PACK_KEYS, "");
  let peerWorkload: string | undefined;
  let peerSource: "verified-certificate" | undefined;
  if (input.external_mtls) {
    const workload = requiredString(input, "peer_workload", "$.peer_workload");
    if (typeof workload !== "string") {
      return workload;
    }
    if (input.peer_source !== "verified-certificate") {
      return fail("incompatible_enum", "$.peer_source");
    }
    peerWorkload = workload;
    peerSource = "verified-certificate";
  } else if ("peer_workload" in input || "peer_source" in input) {
    return fail("invalid_schema", "$.peer_workload");
  }
  const files: PackFileWire[] = [];
  for (let index = 0; index < input.files.length; index += 1) {
    const file = input.files[index];
    if (!isRecord(file)) {
      return fail("invalid_schema", `$.files[${index}]`);
    }
    const path = requiredString(file, "path", `$.files[${index}].path`);
    const sha = requiredString(file, "sha256", `$.files[${index}].sha256`);
    const bytes = parseBytes(file.bytes, `$.files[${index}].bytes`);
    if (typeof path !== "string") return path;
    if (typeof sha !== "string") return sha;
    if (typeof bytes !== "number") return bytes;
    extra.push(...unrecognized(file, ["path", "bytes", "sha256"], `files[${index}]`));
    files.push({ path, bytes, sha256: sha });
  }
  const columns: PackColumnWire[] = [];
  for (let index = 0; index < input.columns.length; index += 1) {
    const column = input.columns[index];
    if (!isRecord(column)) {
      return fail("invalid_schema", `$.columns[${index}]`);
    }
    const table = requiredString(column, "table", `$.columns[${index}].table`);
    const name = requiredString(column, "name", `$.columns[${index}].name`);
    if (typeof table !== "string") return table;
    if (typeof name !== "string") return name;
    extra.push(...unrecognized(column, ["table", "name"], `columns[${index}]`));
    columns.push({ table, name });
  }
  const value: PackMetadataWire = {
    authentication,
    external_mtls: input.external_mtls,
    oauth: false,
    idp: false,
    project_id: projectId as string,
    principal_id: principalId as string,
    role: role as string,
    policy_version: policyVersion as string,
    knowledge_version: knowledgeVersion as string,
    audience_url: audienceUrl as string,
    scope: scope as string,
    sandbox_id: sandboxId as string | null,
    session_id: sessionId as string | null,
    pack_digest: packDigest as string,
    protocol: PACK_METADATA_PROTOCOL,
    files,
    columns,
  };
  if (peerWorkload !== undefined && peerSource !== undefined) {
    value.peer_workload = peerWorkload;
    value.peer_source = peerSource;
  }
  return { ok: true, value, unrecognizedKeys: extra };
}

export function parseGatewayError(input: unknown): ParseOk<GatewayErrorWire> | ParseFailure {
  if (!isRecord(input) || !isRecord(input.error)) {
    return fail("invalid_schema", "$.error");
  }
  const code = input.error.code;
  if (typeof code !== "string") {
    return fail("invalid_schema", "$.error.code");
  }
  if (!Object.prototype.hasOwnProperty.call(GATEWAY_ERROR_TEXT, code)) {
    return fail("incompatible_enum", "$.error.code");
  }
  const known = GATEWAY_ERROR_TEXT[code as GatewayErrorCode];
  if (input.error.message !== known.message) {
    return fail("invalid_schema", "$.error.message");
  }
  if (input.error.hint !== known.hint) {
    return fail("invalid_schema", "$.error.hint");
  }
  if (input.error.retryable !== known.retryable) {
    return fail("invalid_schema", "$.error.retryable");
  }
  if ("query_id" in input.error || "query_id" in input) {
    return fail("invalid_schema", "$.error.query_id");
  }
  const extra = [
    ...unrecognized(input, ["error"], ""),
    ...unrecognized(input.error, ["code", "message", "hint", "retryable"], "error"),
  ];
  return {
    ok: true,
    unrecognizedKeys: extra,
    value: {
      error: {
        code: code as GatewayErrorCode,
        message: known.message,
        hint: known.hint,
        retryable: known.retryable,
      },
    },
  };
}
