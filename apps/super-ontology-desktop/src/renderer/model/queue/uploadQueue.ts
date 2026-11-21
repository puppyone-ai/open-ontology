import type { SuperOntologyPush, UploadProgressPush, UploadSkipReason, UploadStartValue } from "../../../shared/super-ontology/bridge";
import type { SuperDriveApi } from "../superDriveApi";
import { toSuperDriveError, type SuperDriveError } from "../superDriveErrors";
import { normalizeDropPaths, UPLOAD_PATHS_PER_REQUEST } from "./dropPaths";

/** D31 / 10 §4.11: above these sizes a file is stored but not read. */
export const UPLOAD_SIZE_LIMIT_BYTES = 200 * 1024 * 1024;
export const UPLOAD_TEXT_LIMIT_BYTES = 64 * 1024 * 1024;
export const DRIVE_FILE_LIMIT = 10_000;
/** A finished upload stays this long, then counts as processing (20-frontend §3.3). */
export const DONE_RETAIN_MS = 5_000;
/** `fs_poll_seconds`: the engine should have found a copied file by then, otherwise "Sync now" is offered. */
export const HANDOFF_PATIENCE_MS = 30_000;

/** `skipped`: the same file was already there (the same drop twice), so nothing was copied. */
export type UploadItemState = "waiting" | "copying" | "done" | "failed" | "cancelled" | "skipped";
export { UPLOAD_PATHS_PER_REQUEST };
/** Pushes that arrive before their start answer are kept (up to this many batches) and applied when it does. */
const EARLY_PUSH_BATCHES = 8;

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
  /** Why a `skipped` item was not copied. */
  skipReason?: "duplicate" | null;
};

/** What a drop left out, by reason, summed over the drops still listed; `samples` names a few of them. */
export type SkippedCounts = { hidden: number; links: number; junk: number; temporary: number; special: number; unreadable: number };
export type SkippedSample = { name: string; reason: UploadSkipReason };

/** What one drop turned into (the toast reads it): the files queued, what was left out, and the folders that were dropped. */
export type DropOutcome = {
  queued: number;
  skipped: SkippedCounts;
  roots: NonNullable<UploadStartValue["roots"]>;
  emptyFolders: number;
  /** The batches the drop started (the choice to keep a folder in sync takes the copies of these back). */
  batchIds: string[];
  error: SuperDriveError | null;
};

export type UploadQueueState = {
  items: UploadItem[];
  skipped: SkippedCounts;
  skippedSamples: SkippedSample[];
  /** A start request that failed as a whole (no engine, not an fs Drive, ...). */
  error: SuperDriveError | null;
  /** The panel was closed by the user; reopens on the next upload or failure. */
  dismissed: boolean;
};

export const emptySkipped = (): SkippedCounts => ({ hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, unreadable: 0 });
export const initialUploadQueueState = (): UploadQueueState => ({ items: [], skipped: emptySkipped(), skippedSamples: [], error: null, dismissed: false });

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
    const nextState = (["waiting", "copying", "done", "failed", "cancelled", "skipped"].includes(next.state) ? next.state : item.state) as UploadItemState;
    if (nextState === item.state && next.bytesDone === item.bytesDone && (next.code ?? null) === item.code) return item;
    changed = true;
    const finished = nextState === "done" || nextState === "failed" || nextState === "cancelled" || nextState === "skipped";
    return {
      ...item, state: nextState, bytesDone: next.bytesDone, code: next.code ?? null, drivePath: next.drivePath ?? item.drivePath,
      skipReason: next.skipReason ?? item.skipReason ?? null, finishedAt: finished ? item.finishedAt ?? now : null,
    };
  });
  if (!changed) return state;
  const failedNow = items.some((item) => item.state === "failed");
  return { ...state, items, dismissed: failedNow ? false : state.dismissed };
}

/** What a finished upload shows while the engine has not listed the file yet. */
export type HandoffStatus = "uploading" | "retained" | "waiting_engine" | "needs_sync" | "handed_over" | "gone";

