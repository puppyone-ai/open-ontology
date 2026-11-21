// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { normalizeDropPaths, UPLOAD_PATHS_PER_REQUEST } from "@so-model/queue/dropPaths";
import { dragItemCount, dropAttributes, databaseDropAttributes, isFileDrag, isMyFiles, resolveDropTarget } from "../../../../src/renderer/features/drop/dropTarget";

const transfer = (types: string[], kinds: string[] = [], files = 0) => ({ types, items: kinds.map((kind) => ({ kind })), files: { length: files } }) as unknown as DataTransfer;

describe("normalising the paths of a drop", () => {
  it("keeps absolute paths in order and counts the rest as unreadable", () => {
    expect(normalizeDropPaths(["/a/one.md", "relative.md", "", "C:\\x\\y.md", "/b/two.md"])).toEqual({ paths: ["/a/one.md", "C:\\x\\y.md", "/b/two.md"], rejected: 2 });
  });
  it("counts the same path twice once, with or without a trailing slash", () => {
    expect(normalizeDropPaths(["/a/P", "/a/P/", "/a/P"]).paths).toEqual(["/a/P"]);
  });
  it("drops a path inside another dropped path, but not a sibling that only shares a prefix", () => {
    expect(normalizeDropPaths(["/a/P", "/a/P/sub/x.md", "/a/P2/y.md", "/a/P/z.md"]).paths).toEqual(["/a/P", "/a/P2/y.md"]);
    expect(normalizeDropPaths(["/a/P/sub/x.md", "/a/P"]).paths).toEqual(["/a/P"]);
  });
  it("keeps a root path as it is", () => {
    expect(normalizeDropPaths(["/"]).paths).toEqual(["/"]);
  });
  it("matches the request limit of the bridge", () => {
    expect(UPLOAD_PATHS_PER_REQUEST).toBe(1000);
  });
});

describe("what a drag carries", () => {
  it("recognises a file drag and ignores text, links and in-page drags", () => {
    expect(isFileDrag(transfer(["Files"]))).toBe(true);
    expect(isFileDrag(transfer(["text/plain", "Files"]))).toBe(true);
    expect(isFileDrag(transfer(["text/plain"]))).toBe(false);
    expect(isFileDrag(transfer(["text/uri-list"]))).toBe(false);
    expect(isFileDrag(null)).toBe(false);
  });
  it("counts items from the drag, at least one for a file drag", () => {
    expect(dragItemCount(transfer(["Files"], ["file", "file", "string"]))).toBe(2);
    expect(dragItemCount(transfer(["Files"], [], 3))).toBe(3);
    expect(dragItemCount(transfer(["Files"]))).toBe(1);
    expect(dragItemCount(transfer(["text/plain"]))).toBe(0);
  });
});

function row(attributes: Record<string, string>, inner = "<span class='x'>text</span>") {
  const element = document.createElement("div");
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value);
  element.innerHTML = inner;
  document.body.append(element);
  return element;
}

describe("where a drop goes", () => {
  const my = { id: "d-my", name: "My files" };
  it("defaults to My files, and to the Drive that is not listed yet", () => {
    expect(resolveDropTarget(document.body, my)).toEqual({ target: { kind: "drive", driveId: "d-my", folder: "", name: "My files", managed: true }, element: null });
    expect(resolveDropTarget(null, null).target).toMatchObject({ kind: "drive", driveId: null, managed: true });
  });
  it("goes into the source under the pointer, with its folder", () => {
    const element = row(dropAttributes({ id: "d-team", name: "Team docs", backend: "fs" }, "notes/2026"));
    const resolved = resolveDropTarget(element.querySelector(".x"), my);
    expect(resolved.target).toEqual({ kind: "drive", driveId: "d-team", folder: "notes/2026", name: "Team docs", managed: false });
    expect(resolved.element).toBe(element);
  });
  it("treats My files as managed even when addressed as a source", () => {
    const element = row(dropAttributes({ id: "d-my", name: "My files", backend: "fs" }));
    expect(resolveDropTarget(element, my).target).toMatchObject({ managed: true, folder: "" });
  });
  it("explains why cloud storage and databases do not take files", () => {
    expect(resolveDropTarget(row(dropAttributes({ id: "d-s3", name: "Archive", backend: "s3" })), my).target).toEqual({ kind: "rejected", name: "Archive", reason: "cloud" });
    expect(resolveDropTarget(row(databaseDropAttributes("shop")), my).target).toEqual({ kind: "rejected", name: "shop", reason: "database" });
  });
  it("the nearest owner wins", () => {
    const outer = row(dropAttributes({ id: "d-a", name: "A", backend: "fs" }), "<div class='inner'></div>");
    const inner = outer.querySelector(".inner") as HTMLElement;
    for (const [key, value] of Object.entries(databaseDropAttributes("inner-db"))) inner.setAttribute(key, value);
    expect(resolveDropTarget(inner, my).target).toMatchObject({ kind: "rejected", name: "inner-db" });
  });
  it("knows My files by name and backend only", () => {
    expect(isMyFiles({ name: "My files", backend: "fs" })).toBe(true);
    expect(isMyFiles({ name: "My files", backend: "s3" })).toBe(false);
    expect(isMyFiles({ name: "Other", backend: "fs" })).toBe(false);
  });
});
