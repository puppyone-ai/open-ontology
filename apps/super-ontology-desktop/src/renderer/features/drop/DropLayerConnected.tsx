import { useCallback } from "react";
import { useApp } from "../../hooks/useApp";
import { invoke } from "../../model/bridgeInvoke";
import { toSuperDriveError } from "../../model/superDriveErrors";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives } from "../../model/superDriveSelectors";
import { getUploadQueue } from "../../model/uploadStore";
import { DropLayer } from "./DropLayer";

/** The window's drop layer wired to the app: the Drives it lists, the upload queue, and local folder sources. */
export function DropLayerConnected() {
  const app = useApp();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((drive, index) => drive === b[index]));
  const keepInSync = useCallback(async (root: { path: string; name: string }) => {
    const grant = await invoke("grant-dropped-folder", { path: root.path });
    if (!grant.ok) return { ok: false as const, error: toSuperDriveError(grant) };
    const created = await actions.createDrive({ name: root.name, rootGrantId: grant.value.grantId });
    return created.ok ? { ok: true as const } : { ok: false as const, error: created.error };
  }, [actions]);
  return <DropLayer drives={drives} firstRunOpen={app.firstRunOpen} ensureMyFiles={() => actions.ensureMyFiles()} enqueue={(driveId, folder, files) => getUploadQueue().enqueueFiles(driveId, folder, files)} keepInSync={keepInSync} />;
}
