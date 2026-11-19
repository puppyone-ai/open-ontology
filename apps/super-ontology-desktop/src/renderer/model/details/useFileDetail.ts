import { useEffect, useState } from "react";
import type { FileDetailDoc } from "./types";
import { useSuperDriveActions, useSuperDriveSelector } from "../SuperDriveProvider";
import type { SuperDriveError } from "../superDriveErrors";

/**
 * The detail document of one file. It reloads whenever an event changed the file (the record's `seq` moves and the store
 * dropped its cached copy), so the pane never shows a state the list has already left.
 */
export function useFileDetail(fileId: string | null) {
  const actions = useSuperDriveActions();
  const seq = useSuperDriveSelector((state) => (fileId ? state.files[fileId]?.seq ?? 0 : 0));
  const [detail, setDetail] = useState<FileDetailDoc | null>(null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!fileId) { setDetail(null); return undefined; }
    let current = true;
    setLoading(true);
    void actions.loadFileDetail(fileId).then((result) => {
      if (!current) return;
      setLoading(false);
      if (result.ok) { setDetail(result.value as unknown as FileDetailDoc); setError(null); } else { setError(result.error); }
    });
    return () => { current = false; };
  }, [actions, fileId, seq]);

  return { detail: detail?.id === fileId ? detail : null, error, loading };
}
