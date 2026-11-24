import type { Suggestion, SuggestionDigest, SuggestionDiff } from "@so-contract/docs-api.generated";

export const SHARE_A = "00000000-0000-4000-8000-0000000000a1";
export const SHARE_B = "00000000-0000-4000-8000-0000000000a2";

export function makeSuggestion(index: number, overrides: Partial<Suggestion> = {}): Suggestion {
  return {
    id: `00000000-0000-4000-8000-${String(index).padStart(12, "0")}`, share_id: SHARE_A, drive_id: "00000000-0000-4000-8000-000000000001", kind: "supersede", status: "pending", rule: "supersede_in_use",
    revision: 1, submitted_by: "collector", created_at: `2026-11-18T10:${String(index % 60).padStart(2, "0")}:00Z`,
    payload: { protocol: "oo.suggest/v1", kind: "supersede", subject: `Plan ${index}`, predicate: "decision", object: `Option ${index}`, rationale: "cost", evidence: [{ kind: "brief_line", ref: "[src:Projects/a.md#L3-L3]" }] },
    impact: { shares: [{ share_id: SHARE_B, task: "Follow plan two", sections: ["Decisions"], version: 2 }] },
    ...overrides,
  } as Suggestion;
}

export function makeDigest(pending: Suggestion[], inEffect: Suggestion[] = []): SuggestionDigest {
  return { day: "2026-11-18", counts: { pending: pending.length, auto_accepted: inEffect.length }, total: pending.length + inEffect.length, in_effect: inEffect, pending } as SuggestionDigest;
}

export function makeDiff(suggestion: Suggestion, overrides: Partial<SuggestionDiff> = {}): SuggestionDiff {
  return {
    suggestion_id: suggestion.id, share_id: suggestion.share_id, kind: suggestion.kind, status: suggestion.status, rule: suggestion.rule, rationale: "cost", role: "editor", writable: true, writer: "desktop",
    changes: [{ file_id: "f1", path: "Projects/a.md", anchor: { kind: "text", line_start: 3, line_end: 3 }, frag: "#L3-L3", block_id: "b1", source_b3: "b3:x", operation: "replace", after: "Adopt plan two (2026-11)", before_ref: "[src:Projects/a.md#L3-L3]" }],
    ontology_effect: {}, impact: suggestion.impact, ...overrides,
  } as SuggestionDiff;
}
