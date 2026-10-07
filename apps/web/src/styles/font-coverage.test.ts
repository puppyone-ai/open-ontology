import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import manifest from "../../design/font-manifest.json" with { type: "json" };
import { foundationCopy as copy } from "./copy";

describe("font coverage", () => {
  it("subsets every non-ascii copy character except the rare probe", () => {
    const rare = copy.rareGlyph.codePointAt(0);
    expect(rare).toBe(0x9f98);
    const covered = new Set(manifest.subsetCodepoints);
    const missing: string[] = [];
    for (const value of Object.values(copy)) {
      for (const char of value) {
        const point = char.codePointAt(0) ?? 0;
        if (point > 127 && point !== rare && !covered.has(point)) missing.push(char);
      }
    }
    expect(missing).toEqual([]);
    expect(covered.has(rare ?? -1)).toBe(false);
  });

  it("keeps font faces local with swap and a full fallback family", () => {
    const fontsCss = readFileSync(join(process.cwd(), "src/styles/fonts.css"), "utf8");
    expect(fontsCss).not.toMatch(/https?:/i);
    expect(fontsCss).toContain("font-display: swap");
    expect(fontsCss).toContain('font-family: "Noto Sans SC Full"');
    expect(fontsCss).toContain("unicode-range:");
    expect(manifest.files.map((file) => file.file)).toEqual([
      "public/fonts/NotoSansSC-300-subset.woff2",
      "public/fonts/NotoSansSC-400-subset.woff2",
      "public/fonts/NotoSansSC-500-subset.woff2",
      "public/fonts/NotoSansSC-full.woff2",
      "public/fonts/Geist-wght.woff2",
      "public/fonts/GeistMono-Regular.woff2",
    ]);
  });
});
