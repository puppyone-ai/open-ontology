import type { RefreshPolicy, ShareRole } from "../contract/docsApi.generated";
import { shellQuote } from "../format";

/** Values the delivery step turns into the equivalent `oo context mount` line (10 section 7.9). */
export type MountCommandInput = {
  /** Name of the Drive, or its id when the name has spaces or non-ASCII characters (`--drive` takes either). */
  drive: { id: string; name: string };
  scope: readonly string[];
  /** The mount target as the main process displays it; `null` until a folder was picked. */
  to: string | null;
  task?: string;
  budget: number;
  role: ShareRole;
  expiresInSeconds: number;
  version?: number;
  requireComplete?: boolean;
  group?: string;
  refreshPolicy?: RefreshPolicy;
};

const PLAIN = /^[A-Za-z0-9_.,:@%+=/~-]+$/u;
const quote = (value: string): string => (PLAIN.test(value) ? value : shellQuote(value));
/** A Drive name is used when it is shell-plain ASCII; otherwise its id is (names with spaces or non-ASCII letters stay out of the line). */
const driveArgument = (drive: { id: string; name: string }): string => (/^[A-Za-z0-9_.-]+$/u.test(drive.name) ? drive.name : drive.id);

/** `7d`, `24h`, `90m`: the largest whole unit; the CLI accepts the same forms. */
export function formatExpiry(seconds: number): string {
  if (seconds % 86_400 === 0) return `${seconds / 86_400}d`;
  if (seconds % 3_600 === 0) return `${seconds / 3_600}h`;
  return `${Math.max(1, Math.round(seconds / 60))}m`;
}

export function buildMountCommand(input: MountCommandInput): string {
  const parts = ["oo", "context", "mount", "--drive", quote(driveArgument(input.drive))];
  for (const path of input.scope) parts.push("--scope", quote(path));
  parts.push("--to", quote(input.to ?? "<folder>"));
  const task = input.task?.trim();
  if (task) parts.push("--for", shellQuote(task));
  parts.push("--budget", String(input.budget), "--role", input.role, "--expires", formatExpiry(input.expiresInSeconds));
  if (input.version !== undefined) parts.push("--version", String(input.version));
  if (input.group) parts.push("--group", quote(input.group));
  if (input.refreshPolicy && input.refreshPolicy !== "manual") parts.push("--refresh-policy", input.refreshPolicy);
  if (input.requireComplete) parts.push("--require-complete");
  return parts.join(" ");
}

/** The engine's own command wins when it differs from ours; the caller logs the mismatch (20 section 3.6.1). */
export function reconcileCommand(local: string, fromEngine: string | undefined): { command: string; mismatch: boolean } {
  if (!fromEngine) return { command: local, mismatch: false };
  return { command: fromEngine, mismatch: fromEngine.trim() !== local.trim() };
}
