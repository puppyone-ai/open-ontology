import type { AuditShareRecord, AuditSharesQuery } from "../../../shared/super-ontology/bridge";

/** Filter fields of the audit view; dates are the day a person picked (`YYYY-MM-DD`), not timestamps. */
export type AuditFilters = { driveId: string; agent: string; tenant: string; since: string; until: string };
export const EMPTY_AUDIT_FILTERS: AuditFilters = { driveId: "", agent: "", tenant: "", since: "", until: "" };

const DAY = /^\d{4}-\d{2}-\d{2}$/u;

/** The engine's query for the given filters: a whole day from its first to its last second (UTC), empty fields left out. */
export function toAuditQuery(filters: AuditFilters): AuditSharesQuery {
  const agent = filters.agent.trim();
  const tenant = filters.tenant.trim();
  return {
    ...(filters.driveId ? { driveId: filters.driveId } : {}),
    ...(agent ? { agent } : {}),
    ...(tenant ? { tenant } : {}),
    ...(DAY.test(filters.since) ? { since: `${filters.since}T00:00:00Z` } : {}),
    ...(DAY.test(filters.until) ? { until: `${filters.until}T23:59:59Z` } : {}),
  };
}

export const isAuditFiltered = (filters: AuditFilters): boolean => Object.keys(toAuditQuery(filters)).length > 0;

/** `local`, or a cloud token's id (`cloud:<id>`), as the audit records it. */
export function auditActor(actor: string): { kind: "local" } | { kind: "cloud"; short: string } | { kind: "other"; label: string } {
  if (actor === "local") return { kind: "local" };
  if (actor.startsWith("cloud:")) return { kind: "cloud", short: actor.slice("cloud:".length, "cloud:".length + 8) };
  return { kind: "other", label: actor };
}

/** The paths of a Share's scope; an empty or unreadable scope is the whole Drive. */
export function auditScope(scope: unknown): string[] {
  return Array.isArray(scope) ? scope.filter((item): item is string => typeof item === "string" && item !== "") : [];
}

/** Newest first; the engine's sequence is the order of the chain. */
export const sortAudit = (items: readonly AuditShareRecord[]): AuditShareRecord[] => [...items].sort((left, right) => right.seq - left.seq);
