import type { Share, ShareState } from "../contract/docsApi.generated";

export type ShareIconName = "Share2" | "FolderOpen" | "TriangleAlert" | "RotateCw" | "Clock3" | "Unlink" | "LoaderCircle" | "CircleHelp";
export type ShareTone = "success" | "warning" | "neutral" | "info";

export type ShareView = {
  /** Which row of the state table (20 section 4.2) applies. */
  id: "active" | "activeRefreshed" | "mountMissing" | "drifted" | "refreshed" | "expired" | "revoked" | "unknown" | "pending";
  icon: ShareIconName;
  tone: ShareTone;
  messageId: string;
  values: Record<string, string | number>;
  terminal: boolean;
  actions: readonly ShareActionId[];
  /** Hours until expiry when it is within a day (`active` and `drifted` only). */
  expiresInHours: number | null;
};

export type ShareActionId = "viewBrief" | "reveal" | "copyCommand" | "revoke" | "refresh" | "viewChanges" | "reshare" | "viewNew" | "remount" | "extend" | "viewChain";

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;

export function hoursUntil(iso: string | undefined, now: number): number | null {
  if (!iso) return null;
  const at = Date.parse(iso);
  return Number.isNaN(at) ? null : Math.max(0, Math.ceil((at - now) / HOUR_MS));
}

/**
 * One Share as the list shows it. Pure: the caller supplies `now` and a `date` formatter, so the state matrix is testable
 * for every row without rendering anything.
 */
export function presentShare(share: Share, { now, formatDate, pending }: { now: number; formatDate: (iso: string) => string; pending?: "refreshing" | "revoking" | "mounting" | null }): ShareView {
  const expiresIn = hoursUntil(share.expires_at, now);
  const soon = expiresIn !== null && Date.parse(share.expires_at) - now <= DAY_MS ? expiresIn : null;
  const drift = share.drift ?? null;
  const base = { values: {} as Record<string, string | number>, expiresInHours: null as number | null };
  if (pending) {
    return { ...base, id: "pending", icon: "LoaderCircle", tone: "info", messageId: `super-drive.share.pending.${pending}`, terminal: false, actions: [] };
  }
  const state: ShareState | string = share.state;
  switch (state) {
    case "active": {
      const present = share.mount?.present !== false;
      const refreshed = typeof share.refreshed_from === "string" && share.refreshed_from.length > 0;
      const actions: ShareActionId[] = present ? ["viewBrief", "reveal", "copyCommand", "revoke"] : ["remount", "revoke"];
      if (refreshed) actions.push("viewChain");
      if (soon !== null) actions.push("extend");
      if (!present) return { ...base, id: "mountMissing", icon: "FolderOpen", tone: "warning", messageId: "super-drive.share.mount.missing", terminal: false, actions, expiresInHours: soon };
      if (refreshed) return { ...base, id: "activeRefreshed", icon: "Share2", tone: "success", messageId: "super-drive.share.state.activeRefreshed", values: { date: formatDate(share.created_at) }, terminal: false, actions, expiresInHours: soon };
      return { ...base, id: "active", icon: "Share2", tone: "success", messageId: "super-drive.share.state.active", terminal: false, actions, expiresInHours: soon };
    }
    case "drifted": {
      const actions: ShareActionId[] = ["refresh", "viewChanges", "revoke"];
      if (soon !== null) actions.push("extend");
      return { ...base, id: "drifted", icon: "TriangleAlert", tone: "warning", messageId: "super-drive.share.state.drifted", values: { facts: drift?.facts_changed ?? 0, files: drift?.files_changed ?? 0 }, terminal: false, actions, expiresInHours: soon };
    }
    case "refreshed":
      return { ...base, id: "refreshed", icon: "RotateCw", tone: "neutral", messageId: "super-drive.share.state.refreshed", terminal: true, actions: ["viewNew", "viewBrief"] };
    case "expired":
      return { ...base, id: "expired", icon: "Clock3", tone: "neutral", messageId: "super-drive.share.state.expired", values: { date: formatDate(share.expires_at) }, terminal: true, actions: ["reshare", "viewBrief"] };
    case "revoked":
      return { ...base, id: "revoked", icon: "Unlink", tone: "neutral", messageId: "super-drive.share.state.revoked", values: { date: formatDate(share.revoked_at ?? share.expires_at) }, terminal: true, actions: ["reshare", "viewBrief"] };
    default:
      return { ...base, id: "unknown", icon: "CircleHelp", tone: "neutral", messageId: "super-drive.share.state.unknown", values: { state: String(state) }, terminal: false, actions: ["viewBrief"] };
  }
}

/** Sort rank of the list: needs-refresh first, then active, then the ended ones (kept in the history group). */
export function shareRank(share: Pick<Share, "state">): number {
  if (share.state === "drifted") return 0;
  if (share.state === "active") return 1;
  return 2;
}

export type UpdateLevel = "info" | "warn" | "block";
export const LEVEL_RANK: Record<UpdateLevel, number> = { info: 0, warn: 1, block: 2 };

/** Block-level updates are the loudest thing in the list (danger tone), then warn, then info (D55). */
export const levelTone = (level: UpdateLevel | null | undefined): "danger" | "warning" | "info" | "neutral" => (level === "block" ? "danger" : level === "warn" ? "warning" : level === "info" ? "info" : "neutral");

export type ShareGroupRow = { groupId: string; members: Share[] };

/** Shares of one task group (D55): the group id the engine gave, members newest first; shares without a group stay single. */
export function groupShares(shares: readonly Share[]): { groups: ShareGroupRow[]; singles: Share[] } {
  const byGroup = new Map<string, Share[]>();
  const singles: Share[] = [];
  for (const share of shares) {
    if (share.group_id) byGroup.set(share.group_id, [...(byGroup.get(share.group_id) ?? []), share]);
    else singles.push(share);
  }
  return { groups: [...byGroup.entries()].filter(([, members]) => members.length > 1).map(([groupId, members]) => ({ groupId, members })), singles: [...singles, ...[...byGroup.values()].filter((members) => members.length === 1).flat()] };
}
