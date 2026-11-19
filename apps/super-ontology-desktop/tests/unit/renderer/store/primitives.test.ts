import { describe, expect, it } from "vitest";
import { LruMap } from "@so-model/lruMap";
import { createOperationId } from "@so-model/operationIds";
import {
  baseName, driveForRoute, INITIAL_ROUTE, joinPath, navigateToFolder, parentPath, pathSegments, selectDriveRoute,
} from "@so-model/superDriveRoute";
import { formatBytes, formatRelativeTime, shellQuote } from "@so-model/format";
import { OPERATION_ID_PATTERN } from "../../../support/operationIdPattern";
import { makeDrive } from "../../../support/fakeApi";

describe("LruMap", () => {
  it("evicts the least recently used entry and refreshes on read", () => {
    const map = new LruMap<string, number>(2);
    map.set("a", 1).set("b", 2);
    expect(map.get("a")).toBe(1);
    map.set("c", 3);
    expect(map.keys()).toEqual(["a", "c"]);
    expect(map.has("b")).toBe(false);
    map.set("a", 9);
    expect(map.keys()).toEqual(["c", "a"]);
    expect(map.delete("c")).toBe(true);
    map.clear();
    expect(map.size).toBe(0);
    expect(() => new LruMap(0)).toThrow(RangeError);
  });
});

describe("operation ids", () => {
  it("match the pattern the main process accepts and differ per call", () => {
    const ids = new Set(Array.from({ length: 50 }, () => createOperationId()));
    expect(ids.size).toBe(50);
    for (const id of ids) expect(id).toMatch(OPERATION_ID_PATTERN);
    expect(createOperationId("sync")).toMatch(/^sync_[0-9a-f]{32}$/u);
  });
});

describe("route helpers", () => {
  it("splits, joins and walks paths", () => {
    expect(parentPath("a/b/c.md")).toBe("a/b");
    expect(parentPath("c.md")).toBe("");
    expect(baseName("a/b/c.md")).toBe("c.md");
    expect(joinPath("", "x")).toBe("x");
    expect(joinPath("a", "x")).toBe("a/x");
    expect(pathSegments("")).toEqual([]);
    expect(pathSegments("a/b")).toEqual([{ name: "a", path: "a" }, { name: "b", path: "a/b" }]);
  });

  it("navigating clears the open file and choosing a Drive resets the path", () => {
    const route = { ...INITIAL_ROUTE, driveId: "d", path: "a", detailsFileId: "f", section: "settings" as const };
    expect(navigateToFolder(route, "a/b")).toMatchObject({ section: "files", path: "a/b", detailsFileId: null });
    expect(selectDriveRoute(route, "e")).toMatchObject({ driveId: "e", path: "", detailsFileId: null });
    const drives = { d: makeDrive({ id: "d" }) };
    expect(driveForRoute(route, drives, ["d"])?.id).toBe("d");
    expect(driveForRoute({ ...route, driveId: "gone" }, drives, ["d"])?.id).toBe("d");
    expect(driveForRoute(route, {}, [])).toBeNull();
  });
});

describe("format", () => {
  const context = {
    t: (id: string, values?: Record<string, unknown>) => `${id}:${values?.value ?? ""}`,
    formatNumber: (value: number, options?: Intl.NumberFormatOptions) => new Intl.NumberFormat("en", options).format(value),
    formatDate: (value: Date) => value.toISOString().slice(0, 10),
    formatRelativeTime: (value: number, unit: string) => `${value} ${unit}`,
  } as never;

  it("picks the unit and tolerates bad input", () => {
    expect(formatBytes(512, context)).toBe("super-drive.size.bytes:512");
    expect(formatBytes(2048, context)).toBe("super-drive.size.kilobytes:2");
    expect(formatBytes(5 * 1024 * 1024, context)).toBe("super-drive.size.megabytes:5");
    expect(formatBytes(3 * 1024 ** 3, context)).toBe("super-drive.size.gigabytes:3");
    for (const bad of [null, undefined, Number.NaN, -1]) expect(formatBytes(bad as number, context)).toBe("");
  });

  it("formats relative times by distance", () => {
    const now = Date.parse("2026-11-17T12:00:00Z");
    expect(formatRelativeTime("2026-11-17T11:59:40Z", context, now)).toBe("0 second");
    expect(formatRelativeTime("2026-11-17T11:50:00Z", context, now)).toBe("-10 minute");
    expect(formatRelativeTime("2026-11-17T09:00:00Z", context, now)).toBe("-3 hour");
    expect(formatRelativeTime("2026-11-15T12:00:00Z", context, now)).toBe("-2 day");
    expect(formatRelativeTime("2026-01-01T00:00:00Z", context, now)).toBe("2026-01-01");
    expect(formatRelativeTime("nonsense", context, now)).toBe("");
    expect(formatRelativeTime(null, context, now)).toBe("");
  });

  it("quotes shell arguments", () => {
    expect(shellQuote("a b")).toBe("'a b'");
    expect(shellQuote("it's")).toBe("'it'\\''s'");
  });
});
