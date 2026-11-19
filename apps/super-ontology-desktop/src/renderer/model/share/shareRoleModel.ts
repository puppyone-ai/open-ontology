/**
 * What each Share role means in words (docs-api `ShareRole`, 20-frontend §3.6, F-S7-02). The texts promise only what is built:
 * a viewer reads, a suggester may also write suggestions that wait for a person, and an editor's accepted suggestions are written
 * back into the files by Desktop, which it can do for a folder Drive on this computer and not for object storage.
 */
import type { Drive, Suggestion } from "../contract/docsApi.generated";

export type ShareRole = "viewer" | "suggester" | "editor";
export const SHARE_ROLES: readonly ShareRole[] = ["viewer", "suggester", "editor"];

export type RoleAvailability = { available: boolean; reasonMessageId: string | null };

/** Editor needs somewhere Desktop can write: a folder Drive. Anything else keeps the role but says it cannot write back. */
export function roleAvailability(role: ShareRole, backend: Drive["backend"] | null | undefined): RoleAvailability {
  if (role === "editor" && backend !== undefined && backend !== null && backend !== "fs") return { available: false, reasonMessageId: "super-drive.share.role.editor.needsFolderDrive" };
  return { available: true, reasonMessageId: null };
}

export const roleLabelId = (role: string) => (SHARE_ROLES as readonly string[]).includes(role) ? `super-drive.share.role.${role}` : "super-drive.share.role.unknown";
export const roleDescriptionId = (role: string) => (SHARE_ROLES as readonly string[]).includes(role) ? `super-drive.share.role.${role}.description` : "super-drive.share.role.unknown.description";

/** Whether the agent of a Share with this role can write suggestions at all (the "Agent can write suggestions" mark). */
export const agentCanSuggest = (role: string): boolean => role === "suggester" || role === "editor";

/** What the Share row says about suggestions: whether the agent may write them and how many the collector has brought in (the `suggest/` folder). */
export function suggestSummary(share: { id: string; role?: string | null }, suggestions: readonly Pick<Suggestion, "share_id" | "status" | "submitted_by">[]) {
  const own = suggestions.filter((suggestion) => suggestion.share_id === share.id);
  return {
    canSuggest: agentCanSuggest(share.role ?? ""),
    collected: own.filter((suggestion) => suggestion.submitted_by === "collector").length,
    pending: own.filter((suggestion) => suggestion.status === "pending").length,
    total: own.length,
  };
}
