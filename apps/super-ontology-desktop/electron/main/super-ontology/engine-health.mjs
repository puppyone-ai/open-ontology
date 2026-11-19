const READ_LIMIT_BYTES = 1024 * 1024;

/** Port was validated by serve-credentials; the host is fixed to loopback (20-frontend §1.1 rule 3). */
export const engineBaseUrl = (port) => `http://127.0.0.1:${port}`;

async function readLimitedJson(response) {
  try {
    const text = await response.text();
    if (text.length > READ_LIMIT_BYTES) return null;
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * One health round: `GET /livez` (no auth) then `GET /v1/docs/health` with the
 * control token. Throws on a network failure or a failed liveness probe;
 * otherwise returns the raw outcome for `classifyHealth`. The token only
 * goes into the Authorization header of this one request.
 */
export async function fetchEngineHealth({ port, baseUrl = null, token, fetchImpl = globalThis.fetch, timeoutMs = 10_000 }) {
  const base = baseUrl ?? engineBaseUrl(port);
  const live = await fetchImpl(`${base}/livez`, { signal: AbortSignal.timeout(timeoutMs), redirect: "error" });
  await live.arrayBuffer().catch(() => {});
  if (live.status !== 204) throw new Error("Engine liveness probe failed.");
  const response = await fetchImpl(`${base}/v1/docs/health`, {
    headers: { Authorization: `Bearer ${token}`, "OO-Docs-Api-Min": "1.0", Accept: "application/json" },
    signal: AbortSignal.timeout(timeoutMs),
    redirect: "error",
  });
  return {
    httpStatus: response.status,
    apiHeader: response.headers.get("OO-Docs-Api"),
    body: await readLimitedJson(response),
  };
}

const majorOf = (value) => {
  const major = Number.parseInt(String(value ?? "").split(".")[0], 10);
  return Number.isInteger(major) ? major : null;
};

/** Maps one health round to a lifecycle outcome (20-frontend §1.4.1 health patrol). */
export function classifyHealth({ httpStatus, apiHeader, body }) {
  const headerMajor = majorOf(apiHeader);
  if (headerMajor !== null && headerMajor !== 1) return { kind: "incompatible", code: "SO_ENGINE_INCOMPATIBLE" };
  const code = typeof body?.code === "string" ? body.code : null;
  if (code === "DOCS_ENGINE_VERSION_UNSUPPORTED" || code === "DOCS_ENGINE_SCHEMA_MISMATCH") {
    return { kind: "incompatible", code };
  }
  if (httpStatus === 401) return { kind: "unauthenticated" };
  if (httpStatus === 200 && body && typeof body.status === "string") {
    const bodyMajor = majorOf(body.api_version);
    if (bodyMajor !== null && bodyMajor !== 1) return { kind: "incompatible", code: "SO_ENGINE_INCOMPATIBLE" };
    if (body.status === "ok") return { kind: "ok", health: body };
    if (body.status === "degraded") return { kind: "degraded", health: body };
    if (body.status === "starting") return { kind: "starting" };
    if (body.status === "unavailable") return { kind: "container", code: "DOCS_ENGINE_UNAVAILABLE" };
  }
  if (httpStatus === 503) {
    if (code === "DOCS_ENGINE_STARTING") return { kind: "starting" };
    if (code === "DOCS_ENGINE_UNAVAILABLE" || code === "DOCS_ENGINE_DB_UNAVAILABLE") return { kind: "container", code };
  }
  return { kind: "unreachable" };
}
