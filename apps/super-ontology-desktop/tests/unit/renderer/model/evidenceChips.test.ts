import { describe, expect, it } from "vitest";
import { collapseAnchor, groupEvidence } from "@so-model/evidenceChips";
import type { Evidence } from "@so-shared/docs-api.generated";

const ev = (id: string, file_id: string, path: string, src: string): Evidence => ({ id, file_id, path, src } as Evidence);

describe("evidence chips", () => {
  it("collapses a single-line range and keeps real ranges", () => {
    expect(collapseAnchor("L6–L6")).toBe("L6");
    expect(collapseAnchor("L6-L6")).toBe("L6");
    expect(collapseAnchor("L6–L9")).toBe("L6–L9");
    expect(collapseAnchor("p3")).toBe("p3");
  });
  it("groups the anchors of one file into one chip without repeats", () => {
    const groups = groupEvidence([ev("1", "f1", "notes/a.md", "[src:a#L6-L6]"), ev("2", "f2", "b.md", "[src:b]"), ev("3", "f1", "notes/a.md", "[src:a#L9–L12]"), ev("4", "f1", "notes/a.md", "[src:a#L6–L6]")]);
    expect(groups).toEqual([
      { fileId: "f1", path: "notes/a.md", name: "a.md", anchors: ["L6", "L9–L12"] },
      { fileId: "f2", path: "b.md", name: "b.md", anchors: [] },
    ]);
  });
});
