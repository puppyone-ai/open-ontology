import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { callOperation, parseBuildList, registerSameOriginTransport, type CallResult } from "@open-ontology/sdk-ts";

const origin = process.env.FE01_B01_ORIGIN ?? "";
const bootstrapFile = process.env.FE01_BOOTSTRAP_FILE ?? "";
const evidence = process.env.FE01_EVIDENCE ?? "";
const absentBuild = `b_${"0".repeat(32)}`;

type Jar = Map<string, Map<string, string>>;

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

describe("listBuilds against the fixed selfbuild service", () => {
  it("records an empty admin list and a business-owner refusal", async () => {
    expect(process.env.FE01_B04_DRIVE).toBe("1");
    expect(origin.startsWith("https://")).toBe(true);
    const jar: Jar = new Map();
    const operations: Array<{ operationId: string; status: number | null; ok: boolean; reason: string | null; errorCode: string | null; count: number | null }> = [];
    expect(registerSameOriginTransport({ origin, pageOrigin: origin, fetchImpl: jarFetch(jar) }).ok).toBe(true);

    const note = (operationId: string, result: CallResult) => {
      const parsed = result.ok ? parseBuildList(result.body) : null;
      operations.push({
        operationId,
        status: result.status,
        ok: result.ok,
        reason: result.ok ? null : result.reason,
        errorCode: result.ok ? null : result.errorCode,
        count: parsed && parsed.ok ? parsed.count : null,
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

    await login();
    const before = await current();
    expect(before.ok).toBe(true);
    if (!before.ok) throw new Error("current missing");
    const open = recordOf(before.body);
    expect(open?.subject).toBe("fixture-admin");
    expect(open?.install_state).toBe("uninitialized");
    const token = readFileSync(bootstrapFile, "utf8").trim();
    const consumed = note("consumeBootstrapToken", await callOperation({ operationId: "consumeBootstrapToken", current: before.body, body: { token } }));
    expect(consumed.ok).toBe(true);
    expect(consumed.live).toBe(false);

    const admin = await current();
    expect(admin.ok).toBe(true);
    if (!admin.ok) throw new Error("admin current missing");
    const adminBody = recordOf(admin.body);
    const adminProject = Array.isArray(adminBody?.projects) ? recordOf(adminBody.projects[0]) : null;
    expect(adminBody?.install_state).toBe("initialized");
    expect(adminProject?.project_id).toBe("alpha");
    expect(adminProject?.role).toBe("admin");

    const listed = note("listBuilds", await callOperation({ operationId: "listBuilds", query: { project_id: "alpha" } }));
    expect(listed.ok).toBe(true);
    if (!listed.ok) throw new Error("list failed");
    expect(listed.status).toBe(200);
    expect(listed.live).toBe(false);
    expect(listed.journey).toBe(false);
    expect(listed.release).toBe(false);
    expect(listed.customerIntegration).toBe("not_claimed");
    const listBody = recordOf(listed.body);
    expect(listBody?.operation).toBe("listBuilds");
    expect(listBody?.builds).toEqual([]);
    expect(listBody?.published).toBe(false);
    expect(listBody?.active).toBe(false);
    expect(listBody?.compiled_pack).toBe("not_published");
    expect(parseBuildList(listed.body)).toEqual({ ok: true, count: 0 });

    const missing = note("getBuild", await callOperation({ operationId: "getBuild", params: { build_id: absentBuild } }));
    expect(missing.ok).toBe(false);
    if (missing.ok) throw new Error("missing build was accepted");
    expect(missing.fetched).toBe(true);
    expect(missing.reason).toBe("http_error");
    expect(missing.status).toBe(403);
    expect(missing.errorCode).toBe("forbidden");

    const adminFresh = await current();
    expect(adminFresh.ok).toBe(true);
    if (!adminFresh.ok) throw new Error("admin session missing after the build reads");
    const revoked = note("logoutManagementSession", await callOperation({ operationId: "logoutManagementSession", current: adminFresh.body }));
    expect(revoked.ok, JSON.stringify({ status: revoked.status, reason: revoked.ok ? null : revoked.reason, errorCode: revoked.ok ? null : revoked.errorCode })).toBe(true);
    const gone = await current();
    expect(gone.ok).toBe(false);
    if (gone.ok) throw new Error("admin session remained");
    expect(gone.status).toBe(401);

    await waitSubject("fixture-owner");
    await login();
    const owner = await current();
    expect(owner.ok).toBe(true);
    if (!owner.ok) throw new Error("owner current missing");
    const ownerBody = recordOf(owner.body);
    const ownerProject = Array.isArray(ownerBody?.projects) ? recordOf(ownerBody.projects[0]) : null;
    expect(ownerBody?.subject).toBe("fixture-owner");
    expect(ownerProject?.role).toBe("business_owner");
    const refused = note("listBuilds", await callOperation({ operationId: "listBuilds", query: { project_id: "alpha" } }));
    expect(refused.ok).toBe(false);
    if (refused.ok) throw new Error("business owner listed builds");
    expect(refused.reason).toBe("http_error");
    expect(refused.status).toBe(403);
    expect(refused.errorCode).toBe("forbidden");
    const ownerFresh = await current();
    expect(ownerFresh.ok).toBe(true);
    if (!ownerFresh.ok) throw new Error("owner session missing after the refusal");
    const ownerOut = note("logoutManagementSession", await callOperation({ operationId: "logoutManagementSession", current: ownerFresh.body }));
    expect(ownerOut.ok, JSON.stringify({ status: ownerOut.status, reason: ownerOut.ok ? null : ownerOut.reason, errorCode: ownerOut.ok ? null : ownerOut.errorCode })).toBe(true);

    const receipt = {
      customer_integration: "not_claimed",
      live: false,
      journey: false,
      release: false,
      service_commit: "a6f92da8a0bf872f8a30efb116c2c7b14463505b",
      contract_commit: "089b00d7f70da1bc42f7e201dfa070748c651d5b",
      b04_contract: "7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9",
      b04_implementation: "45644fca81325cf1405a38ceb79435da6e8bb090",
      list_count: 0,
      published: false,
      active: false,
      compiled_pack: "not_published",
      operations,
    };
    const text = JSON.stringify(receipt);
    expect(text.includes(token)).toBe(false);
    expect(text.includes(String(adminBody?.csrf_token))).toBe(false);
    writeFileSync(`${evidence}/raw/b04-receipt.json`, `${JSON.stringify(receipt, null, 2)}\n`);
  });
});
