import { catalog } from "../../../support/locales";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { FRESHNESS_STATE_VALUES } from "@so-contract/docs-api.generated";
import {
  FRESHNESS_PRESENTATION, FRESHNESS_PRIORITY, highestPriorityFreshness, presentedFreshnessState,
} from "@so-model/superDriveStateModel";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];


describe("freshness model (00 §5.5, D48)", () => {
  it("presents every contract state and unknown, with a message in every locale", () => {
    expect(Object.keys(FRESHNESS_PRESENTATION).sort()).toEqual([...FRESHNESS_STATE_VALUES, "unknown"].sort());
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const presentation of Object.values(FRESHNESS_PRESENTATION)) {
        expect(messages[presentation.messageId.replace(/^super-drive\./u, "")], `${locale}:${presentation.messageId}`).toBeTruthy();
      }
    }
  });

  it("orders priorities as held > degraded > syncing > processing > publishing > waiting_review > up_to_date", () => {
    expect([...FRESHNESS_PRIORITY]).toEqual(["held", "degraded", "syncing", "processing", "publishing", "waiting_review", "up_to_date"]);
    expect([...FRESHNESS_PRIORITY].sort()).toEqual([...FRESHNESS_STATE_VALUES].sort());
    expect(highestPriorityFreshness(["up_to_date", "processing", "waiting_review"])).toBe("processing");
    expect(highestPriorityFreshness(["up_to_date", "held", "degraded"])).toBe("held");
    expect(highestPriorityFreshness(["melting", undefined, null])).toBeNull();
    expect(highestPriorityFreshness([])).toBeNull();
  });

  it("keeps an unknown state readable instead of throwing, and animates only the working states", () => {
    expect(presentedFreshnessState("publishing")).toBe("publishing");
    for (const odd of ["melting", "", undefined, null]) expect(presentedFreshnessState(odd as string)).toBe("unknown");
    expect(Object.entries(FRESHNESS_PRESENTATION).filter(([, presentation]) => presentation.busy).map(([state]) => state).sort()).toEqual(["processing", "publishing", "syncing"]);
  });
});
