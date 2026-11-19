import { describe, expect, it } from "vitest";
import { computeBanners, MAX_VISIBLE_BANNERS, pruneDismissed, visibleBanners, type BannerInput } from "@so-model/banners/bannerModel";
import type { Drive } from "@so-contract/docs-api.generated";
import { makeDrive, makeSnapshot } from "../../../support/fakeApi";

const input = (overrides: Omit<Partial<BannerInput>, "drive"> & { drive?: Partial<Drive> } = {}): BannerInput => ({
  engine: makeSnapshot(), connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 1 }, hasData: true,
  ...overrides, drive: overrides.drive === undefined ? makeDrive({ files_total: 10, counts: { published: 10 } }) : makeDrive(overrides.drive),
});
const kinds = (value: BannerInput) => computeBanners(value).map((banner) => banner.kind);

describe("banner conditions (§4.7)", () => {
  it("shows nothing for a healthy, fully published Drive", () => {
    expect(kinds(input())).toEqual([]);
  });

  it("orders by priority: engine, disk and held deletion, connection, source problems, partial failure, then the rest", () => {
    const everything = input({
      engine: makeSnapshot({ status: "stopped" }),
      connection: { state: "reconnecting", attempt: 1, nextRetryAt: null, lastSeq: 1 },
      drive: { status_code: "DOCS_SOURCE_PERMISSION_DENIED", last_sync: { status: "held", generation: 4 }, files_total: 10, counts: { parse_failed: 2, queued: 8 }, status: "paused" },
    });
    expect(kinds(everything)).toEqual(["engine_unavailable", "mass_delete_held", "reconnecting", "permission_denied", "partial_failure", "drive_paused"]);
    expect(computeBanners(everything).map((banner) => banner.priority)).toEqual([1, 2, 3, 4, 5, 8]);
  });

  it("shows at most two and counts the rest", () => {
    const banners = computeBanners(input({ engine: makeSnapshot({ status: "crashed" }), drive: { last_sync: { status: "failed", error_code: "DOCS_SOURCE_ROOT_MISSING" }, counts: { parse_failed: 1 }, files_total: 5 } }));
    const { shown, hidden } = visibleBanners(banners, new Set());
    expect(MAX_VISIBLE_BANNERS).toBe(2);
    expect(shown.map((banner) => banner.kind)).toEqual(["engine_unavailable", "sync_failed"]);
    expect(hidden.map((banner) => banner.kind)).toEqual(["partial_failure"]);
  });

  it("does not let the user close disk full or a held mass deletion", () => {
    const banners = computeBanners(input({ drive: { status_code: "DOCS_ENGINE_DISK_FULL", last_sync: { status: "held", generation: 2 }, files_total: 1, counts: { published: 1 } } }));
    expect(banners.map((banner) => [banner.kind, banner.dismissible])).toEqual([["disk_full", false], ["mass_delete_held", false]]);
    const { shown } = visibleBanners(banners, new Set(["disk_full", "mass_delete_held"]));
    expect(shown).toHaveLength(2);
    expect(banners[1].values.generation).toBe(2);
  });

  it("says nothing about an engine that went away before any data was shown, and alerts once on a crash", () => {
    expect(kinds(input({ engine: makeSnapshot({ status: "stopped" }), hasData: false }))).toEqual([]);
    const crashed = computeBanners(input({ engine: makeSnapshot({ status: "crashed" }) }))[0];
    expect(crashed).toMatchObject({ kind: "engine_unavailable", alert: true, actions: ["openEngine"] });
    expect(computeBanners(input({ engine: makeSnapshot({ status: "stopped" }) }))[0].alert).toBe(false);
  });

  it("tells resyncing from reconnecting and low disk from a full disk", () => {
    expect(kinds(input({ connection: { state: "resyncing", attempt: 0, nextRetryAt: null, lastSeq: 1 } }))).toEqual(["resyncing"]);
    expect(kinds(input({ engine: makeSnapshot({ health: { components: { cas: { status: "degraded", code: "DOCS_ENGINE_DISK_LOW" } } } }) }))).toEqual(["disk_low"]);
    expect(kinds(input({ engine: makeSnapshot({ health: { disk: { free_bytes: 0 } } }) }))).toEqual(["disk_full"]);
    expect(kinds(input({ engine: makeSnapshot({ status: "degraded" }) }))).toEqual(["engine_degraded"]);
  });

  it("offers the right actions for a partial failure and a model limit", () => {
    const partial = computeBanners(input({ drive: { counts: { parse_failed: 2, extract_partial: 1, published: 7 }, files_total: 10 } }))[0];
    expect(partial).toMatchObject({ kind: "partial_failure", values: { count: 3 }, actions: ["viewFailures", "retryAll"] });
    expect(kinds(input({ drive: { status_code: "DOCS_EXTRACT_LLM_LIMIT_DRIVE", files_total: 1, counts: { published: 1 } } }))).toEqual(["model_limit"]);
  });
});

describe("dismissal memory", () => {
  it("keeps a dismissal only while the condition lasts, so it returns when the condition comes back", () => {
    const failing = computeBanners(input({ drive: { counts: { parse_failed: 1 }, files_total: 3 } }));
    const dismissed = new Set(["partial_failure" as const]);
    expect(pruneDismissed(dismissed, failing)).toBe(dismissed);
    expect(visibleBanners(failing, dismissed).shown).toEqual([]);
    const gone = pruneDismissed(dismissed, computeBanners(input()));
    expect([...gone]).toEqual([]);
    expect(visibleBanners(failing, gone).shown.map((banner) => banner.kind)).toEqual(["partial_failure"]);
  });
});
