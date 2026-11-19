/**
 * Operation ids become the engine's Idempotency-Key (20-frontend §1.4.2) and
 * must match the main process pattern `[A-Za-z0-9_-]{16,128}`. A retry after a
 * timeout reuses the same id, so callers keep the id with the pending action.
 */
export function createOperationId(prefix = "op"): string {
  const random = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID().replace(/-/gu, "")
    : Array.from({ length: 4 }, () => Math.random().toString(16).slice(2, 10).padEnd(8, "0")).join("");
  return `${prefix}_${random}`;
}
