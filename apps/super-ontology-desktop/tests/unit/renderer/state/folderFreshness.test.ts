import { describe, expect, it } from "vitest";
import { FRESHNESS_STATE_VALUES } from "@so-contract/docs-api.generated";
import { FRESHNESS_PRESENTATION, rollUpFolderFreshness } from "@so-model/superDriveStateModel";

describe("folder freshness roll-up (F-S3-08)", () => {
  it("has a presentation for every freshness state of the contract, plus unknown", () => {
    for (const state of FRESHNESS_STATE_VALUES) expect(FRESHNESS_PRESENTATION[state], state).toBeDefined();
    expect(FRESHNESS_PRESENTATION.unknown).toBeDefined();
  });

  it("marks nothing for an up-to-date Drive and a folder without reviews", () => {
    expect(rollUpFolderFreshness("up_to_date", { pending_review: 0 })).toBeNull();
    expect(rollUpFolderFreshness("up_to_date", undefined)).toBeNull();
    expect(rollUpFolderFreshness(null, {})).toBeNull();
  });

  it("a folder with reviews waiting shows waiting_review when the Drive is calm", () => {
    expect(rollUpFolderFreshness("up_to_date", { pending_review: 2 })).toBe("waiting_review");
    expect(rollUpFolderFreshness("waiting_review", { pending_review: 2 })).toBe("waiting_review");
    expect(rollUpFolderFreshness("waiting_review", { pending_review: 0 })).toBeNull();
  });

  it("takes the Drive's held, degraded, syncing, processing and publishing for every folder, by priority", () => {
    for (const state of ["held", "degraded", "syncing", "processing", "publishing"] as const) expect(rollUpFolderFreshness(state, { pending_review: 0 }), state).toBe(state);
    expect(rollUpFolderFreshness("held", { pending_review: 4 })).toBe("held");
    expect(rollUpFolderFreshness("degraded", { pending_review: 4 })).toBe("degraded");
    expect(rollUpFolderFreshness("processing", { pending_review: 4 })).toBe("processing");
    // Review has the lowest priority of the busy states, but is above up_to_date; publishing outranks it.
    expect(rollUpFolderFreshness("publishing", { pending_review: 4 })).toBe("publishing");
  });

  it("keeps an unknown Drive value visible as unknown", () => {
    expect(rollUpFolderFreshness("melting", { pending_review: 0 })).toBe("unknown");
  });
});
