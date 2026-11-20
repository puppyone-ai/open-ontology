import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const read = (relative) => fs.readFileSync(path.resolve(import.meta.dirname, "..", "..", relative), "utf8");

// P07 reads the freshness and the change rings off the page; these attributes are its contract with the renderer.
describe("the page exposes what P07 measures", () => {
  it("the source list and the source page carry the freshness state", () => {
    expect(read("src/renderer/screens/Sources.tsx")).toMatch(/data-testid="source-row"[^>]*data-freshness=\{fresh\}/u);
    expect(read("src/renderer/screens/sources/SourceDetail.tsx")).toMatch(/data-testid="source-freshness" data-freshness=\{fresh\}/u);
  });
  it("the atlas counts the change rings it plays", () => {
    const atlas = read("src/renderer/features/atlas/Atlas.tsx");
    expect(atlas).toMatch(/data-testid="atlas" data-rings=\{rings\}/u);
    expect(atlas).toMatch(/op\.type === "pulse"/u);
  });
  it("P07 reads exactly those hooks", () => {
    const harness = read("tests/e2e/harness/autosync.mjs");
    for (const hook of ["source-freshness", "data-freshness", "data-rings", "atlas-version"]) expect(harness).toContain(hook);
  });
});
