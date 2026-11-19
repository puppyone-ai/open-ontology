import { describe, expect, it, vi } from "vitest";
import { createEnterpriseHandlers } from "@so-main/enterprise-handlers.mjs";
import { ROUTES } from "@so-main/control-client.mjs";
import { toErrorResult, SuperOntologyError } from "@so-shared/errors.mjs";
import { ENTERPRISE_CHANNELS, SUPER_ONTOLOGY_CHANNELS, parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const DRIVE = "00000000-0000-4000-8000-000000000001";
const TOKEN_ID = "00000000-0000-4000-8000-0000000000d1";
const row = { seq: 3, at: "2026-11-18T09:13:21Z", actor: "cloud:x", tenant: "acme", drive_id: DRIVE, share_id: "s1", agent: "agent-7", grantor: "owner", role: "suggester", scope: ["Projects"], revoked_at: null, refs_reads: 0 };
const make = (respond) => {
  const calls = [];
  const client = { request: vi.fn(async (name, options) => { calls.push({ name, options }); return respond(name, options); }) };
  return { handlers: createEnterpriseHandlers({ client }), calls };
};

describe("L7 audit and token channels (F-S12-02)", () => {
  it("are listed after the others, have routes on the loopback API and need no operation id", () => {
    expect([...ENTERPRISE_CHANNELS]).toEqual(["cloud-mode-read", "cloud-mode-set", "audit-shares", "audit-verify", "cloud-token-create", "cloud-token-revoke", "engine-set-runtime", "engine-connect-remote", "engine-disconnect-remote"]);
    expect(SUPER_ONTOLOGY_CHANNELS).toEqual(expect.arrayContaining([...ENTERPRISE_CHANNELS]));
    expect([ROUTES.auditShares, ROUTES.auditVerify, ROUTES.cloudTokenCreate, ROUTES.cloudTokenRevoke].map((route) => [route.method, route.template, route.cloud === true])).toEqual([
      ["GET", "/v1/docs/audit/shares", false], ["GET", "/v1/docs/audit/verify", false], ["POST", "/v1/docs/cloud-tokens", false], ["DELETE", "/v1/docs/cloud-tokens/{id}", false],
    ]);
  });

  it("reads the audit with the filters the engine knows and returns the rows untouched", async () => {
    const { handlers, calls } = make(() => ({ value: { items: [row] } }));
    const result = await handlers["audit-shares"]({ driveId: DRIVE, agent: "agent-7", tenant: "acme", since: "2026-11-01T00:00:00Z", until: "2026-11-30T00:00:00Z" });
    expect(result.items).toEqual([row]);
    expect(calls[0]).toEqual({ name: "auditShares", options: { query: { drive_id: DRIVE, agent: "agent-7", tenant: "acme", since: "2026-11-01T00:00:00Z", until: "2026-11-30T00:00:00Z" } } });
    await expect(make(() => ({ value: { items: [{ seq: "x" }] } })).handlers["audit-shares"]({})).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });

  it("has no write channel for the audit: nothing here can update, delete or truncate it", () => {
    const { handlers } = make(() => ({ value: {} }));
    expect(Object.keys(handlers).filter((name) => name.startsWith("audit-"))).toEqual(["audit-shares", "audit-verify"]);
    for (const channel of ["audit-update", "audit-delete", "audit-clear"]) expect(() => parseSuperOntologyRequest(channel, {})).toThrow();
  });

  it("verifies the hash chain", async () => {
    expect(await make(() => ({ value: { intact: true, rows: 9 } })).handlers["audit-verify"]({})).toEqual({ intact: true, rows: 9 });
    expect(await make(() => ({ value: { intact: false, broken_at_seq: 4 } })).handlers["audit-verify"]({})).toMatchObject({ intact: false, broken_at_seq: 4 });
  });

  it("issues a cloud token once, with the Drives it is bound to, and refuses an answer that is not a token", async () => {
    const issued = { id: TOKEN_ID, token: "oodc_abcdefghijklmnopqrstuvwxyz0123456789", tenant: "acme", drive_ids: [DRIVE], expires_in_seconds: 86_400 };
    const { handlers, calls } = make(() => ({ value: issued }));
    expect(await handlers["cloud-token-create"]({ tenant: "acme", driveIds: [DRIVE], label: "worker", expiresInSeconds: 86_400 })).toEqual(issued);
    expect(calls[0].options.body).toEqual({ tenant: "acme", drive_ids: [DRIVE], label: "worker", expires_in_seconds: 86_400 });
    await expect(make(() => ({ value: { ...issued, token: "oodt_wrong" } })).handlers["cloud-token-create"]({ tenant: "acme", driveIds: [DRIVE] })).rejects.toMatchObject({ code: "SO_RESPONSE_INVALID" });
  });

  it("revokes a token, and never lets the token into an error", async () => {
    const { handlers, calls } = make(() => ({ value: null }));
    expect(await handlers["cloud-token-revoke"]({ tokenId: TOKEN_ID })).toEqual({ revoked: true });
    expect(calls[0]).toEqual({ name: "cloudTokenRevoke", options: { params: { id: TOKEN_ID } } });
    const failure = toErrorResult(new SuperOntologyError("DOCS_ENGINE_NOT_FOUND", { details: { echo: "oodc_secret" } }));
    expect(JSON.stringify(failure)).not.toContain("oodc_secret");
  });

  it("validates the requests", () => {
    expect(parseSuperOntologyRequest("audit-shares", { driveId: DRIVE, agent: "a", since: "2026-11-01" })).toEqual({ driveId: DRIVE, agent: "a", since: "2026-11-01" });
    expect(parseSuperOntologyRequest("cloud-token-create", { tenant: "acme", driveIds: [DRIVE], expiresInSeconds: 3600 })).toMatchObject({ tenant: "acme" });
    for (const [channel, request] of [
      ["audit-shares", { driveId: "x" }], ["audit-shares", { limit: 5 }], ["cloud-token-create", { tenant: "", driveIds: [DRIVE] }], ["cloud-token-create", { tenant: "a", driveIds: [] }],
      ["cloud-token-create", { tenant: "a", driveIds: ["x"] }], ["cloud-token-create", { tenant: "a", driveIds: [DRIVE], expiresInSeconds: 5 }], ["cloud-token-revoke", { tokenId: "nope" }], ["audit-verify", { x: 1 }],
    ]) expect(() => parseSuperOntologyRequest(channel, request), `${channel} ${JSON.stringify(request)}`).toThrow(expect.objectContaining({ code: "SO_INVALID_REQUEST" }));
  });
});
