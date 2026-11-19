import type { FileView } from "../superDriveFileView";

/** What a context menu acts on (§3.2.5). */
export type MenuTarget =
  | { kind: "file"; fileId: string; path: string; name: string; view: FileView; duplicateOf?: string | null }
  | { kind: "dir"; path: string; writesBlocked?: boolean }
  | { kind: "multi"; fileIds: readonly string[]; retryable: number; safeRetryable: number }
  | { kind: "blank"; writesBlocked?: boolean };

export type MenuActionId =
  | "openDetails" | "copyPath" | "retry" | "retrySafe" | "openFolder" | "onlyFailures" | "syncNow" | "retrySelected" | "retrySelectedSafe" | "clearSelection" | "viewList" | "viewGrid" | "openOriginal";

export type MenuItemModel = { id: MenuActionId; messageId: string };

const item = (id: MenuActionId): MenuItemModel => ({ id, messageId: `super-drive.menu.${id}` });

/** Items that do not apply are left out, not greyed (§3.2.5). */
export function menuItemsFor(target: MenuTarget): MenuItemModel[] {
  switch (target.kind) {
    case "file":
      return [item("openDetails"), ...(target.view.state === "duplicate" && target.duplicateOf ? [item("openOriginal")] : []), item("copyPath"), ...(target.view.canRetry ? [item("retry")] : []), ...(target.view.canRetrySafe ? [item("retrySafe")] : [])];
    case "dir":
      return [item("openFolder"), item("onlyFailures"), ...(target.writesBlocked ? [] : [item("syncNow")])];
    case "multi":
      return [...(target.retryable > 0 ? [item("retrySelected")] : []), ...(target.safeRetryable > 0 ? [item("retrySelectedSafe")] : []), item("clearSelection")];
    case "blank":
      return [...(target.writesBlocked ? [] : [item("syncNow")]), item("viewList"), item("viewGrid")];
    default:
      return [];
  }
}
