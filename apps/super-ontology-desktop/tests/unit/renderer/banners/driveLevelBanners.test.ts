import { describe, expect, it } from "vitest";
import { computeBanners, writeBlockOf, type BannerInput } from "@so-model/banners/bannerModel";
import type { Drive } from "@so-contract/docs-api.generated";
import { makeDrive, makeSnapshot } from "../../../support/fakeApi";

const input = (drive: Partial<Drive>, health: Record<string, unknown> = {}): BannerInput => ({
  engine: makeSnapshot({ health: { events_head: 0, ...health } as never }), connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 1 }, hasData: true,
  drive: makeDrive({ files_total: 10, counts: { published: 10 }, ...drive }),
});
const find = (value: BannerInput, kind: string) => computeBanners(value).find((banner) => banner.kind === kind);

describe("Drive-level rows of §4.6 (F-S4-04)", () => {
  it("held mass deletion carries the number of files and the generation, and cannot be closed", () => {
    const held = find(input({ last_sync: { status: "held", generation: 9, removed: 412 } }), "mass_delete_held");
    expect(held).toMatchObject({ messageId: "super-drive.banner.mass_delete_held_count", dismissible: false, actions: ["confirmMassDelete"], values: { generation: 9, count: 412 } });
    expect(find(input({ last_sync: { status: "held", generation: 9 } }), "mass_delete_held")?.messageId).toBe("super-drive.banner.mass_delete_held");
  });

  it("permission denied and a missing root name the folder", () => {
    for (const code of ["DOCS_SOURCE_PERMISSION_DENIED", "DOCS_SOURCE_ROOT_MISSING"]) {
      const banner = find(input({ status_code: code, config: { root: "/Users/example/Docs" } }), "permission_denied");
      expect(banner, code).toMatchObject({ tone: "danger", messageId: "super-drive.banner.permission_denied_path", values: { path: "/Users/example/Docs" }, actions: ["syncNow"] });
    }
    expect(find(input({ status_code: "DOCS_SOURCE_PERMISSION_DENIED", config: {} }), "permission_denied")?.messageId).toBe("super-drive.banner.permission_denied");
  });

  it("a failed sync offers sync now, and an incomplete listing says nothing was removed", () => {
    expect(find(input({ last_sync: { status: "failed", error_code: "DOCS_SOURCE_TIMEOUT" } }), "sync_failed")?.actions).toEqual(["syncNow"]);
    expect(find(input({ last_sync: { status: "complete", error_code: "DOCS_SOURCE_LIST_INCOMPLETE" } }), "list_incomplete")?.actions).toEqual(["syncNow"]);
  });

  it("low disk shows the space left; a full disk replaces it", () => {
    const low = find(input({}, { disk: { free_bytes: 200 * 1024 * 1024 } }), "disk_low");
    expect(low).toMatchObject({ messageId: "super-drive.banner.disk_low_free", values: { freeBytes: 200 * 1024 * 1024 }, actions: ["openEngine"] });
    const full = input({}, { components: { cas: { status: "unavailable", code: "DOCS_ENGINE_DISK_FULL" } } });
    expect(find(full, "disk_full")?.dismissible).toBe(false);
    expect(find(full, "disk_low")).toBeUndefined();
  });

  it("a Drive being removed is a banner that cannot be closed and blocks writes, like a full disk", () => {
    const deleting = input({ status: "deleting" });
    expect(find(deleting, "drive_deleting")).toMatchObject({ tone: "info", dismissible: false });
    expect(writeBlockOf(computeBanners(deleting))).toBe("drive_deleting");
    expect(writeBlockOf(computeBanners(input({}, { disk: { free_bytes: 0 } })))).toBe("disk_full");
    expect(writeBlockOf(computeBanners(input({})))).toBeNull();
    expect(writeBlockOf(computeBanners(input({ status: "deleting" }, { disk: { free_bytes: 0 } })))).toBe("disk_full");
  });

  it("a paused Drive offers to resume", () => {
    expect(find(input({ status: "paused" }), "drive_paused")?.actions).toEqual(["resumeDrive"]);
  });
});
