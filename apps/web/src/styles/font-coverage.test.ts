import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import manifest from "../../design/font-manifest.json" with { type: "json" };
import sources from "../../design/subset-sources.json" with { type: "json" };
import { foundationCopy as copy } from "./copy";

const preservedSha256: Record<string, string> = {
  "public/fonts/NotoSansSC-full.woff2": "bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b",
  "public/fonts/Geist-wght.woff2": "a369fcf5628ea2aa4e1b9e2ec6a5b3624e365bda588e1f0f2f12b564f728fbb8",
  "public/fonts/GeistMono-Regular.woff2": "e4507fb4fb5f832fbbb6c06aea4206274ba3083007f23fa8cbc0e87a10acf95b",
  "public/fonts/licenses/Geist-OFL.txt": "c683bfbcc7e087f5d37a54ef628f10387c451a83ddc459b151403a164ac46c90",
  "public/fonts/licenses/NotoSans-LICENSE": "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
};

const routePoints = [0x83dc, 0x5355, 0x8d26, 0x6237, 0x53d1, 0x9001, 0x95ee, 0x9898];

function walkText(entry: string, chunks: string[]) {
  const info = statSync(entry);
  if (info.isDirectory()) {
    for (const name of readdirSync(entry)) walkText(join(entry, name), chunks);
    return;
  }
  if (/\.(tsx?|css|json)$/.test(entry)) chunks.push(readFileSync(entry, "utf8"));
}

function coveredPoints(): number[] {
  const found = new Set<number>(sources.gapCodepoints);
  const chunks: string[] = [];
  for (const relative of sources.roots) walkText(join(process.cwd(), relative), chunks);
  for (const chunk of chunks) {
    for (const char of chunk) {
      const point = char.codePointAt(0) ?? 0;
      if (point > 127 && point !== 0x9f98) found.add(point);
    }
  }
  return [...found].filter((point) => point > 127).sort((left, right) => left - right);
}

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

  it("covers the reviewed gap and the registered source text", () => {
    expect(sources.g2Snapshot).toBe("0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8");
    expect(manifest.sourceBinding.g2Snapshot).toBe(sources.g2Snapshot);
    expect(manifest.sourceBinding.roots).toEqual(sources.roots);
    expect(sources.roots).toContain("src/features/admin-overview");
    expect(sources.roots).toContain("src/features/sources");
    expect(sources.roots).toContain("src/features/operations");
    expect(sources.rareCodepoint).toBe(0x9f98);
    expect(sources.gapCodepoints).toHaveLength(98);
    expect(sources.gapCodepoints).not.toContain(0x9f98);
    expect(manifest.rareGlyph).toBe("龘");
    expect(manifest.subsetCodepoints).not.toContain(0x9f98);
    expect(manifest.subsetCodepoints).toEqual(coveredPoints());
    for (const point of [...sources.gapCodepoints, ...routePoints]) {
      expect(manifest.subsetCodepoints).toContain(point);
    }
  });

  it("keeps font faces local with swap and a full fallback family", () => {
    const fontsCss = readFileSync(join(process.cwd(), "src/styles/fonts.css"), "utf8");
    expect(fontsCss).not.toMatch(/https?:/i);
    expect(fontsCss).toContain("font-display: swap");
    expect(fontsCss).toContain('font-family: "Noto Sans SC Full"');
    expect(fontsCss).toContain("unicode-range:");
    expect(fontsCss).not.toContain("U+9F98");
    expect(fontsCss).toContain(manifest.unicodeRange);
    expect(manifest.files.map((file) => file.file)).toEqual([
      "public/fonts/NotoSansSC-300-subset.woff2",
      "public/fonts/NotoSansSC-400-subset.woff2",
      "public/fonts/NotoSansSC-500-subset.woff2",
      "public/fonts/NotoSansSC-full.woff2",
      "public/fonts/Geist-wght.woff2",
      "public/fonts/GeistMono-Regular.woff2",
    ]);
    for (const [file, sha256] of Object.entries(preservedSha256)) {
      const bytes = readFileSync(join(process.cwd(), file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(sha256);
      if (file.endsWith("NotoSansSC-full.woff2")) expect(bytes.length).toBe(7762252);
    }
  });
});
