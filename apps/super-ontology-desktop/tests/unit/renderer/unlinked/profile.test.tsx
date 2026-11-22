// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { profileOf, FILE_PROFILES, useProfileText } from "../../../../src/renderer/model/profile";
import { isIndexedOnly, useUnlinkedText, visibleReport, type UnlinkedReport } from "../../../../src/renderer/model/unlinked";
import { catalog } from "../../../support/locales";

const wrap = ({ children }: { children: React.ReactNode }) => <TestLocalizationProvider messages={mergeCatalogNamespaces({ app: catalog("en", "app") })}>{children}</TestLocalizationProvider>;
const file = (id: string, reason: UnlinkedReport["files"][number]["reason"]) => ({ fileId: id, driveId: "d", path: `a/${id}.md`, name: `${id}.md`, reason });
const report: UnlinkedReport = { files: [file("a", { kind: "indexed_only" }), file("b", { kind: "unsupported", format: "psd" })], entities: [], totalFiles: 2, checkedFiles: 2, read: [], limited: false, complete: true };

describe("document profile", () => {
  it("maps known values and reads anything else as other", () => {
    for (const p of FILE_PROFILES) expect(profileOf({ profile: p })).toBe(p);
    expect(profileOf({})).toBe("other");
    expect(profileOf({ profile: "hologram" })).toBe("other");
    expect(profileOf({ profile: 7 })).toBe("other");
  });
  it("names every profile, and an unknown one as Other", () => {
    const { result } = renderHook(() => useProfileText(), { wrapper: wrap });
    expect(result.current({ profile: "paper" })).toBe("Research paper");
    expect(result.current({ profile: "hologram" })).toBe("Other");
  });
});

describe("indexed-only files", () => {
  it("are left out of Not connected by default and listed with the filter", () => {
    expect(visibleReport(report, false)!.files.map((f) => f.fileId)).toEqual(["b"]);
    expect(visibleReport(report, true)!.files.map((f) => f.fileId)).toEqual(["a", "b"]);
    expect(visibleReport(null, false)).toBeNull();
    expect(isIndexedOnly({ kind: "group", of: "indexed_only", count: 9 })).toBe(true);
    expect(isIndexedOnly({ kind: "processing" })).toBe(false);
  });
  it("read as a neutral state, not an error", () => {
    const { result } = renderHook(() => useUnlinkedText(), { wrapper: wrap });
    expect(result.current({ kind: "indexed_only" })).toBe("Indexed, searchable");
    expect(result.current({ kind: "group", of: "indexed_only", count: 9 })).not.toMatch(/no entities|error|fail/i);
  });
});
