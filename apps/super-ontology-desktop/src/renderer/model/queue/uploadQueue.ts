import type { SuperOntologyPush, UploadProgressPush } from "../../../shared/super-ontology/bridge";
import type { SuperDriveApi } from "../superDriveApi";
import { toSuperDriveError, type SuperDriveError } from "../superDriveErrors";

/** D31 / 10 §4.11: above these sizes a file is stored but not read. */
export const UPLOAD_SIZE_LIMIT_BYTES = 200 * 1024 * 1024;
export const UPLOAD_TEXT_LIMIT_BYTES = 64 * 1024 * 1024;
export const DRIVE_FILE_LIMIT = 10_000;
/** A finished upload stays this long, then counts as processing (20-frontend §3.3). */
export const DONE_RETAIN_MS = 5_000;
/** `fs_poll_seconds`: the engine should have found a copied file by then, otherwise "Sync now" is offered. */
export const HANDOFF_PATIENCE_MS = 30_000;

export type UploadItemState = "waiting" | "copying" | "done" | "failed" | "cancelled";

export type UploadItem = {
  id: string;
  batchId: string;
  driveId: string;
  name: string;
  relative: string;
  /** Drive folder the file lands in (the target folder plus the dropped sub folders). */
  folder: string;
  source: string;
  size: number;
  state: UploadItemState;
  bytesDone: number;
  code: string | null;
  /** Final Drive path (a clash gives "name (2).ext"). */
  drivePath: string | null;
  finishedAt: number | null;
  /** Over the processing limit: stored but not read. */
  overLimit: boolean;
};

export type UploadQueueState = {
  items: UploadItem[];
  skipped: { hidden: number; links: number; unreadable: number };
  /** A start request that failed as a whole (no engine, not an fs Drive, ...). */
  error: SuperDriveError | null;
  /** The panel was closed by the user; reopens on the next upload or failure. */
  dismissed: boolean;
};

export const initialUploadQueueState = (): UploadQueueState => ({ items: [], skipped: { hidden: 0, links: 0, unreadable: 0 }, error: null, dismissed: false });

export function isOverLimit(name: string, size: number): boolean {
  const text = /\.(md|markdown|txt)$/iu.test(name);
  return size > (text ? UPLOAD_TEXT_LIMIT_BYTES : UPLOAD_SIZE_LIMIT_BYTES);
}

export const isUploadActive = (item: UploadItem): boolean => item.state === "waiting" || item.state === "copying";

/** Applies one `upload-progress` push; unknown items are ignored (the push can outrun the start response). */
export function applyUploadProgress(state: UploadQueueState, push: UploadProgressPush, now: number): UploadQueueState {
  let changed = false;
  const reported = new Map(push.items.map((item) => [item.itemId, item]));
  const items = state.items.map((item) => {
    const next = item.batchId === push.batchId ? reported.get(item.id) : undefined;
    if (!next) return item;
    const nextState = (["waiting", "copying", "done", "failed", "cancelled"].includes(next.state) ? next.state : item.state) as UploadItemState;
    if (nextState === item.state && next.bytesDone === item.bytesDone && (next.code ?? null) === item.code) return item;
    changed = true;
    const finished = nextState === "done" || nextState === "failed" || nextState === "cancelled";
    return {
      ...item, state: nextState, bytesDone: next.bytesDone, code: next.code ?? null, drivePath: next.drivePath ?? item.drivePath,
      finishedAt: finished ? item.finishedAt ?? now : null,
    };
  });
  if (!changed) return state;
  const failedNow = items.some((item) => item.state === "failed");
  return { ...state, items, dismissed: failedNow ? false : state.dismissed };
}

/** What a finished upload shows while the engine has not listed the file yet. */
export type HandoffStatus = "uploading" | "retained" | "waiting_engine" | "needs_sync" | "handed_over" | "gone";

export function handoffStatus(item: UploadItem, engineHasFile: boolean, now: number): HandoffStatus {
  if (item.state === "failed" || item.state === "cancelled") return "gone";
  if (isUploadActive(item)) return "uploading";
  const age = now - (item.finishedAt ?? now);
  if (engineHasFile) return age < DONE_RETAIN_MS ? "retained" : "handed_over";
  return age >= HANDOFF_PATIENCE_MS ? "needs_sync" : "waiting_engine";
}

export type UploadSummary = { active: number; failed: number; bytesDone: number; bytesTotal: number };

