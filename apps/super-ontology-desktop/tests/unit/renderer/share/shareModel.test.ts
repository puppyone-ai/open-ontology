import { describe, expect, it } from "vitest";
import { groupShares, levelTone, presentShare, shareRank } from "@so-model/share/shareModel";
import { makeShare } from "../../../support/fakeApi";

const NOW = Date.parse("2026-11-18T00:00:00Z");
const fmt = (iso: string) => iso.slice(0, 10);
const view = (overrides: Parameters<typeof makeShare>[1], pending?: "refreshing" | "revoking" | "mounting") => presentShare(makeShare("s1", overrides), { now: NOW, formatDate: fmt, pending });

describe("share state matrix (20 section 4.2)", () => {
  it("covers every state with its message, tone and actions", () => {
    const rows = [
      [{ state: "active" as const }, "active", "super-drive.share.state.active", "success", ["viewBrief", "reveal", "copyCommand", "revoke"]],
      [{ state: "active" as const, refreshed_from: "s0" }, "activeRefreshed", "super-drive.share.state.activeRefreshed", "success", ["viewBrief", "reveal", "copyCommand", "revoke", "viewChain"]],
      [{ state: "active" as const, mount: { target_dir: "/x", present: false } }, "mountMissing", "super-drive.share.mount.missing", "warning", ["remount", "revoke"]],
      [{ state: "drifted" as const, drift: { facts_changed: 3, files_changed: 1 } }, "drifted", "super-drive.share.state.drifted", "warning", ["refresh", "viewChanges", "revoke"]],
      [{ state: "refreshed" as const }, "refreshed", "super-drive.share.state.refreshed", "neutral", ["viewNew", "viewBrief"]],
      [{ state: "expired" as const }, "expired", "super-drive.share.state.expired", "neutral", ["reshare", "viewBrief"]],
      [{ state: "revoked" as const, revoked_at: "2026-11-17T00:00:00Z" }, "revoked", "super-drive.share.state.revoked", "neutral", ["reshare", "viewBrief"]],
    ] as const;
    for (const [overrides, id, messageId, tone, actions] of rows) {
      const result = view(overrides as never);
      expect([result.id, result.messageId, result.tone, [...result.actions]], id).toEqual([id, messageId, tone, [...actions]]);
    }
  });

  it("shows the drift counts and ended shares as terminal", () => {
    expect(view({ state: "drifted", drift: { facts_changed: 3, files_changed: 1 } }).values).toEqual({ facts: 3, files: 1 });
    expect(view({ state: "revoked", revoked_at: "2026-11-17T00:00:00Z" }).values).toEqual({ date: "2026-11-17" });
    expect(["refreshed", "expired", "revoked"].every((state) => view({ state: state as never }).terminal)).toBe(true);
    expect(view({ state: "active" }).terminal).toBe(false);
  });

  it("shows an unknown state as unknown instead of failing", () => {
    const result = view({ state: "paused" as never });
    expect(result).toMatchObject({ id: "unknown", messageId: "super-drive.share.state.unknown", values: { state: "paused" } });
  });

  it("an operation in flight replaces the state and offers nothing", () => {
    expect(view({ state: "active" }, "refreshing")).toMatchObject({ id: "pending", messageId: "super-drive.share.pending.refreshing", actions: [] });
    expect(view({ state: "active" }, "revoking").messageId).toBe("super-drive.share.pending.revoking");
  });

  it("adds an extend action and the hours when a live share ends within a day", () => {
    const soon = view({ state: "active", expires_at: "2026-11-18T10:00:00Z" });
    expect(soon.expiresInHours).toBe(10);
    expect(soon.actions).toContain("extend");
    expect(view({ state: "active", expires_at: "2026-11-25T00:00:00Z" }).expiresInHours).toBeNull();
    expect(view({ state: "expired", expires_at: "2026-11-18T10:00:00Z" }).actions).not.toContain("extend");
  });
});

describe("ordering and groups", () => {
  it("ranks needs-refresh first, then active, then ended", () => {
    expect(["revoked", "active", "drifted", "expired"].sort((a, b) => shareRank({ state: a as never }) - shareRank({ state: b as never }))).toEqual(["drifted", "active", "revoked", "expired"]);
  });

  it("maps update levels to tones, block being the danger one", () => {
    expect([levelTone("block"), levelTone("warn"), levelTone("info"), levelTone(null)]).toEqual(["danger", "warning", "info", "neutral"]);
  });

  it("groups shares of one ticket and leaves a lone member single", () => {
    const shares = [makeShare("a", { group_id: "g1" }), makeShare("b", { group_id: "g1" }), makeShare("c", { group_id: "g2" }), makeShare("d")];
    const { groups, singles } = groupShares(shares);
    expect(groups.map((group) => [group.groupId, group.members.map((member) => member.id)])).toEqual([["g1", ["a", "b"]]]);
    expect(singles.map((share) => share.id).sort()).toEqual(["c", "d"]);
  });
});
