/** Same-origin B01 client. A caller JSON object is not a session. */

import { callOperation, csrfFromCurrent, registeredTransport } from "@open-ontology/sdk-ts";

export const MANAGEMENT_CAPABILITIES = [
  "audit.read",
  "binding.read",
  "business.record",
  "identity.current",
  "ontology.draft",
  "project.admin",
  "review.decide",
  "session.revoke",
] as const;

export type ManagementCapability = (typeof MANAGEMENT_CAPABILITIES)[number];

export type ManagementProject = {
  projectId: string;
  role: "admin" | "reviewer" | "steward" | "auditor" | "business_owner";
  capabilities: ManagementCapability[];
};

export type ManagementSession = {
  subject: string;
  issuer: string;
  configCommit: string;
  revision: number;
  installState: "uninitialized" | "initialized";
  projects: ManagementProject[];
  csrfToken: string;
  customerIntegration: "not_claimed";
  dataGrants: "none";
};

export type IdentityView =
  | { status: "pending" }
  | { status: "absent" }
  | { status: "anonymous" }
  | { status: "session"; session: ManagementSession };

const ROLES = new Set(["admin", "reviewer", "steward", "auditor", "business_owner"]);
const CAPABILITIES = new Set<string>(MANAGEMENT_CAPABILITIES);

