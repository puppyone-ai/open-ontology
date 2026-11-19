import type { CompileInput } from "../../../shared/super-ontology/bridge";
import type { ShareSettings } from "../superDriveOntologyTypes";

export const BUDGET_PRESETS = [400, 800, 1600, 3200] as const;
export const EXPIRY_PRESETS_DAYS = [1, 3, 7, 30] as const;
export const DEFAULT_SETTINGS: ShareSettings = {
  scope: [], task: "", budget: 800, role: "suggester", expiresInSeconds: 7 * 86_400, requireComplete: false, group: "", refreshPolicy: "manual",
};
/** The one local vector model version docs-api 1.5 knows; part of the pack parameters, so the same version replays byte for byte. */
export const VECTOR_MODEL = "hashgram/1" as const;
export const MIN_BUDGET = 200;
export const MAX_BUDGET = 20_000;
export const MAX_TASK_CHARS = 2000;
export const TICKET_PATTERN = /^[A-Za-z0-9._#:-]{1,128}$/u;

/** A DrivePath of the contract: relative, no empty, `.` or `..` segments. */
export function isValidScopePath(path: string): boolean {
  if (path === "" || path.startsWith("/") || path.length > 1024 || /[\u0000-\u001f\u007f]/u.test(path)) return false;
  return path.split("/").every((part) => part !== "" && part !== "." && part !== "..");
}

export const clampBudget = (value: number): number => Math.min(MAX_BUDGET, Math.max(MIN_BUDGET, Math.round(value)));

/** The settings as a compile request (the preview and the one-click mount send the same request, so they cannot differ). */
export function compileInput(driveId: string, settings: ShareSettings, extra: { waitMs?: number } = {}): CompileInput {
  return {
    driveId,
    ...(settings.scope.length > 0 ? { scope: settings.scope } : {}),
    ...(settings.task.trim() ? { task: settings.task.trim() } : {}),
    budget: settings.budget,
    role: settings.role,
    ...(settings.requireComplete ? { requireComplete: true } : {}),
    ...(settings.vector === true ? { vector: VECTOR_MODEL } : {}),
    ...(extra.waitMs === undefined ? {} : { waitMs: extra.waitMs }),
  };
}

export type SettingsProblem = "taskTooLong" | "budgetRange" | "ticketInvalid" | "scopeInvalid";
export function validateSettings(settings: ShareSettings): SettingsProblem[] {
  const problems: SettingsProblem[] = [];
  if (settings.task.length > MAX_TASK_CHARS) problems.push("taskTooLong");
  if (settings.budget < MIN_BUDGET || settings.budget > MAX_BUDGET) problems.push("budgetRange");
  if (settings.group !== "" && !TICKET_PATTERN.test(settings.group)) problems.push("ticketInvalid");
  if (settings.scope.some((path) => !isValidScopePath(path)) || settings.scope.length > 64) problems.push("scopeInvalid");
  return problems;
}
