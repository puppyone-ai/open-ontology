import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { callOperation, registerSameOriginTransport, type CallResult } from "@open-ontology/sdk-ts";

const origin = process.env.FE01_B01_ORIGIN ?? "";
const bootstrapFile = process.env.FE01_BOOTSTRAP_FILE ?? "";
const evidence = process.env.FE01_EVIDENCE ?? "";

const paths: Record<string, string> = {
  startAuthorizationCodeLogin: "/control/v1/auth/login",
  completeAuthorizationCodeCallback: "/control/v1/auth/callback",
  getIdentityCurrent: "/control/v1/identity/current",
  logoutManagementSession: "/control/v1/auth/logout",
  consumeBootstrapToken: "/control/v1/auth/bootstrap/consume",
  listBuilds: "/control/v1/builds",
};

const denied = [
  { subject: "fixture-reviewer", role: "reviewer", capabilities: ["identity.current", "review.decide"] },
  { subject: "fixture-steward", role: "steward", capabilities: ["identity.current", "ontology.draft"] },
  { subject: "fixture-auditor", role: "auditor", capabilities: ["audit.read", "identity.current"] },
  { subject: "fixture-owner", role: "business_owner", capabilities: ["business.record", "identity.current"] },
] as const;

type Jar = Map<string, Map<string, string>>;
type ReceiptOp = { operationId: string; path: string; status: number | null };

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
    if (response.status !== 302 || !location) {
      throw new Error(`idp status ${response.status}`);
    }
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