export function handoffStatus(item: UploadItem, engineHasFile: boolean, now: number): HandoffStatus {
  if (item.state === "failed" || item.state === "cancelled" || item.state === "skipped") return "gone";
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
export function createUploadQueue({ api, now = () => Date.now() }: { api: Pick<SuperDriveApi, "pathsForFiles" | "startUpload" | "cancelUpload" | "subscribePushes"> & Partial<Pick<SuperDriveApi, "discardUpload">>; now?: () => number }) {
  let state = initialUploadQueueState();
  const listeners = new Set<() => void>();
  let unsubscribe: (() => void) | null = null;
  const set = (next: UploadQueueState) => { if (next !== state) { state = next; for (const listener of [...listeners]) listener(); } };

  /** A push for a batch whose start answer has not arrived yet: kept, then applied in order once the batch is known. */
  const early = new Map<string, UploadProgressPush[]>();

  function ensurePushes() {
    if (unsubscribe) return;
    unsubscribe = api.subscribePushes((push: SuperOntologyPush) => {
      if (push.channel !== "upload-progress") return;
      if (!state.items.some((item) => item.batchId === push.payload.batchId)) {
        const kept = early.get(push.payload.batchId) ?? [];
        kept.push(push.payload);
        early.delete(push.payload.batchId);
        early.set(push.payload.batchId, kept);
        while (early.size > EARLY_PUSH_BATCHES) early.delete(early.keys().next().value as string);
        return;
      }
      set(applyUploadProgress(state, push.payload, now()));
    });
  }

  async function startPaths(driveId: string, folder: string, paths: string[], unreadable: number): Promise<DropOutcome> {
    const none = (error: SuperDriveError | null): DropOutcome => ({ queued: 0, skipped: { ...emptySkipped(), unreadable }, roots: [], emptyFolders: 0, batchIds: [], error });
    const result = await api.startUpload({ driveId, folder, paths });
    if (!result.ok) { const error = toSuperDriveError(result); set({ ...state, error, dismissed: false }); return none(error); }
    const { batchId, items, skipped, skippedSamples, roots, emptyFolders, unreadable: refused } = result.value;
    const added: UploadItem[] = items.map((item) => {
      const sub = item.relative.includes("/") ? item.relative.slice(0, item.relative.lastIndexOf("/")) : "";
      return {
        id: item.itemId, batchId, driveId, name: item.name, relative: item.relative, source: item.source, size: item.size,
        folder: [folder, sub].filter(Boolean).join("/"), state: "waiting" as const, bytesDone: 0, code: null, drivePath: null,
        finishedAt: null, overLimit: isOverLimit(item.name, item.size), skipReason: null,
      };
    });
    const dropSkipped: SkippedCounts = { hidden: skipped.hidden, links: skipped.links, junk: skipped.junk ?? 0, temporary: skipped.temporary ?? 0, special: skipped.special ?? 0, unreadable: unreadable + refused };
    const total = emptySkipped();
    for (const key of Object.keys(total) as (keyof SkippedCounts)[]) total[key] = state.skipped[key] + dropSkipped[key];
    set({
      items: [...state.items.filter((item) => !(added.some((entry) => entry.source === item.source) && item.state === "failed")), ...added],
      skipped: total, skippedSamples: [...state.skippedSamples, ...(skippedSamples ?? [])].slice(0, 12),
      error: null, dismissed: false,
    });
    // What was pushed while the answer was on its way.
    for (const push of early.get(batchId) ?? []) set(applyUploadProgress(state, push, now()));
    early.delete(batchId);
    return { queued: added.length, skipped: dropSkipped, roots: roots ?? [], emptyFolders: emptyFolders ?? 0, batchIds: [batchId], error: null };
  }

  /** The paths in requests of at most UPLOAD_PATHS_PER_REQUEST: a drop of many top-level items is several batches, started one after the other. */
  async function startAll(driveId: string, folder: string, paths: string[], unreadable: number): Promise<DropOutcome> {
    const outcome: DropOutcome = { queued: 0, skipped: emptySkipped(), roots: [], emptyFolders: 0, batchIds: [], error: null };
    if (paths.length === 0) {
      outcome.skipped.unreadable = unreadable;
      set({ ...state, skipped: { ...state.skipped, unreadable: state.skipped.unreadable + unreadable }, dismissed: false });
      return outcome;
    }
    for (let from = 0; from < paths.length; from += UPLOAD_PATHS_PER_REQUEST) {
      const part = await startPaths(driveId, folder, paths.slice(from, from + UPLOAD_PATHS_PER_REQUEST), from === 0 ? unreadable : 0);
      outcome.queued += part.queued;
      for (const key of Object.keys(outcome.skipped) as (keyof SkippedCounts)[]) outcome.skipped[key] += part.skipped[key];
      outcome.roots.push(...part.roots);
      outcome.emptyFolders += part.emptyFolders;
      outcome.batchIds.push(...part.batchIds);
      if (part.error) { outcome.error = part.error; break; }
    }
    return outcome;
  }

  return Object.freeze({
    getState: () => state,
    subscribe(listener: () => void) { listeners.add(listener); ensurePushes(); return () => { listeners.delete(listener); }; },

    /** Starts uploading dropped or picked files (and folders: the main process walks them) into `folder` of `driveId`. */
    async enqueueFiles(driveId: string, folder: string, files: ArrayLike<File>): Promise<DropOutcome> {
      const { paths, unreadable } = api.pathsForFiles(files);
      const normal = normalizeDropPaths(paths);
      return startAll(driveId, folder, normal.paths, unreadable + normal.rejected);
    },

    async cancel(itemId: string) {
      const item = state.items.find((entry) => entry.id === itemId);
      if (item) await api.cancelUpload({ batchId: item.batchId, itemId });
    },
    /** Takes back what a drop copied of `folderPath` (the folder is kept in sync as a source instead); how many copies were removed. */
    async discard(batchIds: readonly string[], folderPath: string): Promise<number> {
      let removed = 0;
      for (const batchId of batchIds) {
        const result = await api.discardUpload?.({ batchId, folderPath });
        if (result?.ok) removed += result.value.removed;
      }
      return removed;
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
    /** Closes the list: finished items and the skipped counts go, work still running stays (and reopens it on the next news). */
    dismiss() {
      const items = state.items.filter(isUploadActive);
      set({ ...state, items, skipped: emptySkipped(), skippedSamples: [], dismissed: true });
    },
    clearError() { set({ ...state, error: null }); },
    /** Drops finished items the engine has taken over. */
    prune(handedOver: (item: UploadItem) => boolean) {
      const items = state.items.filter((item) => !(item.state === "done" && handedOver(item)) && item.state !== "cancelled");
      if (items.length !== state.items.length) set({ ...state, items });
    },
    dispose() { unsubscribe?.(); unsubscribe = null; listeners.clear(); },
  });
}
