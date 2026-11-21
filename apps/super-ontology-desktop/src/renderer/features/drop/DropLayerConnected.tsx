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
  const keepInSync = useCallback(async (root: { path: string; name: string }, batchIds: string[]) => {
    const grant = await invoke("grant-dropped-folder", { path: root.path });
    if (!grant.ok) return { ok: false as const, error: toSuperDriveError(grant) };
    const created = await actions.createDrive({ name: root.name, rootGrantId: grant.value.grantId });
    if (!created.ok) return { ok: false as const, error: created.error };
    // The folder is a source now: the copies this drop made of it in My files would only be duplicates of it.
    await getUploadQueue().discard(batchIds, root.path);
    return { ok: true as const };
  }, [actions]);
  // A drop from any screen shows the Sources screen at once (leaving it alone when it is already there, so an open source stays open).
  const { section, go } = app;
  const onDrop = useCallback(() => { if (section !== "sources") go("sources"); }, [section, go]);
  return (
    <DropLayer drives={drives} firstRunOpen={app.firstRunOpen} ensureMyFiles={() => actions.ensureMyFiles()} enqueue={(driveId, folder, files) => getUploadQueue().enqueueFiles(driveId, folder, files)}
      keepInSync={keepInSync} decide={(batchId, choice, paths) => getUploadQueue().decide(batchId, choice, paths)} cancelBatches={(ids) => getUploadQueue().cancelBatches(ids)} onDrop={onDrop} />
  );
}
