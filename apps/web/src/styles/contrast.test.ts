import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { contrastPairs, contrastRatio, token } from "./contrast";

describe("contrast", () => {
  it("measures black on white as 21:1", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
  });

  it("keeps foundation pairs above their thresholds in both themes", () => {
    for (const pair of contrastPairs) {
      for (const mode of ["light", "dark"] as const) {
        const ratio = contrastRatio(token(pair.foreground, mode), token(pair.background, mode));
        expect(ratio, `${pair.name} ${mode}`).toBeGreaterThanOrEqual(pair.minimum);
      }
    }
  });

  it("puts the same primitive hex values in the token stylesheet", () => {
    const css = readFileSync(join(process.cwd(), "src/styles/tokens.css"), "utf8").toLowerCase();
    for (const mode of ["light", "dark"] as const) {
      expect(css).toContain(token("neutral-text", mode).toLowerCase());
      expect(css).toContain(token("neutral-canvas", mode).toLowerCase());
      expect(css).toContain(token("blue-action", mode).toLowerCase());
      expect(css).toContain(token("input-border", mode).toLowerCase());
    }
  });
});