export function loginHref(returnTo: "/account" | "/admin/bootstrap"): string {
  return `/control/v1/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}

export function hasCapability(session: ManagementSession, capability: ManagementCapability): boolean {
  return session.projects.some((project) => project.capabilities.includes(capability));
}

export function adminGateDecision(view: IdentityView): "wait" | "login" | "bootstrap" | "forbidden" | "allow" {
  if (view.status === "pending") {
    return "wait";
  }
  if (view.status !== "session") {
    return "login";
  }
  if (view.session.installState === "uninitialized") {
    return "bootstrap";
  }
  if (!hasCapability(view.session, "project.admin")) {
    return "forbidden";
  }
  return "allow";
}

export function parseManagementSession(value: unknown): ManagementSession | null {
  if (!isRecord(value)) {
    return null;
  }
  if ("authorized" in value) {
    return null;
  }
  const keys = Object.keys(value).sort();
  const expected = [
    "agent_session",
    "authority",
    "config_commit",
    "contract",
    "csrf_token",
    "customer_integration",
    "data_grants",
    "gateway_token",
    "harness_session",
    "install_state",
    "issuer",
    "operation",
    "owner_certification",
    "projects",
    "revision",
    "session",
    "session_class",
    "subject",
  ];
  if (keys.join() !== expected.join()) {
    return null;
  }
  if (
    value.contract !== "oo.control-identity/v1" ||
    value.operation !== "getIdentityCurrent" ||
    value.authority !== "management_session" ||
    value.session_class !== "management_cookie" ||
    value.data_grants !== "none" ||
    value.owner_certification !== "not_issued" ||
    value.agent_session !== "not_issued" ||
    value.harness_session !== "not_issued" ||
    value.gateway_token !== "not_issued" ||
    value.customer_integration !== "not_claimed"
  ) {
    return null;
  }
  if (typeof value.subject !== "string" || value.subject.length < 1 || value.subject.length > 256 || /\s/.test(value.subject)) {
    return null;
  }
  if (typeof value.issuer !== "string" || !value.issuer.startsWith("https://") || value.issuer.length < 8) {
    return null;
  }
  if (typeof value.config_commit !== "string" || !/^[0-9a-f]{64}$/.test(value.config_commit)) {
    return null;
  }
  if (typeof value.revision !== "number" || !Number.isInteger(value.revision) || value.revision < 1) {
    return null;
  }
  if (value.install_state !== "uninitialized" && value.install_state !== "initialized") {
    return null;
  }
  if (typeof value.csrf_token !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(value.csrf_token)) {
    return null;
  }
  if (!isRecord(value.session) || value.session.revoked !== false) {
    return null;
  }
  if (typeof value.session.idle_expires_at !== "string" || typeof value.session.absolute_expires_at !== "string") {
    return null;
  }
  if (!Array.isArray(value.projects)) {
    return null;
  }
  const projects: ManagementProject[] = [];
  for (const item of value.projects) {
    const project = parseProject(item);
    if (!project) {
      return null;
    }
    projects.push(project);
  }
  if (value.install_state === "uninitialized" && projects.length !== 0) {
    return null;
  }
  return {
    subject: value.subject,
    issuer: value.issuer,
    configCommit: value.config_commit,
    revision: value.revision,
    installState: value.install_state,
    projects,
    csrfToken: value.csrf_token,
    customerIntegration: "not_claimed",
    dataGrants: "none",
  };
}

function browserTransportReady(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  const transport = registeredTransport();
  return transport !== null && transport.origin === window.location.origin;
}

export async function fetchIdentityCurrent(): Promise<IdentityView> {
  if (browserTransportReady()) {
    const result = await callOperation({ operationId: "getIdentityCurrent" });
    if (!result.ok && result.status === 401) {
      return { status: "anonymous" };
    }
    if (!result.ok) {
      return { status: "absent" };
    }
    const session = parseManagementSession(result.body);
    return session ? { status: "session", session } : { status: "absent" };
  }
  let response: Response;
  try {
    response = await fetch("/control/v1/identity/current", {
      credentials: "include",
      headers: { accept: "application/json" },
    });
  } catch {
    return { status: "absent" };
  }
  if (response.status === 401) {
    return { status: "anonymous" };
  }
  if (response.status !== 200) {
    return { status: "absent" };
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return { status: "absent" };
  }
  const session = parseManagementSession(body);
  return session ? { status: "session", session } : { status: "absent" };
}

export async function logoutManagementSession(csrfToken: string): Promise<"revoked" | "rejected"> {
  if (browserTransportReady()) {
    const current = await callOperation({ operationId: "getIdentityCurrent" });
    if (!current.ok) {
      return "rejected";
    }
    const csrf = csrfFromCurrent(current.body);
    if (!csrf.ok || csrf.token !== csrfToken) {
      return "rejected";
    }
    const result = await callOperation({ operationId: "logoutManagementSession", current: current.body });
    return result.ok ? "revoked" : "rejected";
  }
  let response: Response;
  try {
    response = await fetch("/control/v1/auth/logout", {
      method: "POST",
      credentials: "include",
      headers: { accept: "application/json", "x-oo-csrf": csrfToken },
    });
  } catch {
    return "rejected";
  }
  if (response.status !== 200) {
    return "rejected";
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return "rejected";
  }
  if (!isRecord(body) || body.outcome !== "revoked" || body.customer_integration !== "not_claimed") {
    return "rejected";
  }
  return "revoked";
}

export async function consumeBootstrapToken(csrfToken: string, token: string): Promise<"consumed" | "rejected"> {
  if (browserTransportReady()) {
    const current = await callOperation({ operationId: "getIdentityCurrent" });
    if (!current.ok) {
      return "rejected";
    }
    const csrf = csrfFromCurrent(current.body);
    if (!csrf.ok || csrf.token !== csrfToken) {
      return "rejected";
    }
    const result = await callOperation({
      operationId: "consumeBootstrapToken",
      current: current.body,
      body: { token },
    });
    return result.ok ? "consumed" : "rejected";
  }
  let response: Response;
  try {
    response = await fetch("/control/v1/auth/bootstrap/consume", {
      method: "POST",
      credentials: "include",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        "x-oo-csrf": csrfToken,
      },
      body: JSON.stringify({ token }),
    });
  } catch {
    return "rejected";
  }
  if (response.status !== 200) {
    return "rejected";
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return "rejected";
  }
  if (
    !isRecord(body) ||
    body.outcome !== "consumed" ||
    body.install_state !== "initialized" ||
    body.customer_integration !== "not_claimed" ||
    body.customer_owner !== false ||
    body.fixture_class !== "explicit_test_fixture"
  ) {
    return "rejected";
  }
  return "consumed";
}

function parseProject(value: unknown): ManagementProject | null {
  if (!isRecord(value)) {
    return null;
  }
  if (Object.keys(value).sort().join() !== "capabilities,project_id,role") {
    return null;
  }
  if (typeof value.project_id !== "string" || !/^[a-z][a-z0-9_]{0,62}$/.test(value.project_id)) {
    return null;
  }
  if (typeof value.role !== "string" || !ROLES.has(value.role)) {
    return null;
  }
  if (!Array.isArray(value.capabilities) || value.capabilities.some((item) => typeof item !== "string" || !CAPABILITIES.has(item))) {
    return null;
  }
  return {
    projectId: value.project_id,
    role: value.role as ManagementProject["role"],
    capabilities: value.capabilities as ManagementCapability[],
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
