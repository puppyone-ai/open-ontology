/**
 * Presentation model of agent suggestions (docs-api 1.3, 20-frontend §3.5.4): which ones are already in effect, which wait for a
 * person, how they group for the day's digest, and the line of the file a suggestion would change. Pure functions, no UI.
 */
import type { Suggestion, SuggestionDigest, SuggestionDiff } from "../../../shared/super-ontology/docs-api.generated";

export const SUGGESTION_KIND_ORDER = ["new_fact", "supersede", "correction", "preference", "type_change"] as const;

/** Statuses that are already applied to the ontology and can be taken back (the engine's `undo` rule). */
export const UNDOABLE_STATUSES: readonly string[] = ["auto_accepted", "superseded_silently", "accepted"];

export const kindMessageId = (kind: string) => (SUGGESTION_KIND_ORDER as readonly string[]).includes(kind) ? `super-drive.suggest.kind.${kind}` : "super-drive.suggest.kind.unknown";
export const statusMessageId = (status: string) => ["pending", "auto_accepted", "superseded_silently", "accepted", "rejected", "undone", "duplicate"].includes(status) ? `super-drive.suggest.status.${status}` : "super-drive.suggest.status.unknown";
export const ruleMessageId = (rule: string) => ["new_fact", "supersede_unused", "supersede_in_use", "manual"].includes(rule) ? `super-drive.suggest.rule.${rule}` : "super-drive.suggest.rule.unknown";

export const canUndo = (suggestion: Pick<Suggestion, "status">): boolean => UNDOABLE_STATUSES.includes(suggestion.status);
export const isPending = (suggestion: Pick<Suggestion, "status">): boolean => suggestion.status === "pending";

/** The wording of a suggested fact as one line: subject, predicate, the value or object, and when it starts. Same line the engine puts in the diff. */
export function suggestionLine(suggestion: Pick<Suggestion, "payload">): string {
  const payload = suggestion.payload;
  const right = payload.object ?? payload.value ?? "";
  const from = payload.valid_from ? ` (${payload.valid_from})` : "";
  return `${payload.subject} ${payload.predicate} ${right}${from}`.replaceAll(/\s+/gu, " ").trim();
}

export type SuggestionGroup = { shareId: string; task: string | null; items: Suggestion[] };

/** Groups by the Share (the source) they came from, in the order the shares first appear; the task comes from the impact data when it has it. */
export function groupBySource(items: readonly Suggestion[]): SuggestionGroup[] {
  const groups = new Map<string, SuggestionGroup>();
  for (const item of items) {
    const task = item.impact?.shares?.find((share) => share.share_id === item.share_id)?.task ?? null;
    const group = groups.get(item.share_id) ?? { shareId: item.share_id, task, items: [] };
    group.task ??= task;
    group.items.push(item);
    groups.set(item.share_id, group);
  }
  return [...groups.values()];
}

export type DigestView = {
  day: string;
  total: number;
  /** Already applied: auto accepted and silently superseded, newest first, each with an undo. */
  inEffect: Suggestion[];
  /** Waiting for a person, grouped by source: one decision per group accepts them all. */
  pending: SuggestionGroup[];
  pendingCount: number;
  /** What the day needed from a person: one for every group of pending items plus none for the ones already in effect (§ L5 acceptance 3). */
  humanOperations: number;
  counts: { status: string; count: number }[];
};

const byNewest = (left: Suggestion, right: Suggestion) => (right.created_at ?? "").localeCompare(left.created_at ?? "");

/** The day's digest as the view shows it. */
export function digestView(digest: SuggestionDigest): DigestView {
  const pending = groupBySource([...digest.pending].sort(byNewest));
  return {
    day: digest.day, total: digest.total, inEffect: [...digest.in_effect].sort(byNewest), pending, pendingCount: digest.pending.length,
    humanOperations: pending.length,
    counts: Object.entries(digest.counts ?? {}).map(([status, count]) => ({ status, count: Number(count) })).filter((entry) => entry.count > 0).sort((a, b) => a.status.localeCompare(b.status)),
  };
}

/** The Shares an accepted suggestion would mark as needing a refresh (`impact.shares`), with their task and the sections it touches. */
export function impactedShares(suggestion: Pick<Suggestion, "impact" | "share_id">, diff?: Pick<SuggestionDiff, "impact"> | null) {
  const fromDiff = (diff?.impact as { shares?: NonNullable<Suggestion["impact"]>["shares"] } | undefined)?.shares;
  return (fromDiff ?? suggestion.impact?.shares ?? []).filter((share) => typeof share.share_id === "string").map((share) => ({
    shareId: share.share_id as string, task: share.task ?? null, groupId: share.group_id ?? null, sections: share.sections ?? [], version: share.version ?? null,
  }));
}
