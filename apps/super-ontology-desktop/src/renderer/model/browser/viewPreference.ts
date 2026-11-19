import type { ViewMode } from "../superDriveRoute";

/** Which view a Drive is shown in (§3.2.2: remembered per Drive). Stored on this computer only; a missing or unreadable value means the list. */
const key = (driveId: string) => `so.sources.view.${driveId}`;

export function readViewPreference(driveId: string, storage: Pick<Storage, "getItem"> | undefined = safeStorage()): ViewMode {
  try {
    return storage?.getItem(key(driveId)) === "grid" ? "grid" : "list";
  } catch {
    return "list";
  }
}

export function writeViewPreference(driveId: string, view: ViewMode, storage: Pick<Storage, "setItem" | "removeItem"> | undefined = safeStorage()): void {
  try {
    if (view === "grid") storage?.setItem(key(driveId), "grid");
    else storage?.removeItem(key(driveId));
  } catch {
    // The preference is a convenience; a full or blocked store only means it is not remembered.
  }
}

function safeStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}
