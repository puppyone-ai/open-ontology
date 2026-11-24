import { catalog } from "../../../support/locales";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { FILE_STATE_VALUES } from "@so-contract/docs-api.generated";
import {
  BLOCKED_REASONS, FILE_STATE_GROUPS, FILE_STATE_PRESENTATION, presentedFileState,
} from "@so-model/superDriveStateModel";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];


describe("file state model (20-frontend §4.1)", () => {
  it("presents every contract FileState plus uploading and unknown", () => {
    for (const state of FILE_STATE_VALUES) expect(FILE_STATE_PRESENTATION[state], state).toBeDefined();
    expect(Object.keys(FILE_STATE_PRESENTATION).sort()).toEqual([...FILE_STATE_VALUES, "unknown", "uploading"].sort());
  });

  it("has a message in every locale for every state and blocked reason", () => {
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const presentation of Object.values(FILE_STATE_PRESENTATION)) {
        expect(messages[presentation.messageId.replace(/^super-drive\./u, "")], `${locale}:${presentation.messageId}`).toBeTruthy();
      }
      for (const reason of BLOCKED_REASONS) expect(messages[`file.state.blocked.${reason}`], `${locale}:${reason}`).toBeTruthy();
      for (const unit of ["page", "sheet", "line", "batch"]) expect(messages[`file.progress.${unit}`], `${locale}:${unit}`).toBeTruthy();
    }
  });

  it("puts every contract state into exactly one filter group, matching its presentation", () => {
    const seen = new Map<string, string>();
    for (const [group, states] of Object.entries(FILE_STATE_GROUPS)) {
      for (const state of states) {
        expect(seen.has(state), `${state} in two groups`).toBe(false);
        seen.set(state, group);
        expect(FILE_STATE_PRESENTATION[state].group, state).toBe(group);
      }
    }
    expect([...seen.keys()].sort()).toEqual([...FILE_STATE_VALUES].sort());
  });

  it("keeps an unknown value readable as unknown instead of throwing", () => {
    expect(presentedFileState("published")).toBe("published");
    for (const odd of ["melting", "", undefined, null, "Published"]) expect(presentedFileState(odd as string)).toBe("unknown");
    expect(FILE_STATE_PRESENTATION.unknown.group).toBeNull();
  });

  it("animates only the states that are working right now", () => {
    expect(Object.entries(FILE_STATE_PRESENTATION).filter(([, presentation]) => presentation.busy).map(([state]) => state).sort()).toEqual(["extracting", "parsing"]);
  });
});
