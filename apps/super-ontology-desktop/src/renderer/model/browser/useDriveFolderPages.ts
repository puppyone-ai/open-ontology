import { useCallback, useEffect, useMemo } from "react";
import { useSuperDriveActions, useSuperDriveSelector } from "../SuperDriveProvider";
import { folderKey, type FolderFilter } from "../superDriveRoute";
import { selectFolder, selectFolderFiles } from "../superDriveSelectors";
import type { FileRecord, FolderPage } from "../superDriveState";

export type DriveFolderPages = {
  page: FolderPage;
  files: FileRecord[];
  /** First load in progress and nothing to show yet. */
  initialLoading: boolean;
  loadMore: () => void;
  reload: () => void;
};

/** Loads and follows one folder listing: fetches on mount and again when it turns stale; `loadFolder` skips fresh pages. */
export function useDriveFolderPages(driveId: string | null, path: string, filterInput?: FolderFilter): DriveFolderPages {
  const actions = useSuperDriveActions();
  // Callers build the filter inline; keep one identity per distinct content.
  const filterJson = JSON.stringify(filterInput ?? null);
  const filter = useMemo<FolderFilter | undefined>(() => (JSON.parse(filterJson) as FolderFilter | null) ?? undefined, [filterJson]);
  const key = driveId ? folderKey(driveId, path, filter) : null;
  const page = useSuperDriveSelector((state) => selectFolder(state, driveId, path, filter));
  const files = useSuperDriveSelector(
    (state) => selectFolderFiles(state, selectFolder(state, driveId, path, filter)),
    (left, right) => left.length === right.length && left.every((record, index) => record === right[index]),
  );
  const stale = page.stale;
  useEffect(() => {
    if (driveId) void actions.loadFolder({ driveId, path, filter });
  }, [actions, driveId, path, filter, stale]);
  const loadMore = useCallback(() => { if (key) void actions.loadMore(key); }, [actions, key]);
  const reload = useCallback(() => { if (key) void actions.reloadFolder(key); }, [actions, key]);
  return { page, files, initialLoading: page.loading && page.ids.length === 0 && page.error === null, loadMore, reload };
}
