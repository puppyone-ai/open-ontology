import { describe, expect, it } from "vitest";
import { adminGateDecision, parseManagementSession } from "./identity-session";

const csrf = "a".repeat(43);

function current(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.control-identity/v1",
    operation: "getIdentityCurrent",
    subject: "fixture-admin",
    issuer: "https://idp.example",
    config_commit: "ab".repeat(32),
    revision: 1,
    install_state: "initialized",
    projects: [{ project_id: "alpha", role: "admin", capabilities: ["binding.read", "identity.current", "project.admin", "session.revoke"] }],
    csrf_token: csrf,
    session: { idle_expires_at: "2026-10-08T00:00:00Z", absolute_expires_at: "2026-10-08T01:00:00Z", revoked: false },
    authority: "management_session",
    session_class: "management_cookie",
    data_grants: "none",
    owner_certification: "not_issued",
    agent_session: "not_issued",
    harness_session: "not_issued",
    gateway_token: "not_issued",
    customer_integration: "not_claimed",
    ...overrides,
  };
}

describe("management session parser", () => {
  it("rejects a caller claim and a customer-live envelope", () => {
    expect(parseManagementSession({ authorized: true })).toBeNull();
    expect(parseManagementSession(current({ customer_integration: "verified" }))).toBeNull();
    expect(parseManagementSession(current({ data_grants: "owner" }))).toBeNull();
    expect(adminGateDecision({ status: "absent" })).toBe("login");
    expect(adminGateDecision({ status: "anonymous" })).toBe("login");
  });

  it("keeps an uninitialized session on bootstrap and requires project.admin after that", () => {
    const uninitialized = parseManagementSession(current({ install_state: "uninitialized", projects: [] }));
    expect(uninitialized?.installState).toBe("uninitialized");
    expect(adminGateDecision({ status: "session", session: uninitialized! })).toBe("bootstrap");
    const reviewer = parseManagementSession(
      current({
        projects: [{ project_id: "beta", role: "reviewer", capabilities: ["identity.current", "review.decide"] }],
      }),
    );
    expect(adminGateDecision({ status: "session", session: reviewer! })).toBe("forbidden");
    const admin = parseManagementSession(current());
    expect(adminGateDecision({ status: "session", session: admin! })).toBe("allow");
    expect(admin?.customerIntegration).toBe("not_claimed");
  });

  it("forbids steward, auditor, and business_owner without project.admin", () => {
    const roles = [
      ["steward", ["identity.current", "ontology.draft"]],
      ["auditor", ["audit.read", "identity.current"]],
      ["business_owner", ["business.record", "identity.current"]],
    ] as const;
    for (const [role, capabilities] of roles) {
      const session = parseManagementSession(
        current({
          subject: role === "business_owner" ? "fixture-owner" : `fixture-${role}`,
          projects: [{ project_id: "alpha", role, capabilities }],
        }),
      );
      expect(session?.projects[0]?.role).toBe(role);
      expect(session?.projects[0]?.capabilities.includes("project.admin")).toBe(false);
      expect(adminGateDecision({ status: "session", session: session! })).toBe("forbidden");
    }
  });
});
