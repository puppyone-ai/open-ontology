import { act } from "react";
import type { Evidence, Fact, ReviewGroup, ReviewItem } from "@so-contract/docs-api.generated";
import { DRIVE_ID } from "./fakeApi";

export const FILE_ID = "00000000-0000-4000-8000-0000000000f1";
export const OTHER_FILE_ID = "00000000-0000-4000-8000-0000000000f2";

export const makeEvidence = (n: number, extra: Partial<Evidence> = {}): Evidence => ({
  id: `00000000-0000-4000-8000-00000000e${String(n).padStart(3, "0")}`, file_id: FILE_ID, path: "notes/a.md", block_id: `blk_${n}`, anchor: { kind: "text", line_start: n, line_end: n, char_start: 0, char_end: 4 },
  src: `src:notes/a.md#L${n}`, quote: `Quote ${n}`, origin: "rule", ...extra,
} as Evidence);

export const entityRef = (key: string, name: string, type = "Person") => ({ id: `id-${key}`, key, type, name });

export const makeFact = (n: number, extra: Record<string, unknown> = {}): Fact => ({
  id: `00000000-0000-4000-8000-0000000fa${String(n).padStart(3, "0")}`, fact_key: "f".repeat(64), subject: entityRef("person:wang", "Wang Xiaoming"), predicate: "owns",
  object: entityRef("project:p2", "Plan two", "Project"), valid_from: "2026-10-01", valid_precision: "month", confidence_bp: 8600, origin: "llm", status: "published", revision: 3, recorded_at: "2026-11-01T00:00:00Z",
  evidence: [makeEvidence(n)], evidence_total: 1, ...extra,
} as unknown as Fact);

export const makeReviewItem = (n: number, kind: ReviewItem["kind"] = "low_confidence_fact", extra: Record<string, unknown> = {}): ReviewItem => ({
  id: `item-${n}`, kind, status: "pending", reason_code: "low_conf", confidence_bp: 6200, drive_id: DRIVE_ID, source: { file_id: FILE_ID, path: "notes/a.md" },
  subject: kind === "low_confidence_fact" ? { fact: makeFact(n) } : {}, evidence: [makeEvidence(n)], created_at: "2026-11-17T00:00:00Z", revision: 1, ...extra,
} as unknown as ReviewItem);

export const makeGroup = (count: number, kind: ReviewItem["kind"] = "low_confidence_fact", fileId = FILE_ID, path = "notes/a.md"): ReviewGroup => ({ source: { file_id: fileId, path }, kind, count });

export const q = (host: HTMLElement, id: string) => host.querySelector<HTMLElement>(`[data-testid="${id}"]`);
export const qa = (host: HTMLElement, id: string) => [...host.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];

export const flush = async () => { await act(async () => { await new Promise((resolve) => { setTimeout(resolve, 0); }); }); };

export const press = (target: Element, key: string, init: KeyboardEventInit = {}) => act(() => { target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init })); });

/** Every interactive element has a non-empty accessible name (A1). */
export function expectAccessibleNames(root: HTMLElement) {
  const bad: string[] = [];
  for (const element of root.querySelectorAll<HTMLElement>('button, a[href], input:not([type="hidden"]), select, textarea, [role="button"], [role="tab"], [role="menuitem"], [role="checkbox"], [role="switch"], [role="option"], [role="radio"]')) {
    const labelled = element.getAttribute("aria-label") ?? "";
    const byId = (element.getAttribute("aria-labelledby") ?? "").split(" ").map((id) => document.getElementById(id)?.textContent ?? "").join("");
    const wrapped = element.closest("label")?.textContent ?? "";
    const own = element.tagName === "INPUT" || element.tagName === "SELECT" || element.tagName === "TEXTAREA" ? "" : element.textContent ?? "";
    if ((labelled + byId + wrapped + own).trim() === "") bad.push(element.outerHTML.slice(0, 120));
  }
  expect(bad).toEqual([]);
}
import { expect } from "vitest";
