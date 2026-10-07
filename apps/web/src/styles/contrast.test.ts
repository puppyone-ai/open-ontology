import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import tokens from "../../design/tokens.json" with { type: "json" };
import { contrastPairs, contrastRatio, token } from "./contrast";

const approvedPrimitive = {
  "neutral-canvas": { light: "#F7F8FA", dark: "#101318" },
  "neutral-surface": { light: "#FFFFFF", dark: "#171C24" },
  "neutral-subtle": { light: "#F0F2F5", dark: "#202733" },
  "neutral-text": { light: "#202733", dark: "#EDF0F5" },
  "neutral-muted": { light: "#566170", dark: "#A8B2C2" },
  "neutral-border": { light: "#DDE2EA", dark: "#364151" },
  "blue-action": { light: "#245BD6", dark: "#8EB0FF" },
  "on-action": { light: "#FFFFFF", dark: "#101318" },
  "success-text": { light: "#176B47", dark: "#80D8AD" },
  "warning-text": { light: "#8A570A", dark: "#F0C36C" },
  "danger-text": { light: "#B42332", dark: "#FF9EA7" },
  "success-bg": { light: "#E7F6EE", dark: "#123528" },
  "warning-bg": { light: "#FBF1DC", dark: "#3A2C12" },
  "danger-bg": { light: "#FDE8EA", dark: "#3A1C22" },
  "accent-bg": { light: "#E7EEFB", dark: "#1A2744" },
  "input-border": { light: "#6E7A8C", dark: "#61758F" },
} as const;

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
    expect(css).toContain("--oo-surface-selected: var(--oo-accent-bg)");
    expect(css).toContain("--oo-button-primary-bg: var(--oo-accent)");
    expect(css).toContain("--oo-button-primary-fg: var(--oo-on-accent)");
    expect(css).toContain("--oo-input-error: var(--oo-danger)");
  });

  it("keeps approved primitive colors and points new names at measured pairs", () => {
    expect(tokens.primitive).toEqual(approvedPrimitive);
    expect(JSON.stringify(tokens.semantic) + JSON.stringify(tokens.component)).not.toContain("#");
    expect(tokens.semantic["surface-selected"].ref).toBe("accent-bg");
    expect(tokens.semantic["surface-selected"].measuredAs).toBe("text/accent-bg");
    expect(tokens.component["input-error"].ref).toBe("danger-text");
    expect(tokens.component["input-error"].measuredAs).toBe("danger/surface");
    expect(tokens.component["button-primary-bg"].ref).toBe("blue-action");
    expect(tokens.component["button-primary-fg"].ref).toBe("on-action");
    expect(tokens.component["input-border"].ref).toBe("input-border");
    for (const mode of ["light", "dark"] as const) {
      const selected = contrastRatio(token("neutral-text", mode), token("accent-bg", mode));
      const inputError = contrastRatio(token("danger-text", mode), token("neutral-surface", mode));
      expect(selected, `surface-selected ${mode}`).toBeGreaterThanOrEqual(4.5);
      expect(inputError, `input-error ${mode}`).toBeGreaterThanOrEqual(4.5);
    }
  });
});