export function summarizeUploads(state: UploadQueueState): UploadSummary {
  let active = 0;
  let failed = 0;
  let bytesDone = 0;
  let bytesTotal = 0;
  for (const item of state.items) {
    if (isUploadActive(item)) { active += 1; bytesDone += item.bytesDone; bytesTotal += item.size; }
    if (item.state === "failed") failed += 1;
  }
  return { active, failed, bytesDone, bytesTotal };
}

export type UploadQueue = ReturnType<typeof createUploadQueue>;

/**
 * Client side of the upload queue (20-frontend §3.3): turns a drop into an upload batch in the main process and folds
 * its progress pushes into a small state the queue panel and the drop zone read. Framework independent.
 */
export function createUploadQueue({ api, now = () => Date.now() }: { api: Pick<SuperDriveApi, "pathsForFiles" | "startUpload" | "cancelUpload" | "subscribePushes">; now?: () => number }) {
  let state = initialUploadQueueState();
  const listeners = new Set<() => void>();
  let unsubscribe: (() => void) | null = null;
  const set = (next: UploadQueueState) => { if (next !== state) { state = next; for (const listener of [...listeners]) listener(); } };

  function ensurePushes() {
    if (unsubscribe) return;
    unsubscribe = api.subscribePushes((push: SuperOntologyPush) => {
      if (push.channel === "upload-progress") set(applyUploadProgress(state, push.payload, now()));
    });
  }

  async function startPaths(driveId: string, folder: string, paths: string[], unreadable: number) {
    const result = await api.startUpload({ driveId, folder, paths });
    if (!result.ok) { set({ ...state, error: toSuperDriveError(result), dismissed: false }); return; }
    const { batchId, items, skipped, unreadable: refused } = result.value;
    const added: UploadItem[] = items.map((item) => {
      const sub = item.relative.includes("/") ? item.relative.slice(0, item.relative.lastIndexOf("/")) : "";
      return {
        id: item.itemId, batchId, driveId, name: item.name, relative: item.relative, source: item.source, size: item.size,
        folder: [folder, sub].filter(Boolean).join("/"), state: "waiting" as const, bytesDone: 0, code: null, drivePath: null,
        finishedAt: null, overLimit: isOverLimit(item.name, item.size),
      };
    });
    set({
      items: [...state.items.filter((item) => !(added.some((entry) => entry.source === item.source) && item.state === "failed")), ...added],
      skipped: { hidden: state.skipped.hidden + skipped.hidden, links: state.skipped.links + skipped.links, unreadable: state.skipped.unreadable + unreadable + refused },
      error: null, dismissed: false,
    });
  }

  return Object.freeze({
    getState: () => state,
    subscribe(listener: () => void) { listeners.add(listener); ensurePushes(); return () => { listeners.delete(listener); }; },

    /** Starts uploading dropped or picked files into `folder` of `driveId`. */
    async enqueueFiles(driveId: string, folder: string, files: ArrayLike<File>) {
      const { paths, unreadable } = api.pathsForFiles(files);
      if (paths.length === 0) { set({ ...state, skipped: { ...state.skipped, unreadable: state.skipped.unreadable + unreadable }, dismissed: false }); return; }
      await startPaths(driveId, folder, paths, unreadable);
    },

    async cancel(itemId: string) {
      const item = state.items.find((entry) => entry.id === itemId);
      if (item) await api.cancelUpload({ batchId: item.batchId, itemId });
    },
    async cancelAll() {
      for (const batchId of new Set(state.items.filter(isUploadActive).map((item) => item.batchId))) await api.cancelUpload({ batchId });
    },
    /** Uploads the failed item's file again into the same folder. */
    async retry(itemId: string) {
      const item = state.items.find((entry) => entry.id === itemId);
      if (!item || item.state !== "failed") return;
      await startPaths(item.driveId, item.folder, [item.source], 0);
    },
    dismiss() { set({ ...state, dismissed: true }); },
    clearError() { set({ ...state, error: null }); },
    /** Drops finished items the engine has taken over. */
    prune(handedOver: (item: UploadItem) => boolean) {
      const items = state.items.filter((item) => !(item.state === "done" && handedOver(item)) && item.state !== "cancelled");
      if (items.length !== state.items.length) set({ ...state, items });
    },
    dispose() { unsubscribe?.(); unsubscribe = null; listeners.clear(); },
  });
}
