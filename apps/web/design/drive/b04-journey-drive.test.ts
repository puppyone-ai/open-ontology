import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { callOperation, registerSameOriginTransport, type CallResult } from "@open-ontology/sdk-ts";

const origin = process.env.FE01_B01_ORIGIN ?? "";
const bootstrapFile = process.env.FE01_BOOTSTRAP_FILE ?? "";
const evidence = process.env.FE01_EVIDENCE ?? "";
const absentBuild = `b_${"0".repeat(32)}`;
const columnId = "public.party.id";

type Jar = Map<string, Map<string, string>>;
type Note = { operationId: string; status: number | null; ok: boolean; reason: string | null; errorCode: string | null };

function recordOf(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function cookieHeader(jar: Jar, host: string): string {
  const rows = jar.get(host);
  if (!rows || rows.size === 0) return "";
  return [...rows.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
}

function storeCookies(jar: Jar, host: string, response: Response): void {
  const reader = response.headers as Headers & { getSetCookie?: () => string[] };
  const lines = reader.getSetCookie?.() ?? [];
  for (const line of lines) {
    const pair = line.split(";", 1)[0] ?? "";
    const eq = pair.indexOf("=");
    if (eq <= 0) continue;
    const name = pair.slice(0, eq).trim();
    const value = pair.slice(eq + 1).trim();
    const rows = jar.get(host) ?? new Map<string, string>();
    if (value.length === 0 || /max-age=0/i.test(line)) rows.delete(name);
    else rows.set(name, value);
    jar.set(host, rows);
  }
}

function jarFetch(jar: Jar): typeof fetch {
  return async (input, init) => {
    const raw = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    const url = new URL(raw);
    const headers = new Headers(init?.headers);
    const cookie = cookieHeader(jar, url.hostname);
    if (cookie.length > 0 && !headers.has("cookie")) headers.set("cookie", cookie);
    const response = await fetch(input, { ...init, headers });
    storeCookies(jar, url.hostname, response);
    return response;
  };
}

async function followToEdge(start: string, edge: string, jar: Jar): Promise<URL> {
  let current = start;
  for (let hop = 0; hop < 4; hop += 1) {
    const url = new URL(current);
    if (url.origin === edge) return url;
    const headers = new Headers();
    const cookie = cookieHeader(jar, url.hostname);
    if (cookie.length > 0) headers.set("cookie", cookie);
    const response = await fetch(url, { redirect: "manual", headers });
    storeCookies(jar, url.hostname, response);
    const location = response.headers.get("location");
    if (response.status !== 302 || !location) throw new Error(`idp status ${response.status}`);
    current = new URL(location, url).toString();
  }
  throw new Error("idp hops exceeded");
}

async function waitSubject(name: string): Promise<void> {
  writeFileSync(`${evidence}/subject.request`, `${name}\n`);
  const started = Date.now();
  while (Date.now() - started < 20000) {
    const applied = readFileSync(`${evidence}/subject.applied`, "utf8").trim();
    if (applied === name) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`subject ${name} was not applied`);
}

function revisionOf(body: unknown): number {
  const revision = recordOf(body)?.revision;
  if (typeof revision !== "number") throw new Error("revision missing");
  return revision;
}

describe("build review and classification calls on the fixed selfbuild service", () => {
  it("sends every published B04 operation and records the service result", async () => {
    expect(process.env.FE01_B04_DRIVE).toBe("1");
    expect(origin.startsWith("https://")).toBe(true);
    const jar: Jar = new Map();
    const operations: Note[] = [];
    expect(registerSameOriginTransport({ origin, pageOrigin: origin, fetchImpl: jarFetch(jar) }).ok).toBe(true);
    const note = (operationId: string, result: CallResult): CallResult => {
      operations.push({
        operationId,
        status: result.status,
        ok: result.ok,
        reason: result.ok ? null : result.reason,
        errorCode: result.ok ? null : result.errorCode,
      });
      return result;
    };
    const login = async () => {
      const started = note("startAuthorizationCodeLogin", await callOperation({ operationId: "startAuthorizationCodeLogin", query: { return_to: "/account" } }));
      expect(started.ok).toBe(true);
      if (!started.ok || !started.location) throw new Error("login location missing");
      const callback = await followToEdge(started.location, origin, jar);
      const done = note(
        "completeAuthorizationCodeCallback",
        await callOperation({
          operationId: "completeAuthorizationCodeCallback",
          query: { code: callback.searchParams.get("code") ?? "", state: callback.searchParams.get("state") ?? "" },
        }),
      );
      expect(done.ok).toBe(true);
    };
    const current = async () => note("getIdentityCurrent", await callOperation({ operationId: "getIdentityCurrent" }));
    const rotate = async () => {
      const next = await current();
      expect(next.ok).toBe(true);
      if (!next.ok) throw new Error("current missing after a selfbuild call");
      return next.body;
    };
    const expectHttp = (result: CallResult, status: number, code: string) => {
      expect(result.ok).toBe(false);
      if (result.ok) throw new Error("expected an error document");
      expect(result.fetched).toBe(true);
      expect(result.reason).toBe("http_error");
      expect(result.status).toBe(status);
      expect(result.errorCode).toBe(code);
      expect(result.live).toBe(false);
      expect(result.journey).toBe(false);
      expect(result.release).toBe(false);
    };

    await waitSubject("fixture-admin");
    await login();
    const before = await current();
    expect(before.ok).toBe(true);
    if (!before.ok) throw new Error("current missing");
    const token = readFileSync(bootstrapFile, "utf8").trim();
    const consumed = note("consumeBootstrapToken", await callOperation({ operationId: "consumeBootstrapToken", current: before.body, body: { token } }));
    expect(consumed.ok).toBe(true);
    let session = await rotate();
    const adminBody = recordOf(session);
    expect(recordOf(Array.isArray(adminBody?.projects) ? adminBody.projects[0] : null)?.role).toBe("admin");

    const listed = note("listBuilds", await callOperation({ operationId: "listBuilds", query: { project_id: "alpha" } }));
    expect(listed.ok).toBe(true);
    expect(recordOf(listed.ok ? listed.body : null)?.builds).toEqual([]);
    session = await rotate();

    const created = note("createBuild", await callOperation({
      operationId: "createBuild",
      current: session,
      body: {
        project_id: "alpha",
        identity_revision: revisionOf(session),
        config_commit: "0".repeat(64),
        source_ref: "source_a",
        allowance_id: "allow_a",
        llm_secret_ref: "secret://llm/unused",
        idempotency_key: "journey-create-0001",
      },
    }));
    expectHttp(created, 503, "dependency.not_ready");

    const missing = note("getBuild", await callOperation({ operationId: "getBuild", params: { build_id: absentBuild } }));
    expectHttp(missing, 403, "forbidden");
    session = await rotate();

    const canceled = note("cancelBuild", await callOperation({
      operationId: "cancelBuild",
      params: { build_id: absentBuild },
      current: session,
      body: { identity_revision: revisionOf(session) },
    }));
    expectHttp(canceled, 403, "forbidden");

    const cards = note("getBuildCards", await callOperation({ operationId: "getBuildCards", params: { build_id: absentBuild } }));
    expectHttp(cards, 403, "forbidden");
    const candidates = note("getBuildCandidates", await callOperation({ operationId: "getBuildCandidates", params: { build_id: absentBuild } }));
    expectHttp(candidates, 403, "forbidden");

    const reviews = note("listReviewItems", await callOperation({ operationId: "listReviewItems", query: { project_id: "alpha", build_id: absentBuild } }));
    expect(reviews.ok).toBe(true);
    expect(recordOf(reviews.ok ? reviews.body : null)?.items).toEqual([]);
    const review = note("getReviewItem", await callOperation({
      operationId: "getReviewItem",
      params: { item_id: columnId },
      query: { project_id: "alpha", build_id: absentBuild },
    }));
    expectHttp(review, 403, "forbidden");
    session = await rotate();
    const reviewPreview = note("previewReviewBatch", await callOperation({
      operationId: "previewReviewBatch",
      current: session,
      body: { project_id: "alpha", identity_revision: revisionOf(session), items: [] },
    }));
    expect(reviewPreview.ok).toBe(true);
    expect(recordOf(reviewPreview.ok ? reviewPreview.body : null)?.wrote).toBe(false);
    expect(recordOf(reviewPreview.ok ? reviewPreview.body : null)?.items).toEqual([]);

    const classes = note("listClassifications", await callOperation({ operationId: "listClassifications", query: { project_id: "alpha" } }));
    expect(classes.ok).toBe(true);
    expect(recordOf(classes.ok ? classes.body : null)?.columns).toEqual([{ object_id: columnId, hidden: false }]);
    session = await rotate();
    const classPreview = note("previewClassifications", await callOperation({
      operationId: "previewClassifications",
      current: session,
      body: {
        project_id: "alpha",
        build_id: absentBuild,
        identity_revision: revisionOf(session),
        idempotency_key: "journey-preview-0001",
        items: [{ object_id: columnId, pii: "none", semantic_type: "identifier" }],
      },
    }));
    expect(classPreview.ok).toBe(false);
    if (classPreview.ok) throw new Error("string preview items were accepted");
    expect(classPreview.fetched).toBe(true);
    expect(classPreview.status).toBe(200);
    expect(classPreview.reason).toBe("invalid_schema");
    session = await rotate();

    const compiled = note("compileBuildPack", await callOperation({
      operationId: "compileBuildPack",
      params: { build_id: absentBuild },
      current: session,
      body: { identity_revision: revisionOf(session), idempotency_key: "journey-pack-00001" },
    }));
    expectHttp(compiled, 403, "forbidden");
    const pack = note("getBuildPack", await callOperation({ operationId: "getBuildPack", params: { build_id: absentBuild } }));
    expectHttp(pack, 403, "forbidden");
    session = await rotate();
    const adminOut = note("logoutManagementSession", await callOperation({ operationId: "logoutManagementSession", current: session }));
    expect(adminOut.ok).toBe(true);

    await waitSubject("fixture-reviewer");
    await login();
    session = await rotate();
    expect(recordOf(session)?.subject).toBe("fixture-reviewer");
    const decided = note("decideReviewItem", await callOperation({
      operationId: "decideReviewItem",
      params: { item_id: columnId },
      current: session,
      body: {
        build_id: absentBuild,
        identity_revision: revisionOf(session),
        expected_revision: 1,
        idempotency_key: "journey-decide-0001",
        decision_kind: "label_description",
        choice: "defer",
        candidate_sha256: "0".repeat(64),
      },
    }));
    expectHttp(decided, 403, "forbidden");
    session = await rotate();
    const reviewerOut = note("logoutManagementSession", await callOperation({ operationId: "logoutManagementSession", current: session }));
    expect(reviewerOut.ok).toBe(true);

    await waitSubject("fixture-owner");
    await login();
    session = await rotate();
    expect(recordOf(session)?.subject).toBe("fixture-owner");
    const confirmed = note("confirmClassifications", await callOperation({
      operationId: "confirmClassifications",
      current: session,
      body: {
        project_id: "alpha",
        build_id: absentBuild,
        identity_revision: revisionOf(session),
        idempotency_key: "journey-confirm-0001",
        items: [{ object_id: columnId, pii: "none", semantic_type: "identifier" }],
      },
    }));
    expectHttp(confirmed, 403, "forbidden");
    session = await rotate();
    const ownerOut = note("logoutManagementSession", await callOperation({ operationId: "logoutManagementSession", current: session }));
    expect(ownerOut.ok).toBe(true);

    const called = new Set(operations.map((item) => item.operationId));
    for (const operationId of [
      "createBuild", "listBuilds", "getBuild", "cancelBuild", "getBuildCards", "getBuildCandidates",
      "listReviewItems", "getReviewItem", "previewReviewBatch", "decideReviewItem",
      "listClassifications", "previewClassifications", "confirmClassifications", "compileBuildPack", "getBuildPack",
    ]) {
      expect(called.has(operationId)).toBe(true);
    }
    const receipt = {
      customer_integration: "not_claimed",
      live: false,
      journey: false,
      release: false,
      completed_build: false,
      model_called: false,
      binaries_present: false,
      service_commit: "a6f92da8a0bf872f8a30efb116c2c7b14463505b",
      contract_commit: "089b00d7f70da1bc42f7e201dfa070748c651d5b",
      b04_contract: "7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9",
      b04_implementation: "45644fca81325cf1405a38ceb79435da6e8bb090",
      operations,
    };
    const text = JSON.stringify(receipt);
    expect(text.includes(token)).toBe(false);
    expect(text.includes(String(adminBody?.csrf_token))).toBe(false);
    writeFileSync(`${evidence}/raw/b04-journey-receipt.json`, `${JSON.stringify(receipt, null, 2)}\n`);
  });
});
