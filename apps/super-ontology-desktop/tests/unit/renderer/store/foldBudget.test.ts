import { describe, expect, it } from "vitest";
import { applyEvents } from "@so-model/superDriveEvents";
import { folderKey } from "@so-model/superDriveRoute";
import { createEmptyFolderPage, createInitialState } from "@so-model/superDriveState";
import type { EngineEvent } from "@so-shared/bridge";

const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
const TOTAL = 10_000;
const BUDGET_MS = 50;
const file = (index: number, state: string) => ({
  id: `f-${index}`, drive_id: DRIVE_ID, path: `folder/file-${String(index).padStart(5, "0")}.md`, name: `file-${index}.md`, state,
  state_since: "2026-11-17T00:00:00Z", retryable: false, pending_review: 0,
});

describe("event folding budget (F-S4-02: 10,000 events in under 50 ms)", () => {
  it("folds 10,000 state changes into a loaded 10,000-file folder inside the budget (best of five)", () => {
    const state = createInitialState();
    const files: Record<string, never> = {};
    const ids: string[] = [];
    for (let index = 0; index < TOTAL; index += 1) { files[`f-${index}`] = { ...file(index, "queued"), seq: 1 } as never; ids.push(`f-${index}`); }
    const loaded = { ...state, files, folders: { [folderKey(DRIVE_ID, "folder")]: { ...createEmptyFolderPage(), ids, asOfSeq: 1, total: TOTAL } } };
    const events: EngineEvent[] = Array.from({ length: TOTAL }, (_, index) => ({ seq: 10 + index, type: "file.state_changed", at: "2026-11-17T00:00:00Z", drive_id: DRIVE_ID, data: { file: file(index, "published") } }));
    const times: number[] = [];
    for (let run = 0; run < 5; run += 1) {
      const started = performance.now();
      const next = applyEvents(loaded, events);
      times.push(performance.now() - started);
      expect(Object.keys(next.files)).toHaveLength(TOTAL);
    }
    expect(Math.min(...times)).toBeLessThan(BUDGET_MS);
  });
});
