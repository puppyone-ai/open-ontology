import { describe, expect, it } from "vitest";
import { shouldOfferCliData } from "@so-model/cliDataOffer";
import { selectDrives, selectMergedDrive } from "@so-model/superDriveSelectors";
import { createInitialState } from "@so-model/superDriveState";
import { makeDrive } from "../../../support/fakeApi";

const base = { mode: "app", decided: false, cliHasData: true, legacyHasData: true } as never;

describe("the one-time offer to use the CLI's data", () => {
  it("is shown only on the app's own state, undecided, with CLI data present, and not once dismissed", () => {
    expect(shouldOfferCliData(base, false)).toBe(true);
    expect(shouldOfferCliData(base, true)).toBe(false);
    expect(shouldOfferCliData({ ...base, mode: "cli" }, false)).toBe(false);
    expect(shouldOfferCliData({ ...base, decided: true }, false)).toBe(false);
    expect(shouldOfferCliData({ ...base, cliHasData: false }, false)).toBe(false);
    expect(shouldOfferCliData(null, false)).toBe(false);
  });
});

describe("one My files in the lists", () => {
  it("hides a merged earlier CLI folder from the sources and reports it as a note", () => {
    const state = createInitialState();
    const mine = makeDrive({ id: "a", name: "My files" }); const old = makeDrive({ id: "b", name: "My files (earlier CLI folder)", status: "paused" });
    state.drives = { ...state.drives, byId: { a: mine, b: old }, order: ["a", "b"], loaded: true };
    expect(selectDrives(state).map((d) => d.id)).toEqual(["a"]);
    expect(selectMergedDrive(state)?.id).toBe("b");
  });
});