describe("same-origin client against the test issuer", () => {
  it("drives five roles and records an honest builds failure", async () => {
    expect(origin.startsWith("https://")).toBe(true);
    expect(bootstrapFile.length).toBeGreaterThan(0);
    expect(evidence.length).toBeGreaterThan(0);
    const jar: Jar = new Map();
    const operations: ReceiptOp[] = [];
    const registered = registerSameOriginTransport({ origin, pageOrigin: origin, fetchImpl: jarFetch(jar) });
    expect(registered.ok).toBe(true);

    const note = (result: CallResult, operationId: string) => {
      operations.push({ operationId, path: paths[operationId] ?? operationId, status: result.ok ? result.status : result.status });
      return result;
    };

    const login = async (returnTo: "/account" | "/admin/bootstrap") => {
      const started = note(
        await callOperation({ operationId: "startAuthorizationCodeLogin", query: { return_to: returnTo } }),
        "startAuthorizationCodeLogin",
      );
      expect(started.ok).toBe(true);
      if (!started.ok || !started.location) throw new Error("login location missing");
      const callback = await followToEdge(started.location, origin, jar);
      const code = callback.searchParams.get("code");
      const state = callback.searchParams.get("state");
      expect(callback.pathname).toBe("/control/v1/auth/callback");
      expect(code).toBeTruthy();
      expect(state).toBeTruthy();
      const done = note(
        await callOperation({
          operationId: "completeAuthorizationCodeCallback",
          query: { code: code ?? "", state: state ?? "" },
        }),
        "completeAuthorizationCodeCallback",
      );
      expect(done.ok).toBe(true);
    };

    const current = async () => {
      const result = note(await callOperation({ operationId: "getIdentityCurrent" }), "getIdentityCurrent");
      return result;
    };

    const projectOf = (result: CallResult) => {
      expect(result.ok).toBe(true);
      if (!result.ok) throw new Error("current failed");
      const body = recordOf(result.body);
      const projects = body?.projects;
      const project = Array.isArray(projects) ? recordOf(projects[0]) : null;
      return { body, project };
    };

    await login("/account");
    const before = await current();
    const open = projectOf(before);
    expect(open.body?.subject).toBe("fixture-admin");
    expect(open.body?.install_state).toBe("uninitialized");
    expect(open.body?.projects).toEqual([]);
    expect(open.body?.customer_integration).toBe("not_claimed");

    const token = readFileSync(bootstrapFile, "utf8").trim();
    const consumed = note(
      await callOperation({ operationId: "consumeBootstrapToken", current: before.ok ? before.body : null, body: { token } }),
      "consumeBootstrapToken",
    );
    expect(consumed.ok).toBe(true);
    expect(consumed.live).toBe(false);

    const admin = await current();
    const adminView = projectOf(admin);
    expect(adminView.body?.install_state).toBe("initialized");
    expect(adminView.project?.role).toBe("admin");
    expect(adminView.project?.project_id).toBe("alpha");
    expect(adminView.project?.capabilities).toEqual(["binding.read", "identity.current", "project.admin", "session.revoke"]);
    const realCsrf = adminView.body?.csrf_token;
    expect(typeof realCsrf).toBe("string");
    const forged = recordOf(structuredClone(admin.ok ? admin.body : null));
    if (!forged) throw new Error("admin body missing");
    forged.csrf_token = "b".repeat(43);
    expect(forged.csrf_token).not.toBe(realCsrf);
    const wrong = note(await callOperation({ operationId: "logoutManagementSession", current: forged }), "logoutManagementSession");
    expect(wrong.ok).toBe(false);
    if (wrong.ok) throw new Error("bad csrf was accepted");
    expect(wrong.status).toBe(403);
    expect(wrong.errorCode).toBe("csrf.rejected");
    const still = await current();
    expect(projectOf(still).project?.role).toBe("admin");

    const fresh = await current();
    const revoked = note(
      await callOperation({ operationId: "logoutManagementSession", current: fresh.ok ? fresh.body : null }),
      "logoutManagementSession",
    );
    expect(revoked.ok).toBe(true);
    const gone = await current();
    expect(gone.ok).toBe(false);
    if (gone.ok) throw new Error("session remained");
    expect(gone.status).toBe(401);

    for (const role of denied) {
      await waitSubject(role.subject);
      await login("/account");
      const view = projectOf(await current());
      expect(view.body?.subject).toBe(role.subject);
      expect(view.body?.install_state).toBe("initialized");
      expect(view.project?.role).toBe(role.role);
      expect(view.project?.project_id).toBe("alpha");
      expect(view.project?.capabilities).toEqual([...role.capabilities]);
      expect(JSON.stringify(view.project?.capabilities).includes("project.admin")).toBe(false);
      const session = await current();
      const out = note(
        await callOperation({ operationId: "logoutManagementSession", current: session.ok ? session.body : null }),
        "logoutManagementSession",
      );
      expect(out.ok).toBe(true);
      const after = await current();
      expect(after.ok).toBe(false);
      if (!after.ok) expect(after.status).toBe(401);
    }

    const listed = note(await callOperation({ operationId: "listBuilds" }), "listBuilds");
    expect(listed.fetched).toBe(true);
    expect(listed.ok).toBe(false);
    if (!listed.ok) expect(listed.reason).not.toBe("not_connected");

    const ready = JSON.parse(readFileSync(`${evidence}/ready.json`, "utf8")) as { config_commit?: string };
    const receipt = {
      customer_integration: "not_claimed",
      live: false,
      journey: false,
      release: false,
      fixture_class: "explicit_test_fixture",
      proved_roles: ["admin", "reviewer", "steward", "auditor", "business_owner"],
      proved_project: "alpha",
      proved_subject: "fixture-admin",
      service_commit: "a6f92da8a0bf872f8a30efb116c2c7b14463505b",
      contract_commit: "089b00d7f70da1bc42f7e201dfa070748c651d5b",
      config_commit: typeof adminView.body?.config_commit === "string" ? adminView.body.config_commit : null,
      ready_config_commit: ready.config_commit ?? null,
      b04: "schema_frozen",
      b04_list_builds: listed.ok
        ? { fetched: true, ok: true }
        : { fetched: listed.fetched, ok: false, status: listed.status, reason: listed.reason, errorCode: listed.errorCode },
      wrong_csrf_status: 403,
      operations,
    };
    expect(receipt.config_commit).toBe(receipt.ready_config_commit);
    expect(operations.filter((item) => item.path === "/control/v1/auth/login" && item.status === 302).length).toBe(5);
    expect(operations.filter((item) => item.path === "/control/v1/auth/logout" && item.status === 200).length).toBe(5);
    expect(operations.filter((item) => item.path === "/control/v1/auth/logout" && item.status === 403).length).toBe(1);
    writeFileSync(`${evidence}/raw/client-receipt.json`, `${JSON.stringify(receipt, null, 2)}\n`);
    expect(JSON.stringify(operations).includes(token)).toBe(false);
    expect(JSON.stringify(operations).includes(String(realCsrf))).toBe(false);
  });
});
