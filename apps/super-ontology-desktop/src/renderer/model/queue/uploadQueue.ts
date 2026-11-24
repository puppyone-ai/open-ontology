import type { SuperOntologyPush, UploadBatchSnapshot, UploadLargeRoot, UploadProgressPush, UploadSkipReason } from "../../../shared/super-ontology/bridge";
import type { SuperDriveApi } from "../superDriveApi";
import { toSuperDriveError, type SuperDriveError } from "../superDriveErrors";
import { normalizeDropPaths, UPLOAD_PATHS_PER_REQUEST } from "./dropPaths";

/** D31 / 10 §4.11: above these sizes a file is stored but not read. */
export const UPLOAD_SIZE_LIMIT_BYTES = 200 * 1024 * 1024;
export const UPLOAD_TEXT_LIMIT_BYTES = 64 * 1024 * 1024;
/** A finished upload stays this long, then counts as processing (20-frontend §3.3). */
export const DONE_RETAIN_MS = 5_000;
/** `fs_poll_seconds`: the engine should have found a copied file by then, otherwise "Sync now" is offered. */
export const HANDOFF_PATIENCE_MS = 30_000;

/** `skipped`: the same file was already there (the same drop twice), so nothing was copied. */
export type UploadItemState = "waiting" | "copying" | "done" | "failed" | "cancelled" | "skipped";
export { UPLOAD_PATHS_PER_REQUEST };
/** Pushes that arrive before their start answer are kept (up to this many batches) and applied when it does. */
const EARLY_PUSH_BATCHES = 8;
/** Finished rows kept in the list (the batch counts stay exact): a drop of tens of thousands of files is a summary and the work in flight. */
export const SETTLED_ROWS_KEPT = 200;

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
export type SkippedCounts = { hidden: number; links: number; junk: number; temporary: number; special: number; unreadable: number; gitignore: number; ooignore: number };
export type SkippedSample = { name: string; reason: UploadSkipReason };
export type BatchPhase = UploadBatchSnapshot["phase"];

/** One drop request as the main process reports it: walked (found so far), maybe waiting for a choice on big folders, copied (exact counts). */
export type BatchInfo = {
  id: string;
  driveId: string;
  folder: string;
  phase: BatchPhase;
  found: number;
  bytes: number;
  queued: number;
  counts: UploadBatchSnapshot["counts"];
  skipped: SkippedCounts;
  samples: SkippedSample[];
  roots: UploadBatchSnapshot["roots"];
  large: UploadLargeRoot[];
  emptyFolders: number;
  /** A problem of the whole batch: not enough free disk space (needed and free bytes), or a walk that failed. */
  problem: NonNullable<UploadBatchSnapshot["error"]> | null;
};

/** What `enqueueFiles` answers: the batches that were started (their progress is in the queue state) or why none could be. */
export type DropOutcome = { batchIds: string[]; error: SuperDriveError | null };

export type UploadQueueState = {
  items: UploadItem[];
  batches: BatchInfo[];
  /** Summed over the batches listed (and what had no path). */
  skipped: SkippedCounts;
  skippedSamples: SkippedSample[];
  /** A start request that failed as a whole (no engine, not an fs Drive, ...). */
  error: SuperDriveError | null;
  /** The panel was closed by the user; reopens on the next upload or failure. */
  dismissed: boolean;
  /** Dropped files that carried no path (a browser drag): counted as unreadable. */
  looseUnreadable: number;
};

export const emptySkipped = (): SkippedCounts => ({ hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, unreadable: 0, gitignore: 0, ooignore: 0 });
export const initialUploadQueueState = (): UploadQueueState => ({ items: [], batches: [], skipped: emptySkipped(), skippedSamples: [], error: null, dismissed: false, looseUnreadable: 0 });

export function isOverLimit(name: string, size: number): boolean {
  const text = /\.(md|markdown|txt)$/iu.test(name);
  return size > (text ? UPLOAD_TEXT_LIMIT_BYTES : UPLOAD_SIZE_LIMIT_BYTES);
}

export const isUploadActive = (item: UploadItem): boolean => item.state === "waiting" || item.state === "copying";

const SETTLED_STATES = new Set<UploadItemState>(["done", "skipped", "cancelled"]);
const settledOf = (batch: BatchInfo): number => batch.counts.done + batch.counts.failed + batch.counts.skipped + batch.counts.cancelled;

/** Files of a batch still to copy (released and not settled), or, while the walk goes on, the files found so far. */
export const addingOf = (batch: BatchInfo): number => (batch.phase === "scanning" ? Math.max(batch.found, batch.queued - settledOf(batch)) : Math.max(0, batch.queued - settledOf(batch)));

function deriveSkipped(batches: readonly BatchInfo[], loose: number): { skipped: SkippedCounts; skippedSamples: SkippedSample[] } {
  const skipped = emptySkipped();
  const samples: SkippedSample[] = [];
  for (const batch of batches) {
    for (const key of Object.keys(skipped) as (keyof SkippedCounts)[]) skipped[key] += batch.skipped[key];
    for (const sample of batch.samples) if (samples.length < 12) samples.push(sample);
  }
  skipped.unreadable += loose;
  return { skipped, skippedSamples: samples };
}

const withDerived = (state: UploadQueueState): UploadQueueState => ({ ...state, ...deriveSkipped(state.batches, state.looseUnreadable) });

const newBatch = (id: string, driveId: string, folder: string): BatchInfo => ({
  id, driveId, folder, phase: "scanning", found: 0, bytes: 0, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 },
  skipped: emptySkipped(), samples: [], roots: [], large: [], emptyFolders: 0, problem: null,
});

function mergeSnapshot(batch: BatchInfo, snapshot: UploadBatchSnapshot): BatchInfo {
  return {
    ...batch, phase: snapshot.phase, found: snapshot.found, bytes: snapshot.bytes, queued: snapshot.queued, counts: snapshot.counts,
    skipped: { ...emptySkipped(), ...snapshot.skipped, unreadable: snapshot.unreadable }, samples: snapshot.skippedSamples, roots: snapshot.roots,
    large: snapshot.large, emptyFolders: snapshot.emptyFolders, problem: snapshot.error ?? null,
  };
}

/** Keeps the list short: finished rows beyond the newest {@link SETTLED_ROWS_KEPT} go (failed ones and work in flight stay). */
function trimSettled(items: UploadItem[]): UploadItem[] {
  let settled = 0;
  for (const item of items) if (SETTLED_STATES.has(item.state)) settled += 1;
  if (settled <= SETTLED_ROWS_KEPT) return items;
  let drop = settled - SETTLED_ROWS_KEPT;
  return items.filter((item) => { if (drop > 0 && SETTLED_STATES.has(item.state)) { drop -= 1; return false; } return true; });
}

/** Applies one `upload-progress` push: the batch's counts, the rows it released, the item changes; unknown batches are ignored. */
export function applyUploadProgress(state: UploadQueueState, push: UploadProgressPush, now: number): UploadQueueState {
  const known = state.batches.find((batch) => batch.id === push.batchId);
  if (!known) return state;
  let batches = state.batches;
  if (push.batch) batches = batches.map((batch) => (batch.id === push.batchId ? mergeSnapshot(batch, push.batch as UploadBatchSnapshot) : batch));
  let items = state.items;
  const existing = new Set(items.filter((item) => item.batchId === push.batchId).map((item) => item.id));
  const fresh = (push.added ?? []).filter((row) => !existing.has(row.itemId));
  if (fresh.length > 0) {
    items = [...items, ...fresh.map((row): UploadItem => {
      const sub = row.relative.includes("/") ? row.relative.slice(0, row.relative.lastIndexOf("/")) : "";
      return {
        id: row.itemId, batchId: push.batchId, driveId: known.driveId, name: row.name, relative: row.relative, source: row.source, size: row.size,
        folder: [known.folder, sub].filter(Boolean).join("/"), state: "waiting", bytesDone: 0, code: null, drivePath: null, finishedAt: null,
        overLimit: isOverLimit(row.name, row.size), skipReason: null,
      };
    })];
  }
  const reported = new Map(push.items.map((item) => [item.itemId, item]));
  let changed = fresh.length > 0 || batches !== state.batches;
  if (reported.size > 0) {
    items = items.map((item) => {
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
  }
  if (!changed) return state;
  const failedNow = items.some((item) => item.state === "failed") || batches.some((batch) => batch.problem !== null);
  return withDerived({ ...state, items: trimSettled(items), batches, dismissed: failedNow ? false : state.dismissed });
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

/** Where a drop stands, over the batches it started (the toast reads it). */
export type DropSummary = {
  phase: "preparing" | "choice" | "copying" | "done" | "cancelled";
  /** Files found so far by the walk, files handed to the copy, files settled, failed. */
  found: number;
  queued: number;
  settled: number;
  done: number;
  failed: number;
  bytes: number;
  skipped: number;
  /** Folders that wait for a choice. */
  pending: (UploadLargeRoot & { batchId: string })[];
  /** Folders of the drop that were copied (rows in My files). */
  folders: { path: string; name: string; files: number }[];
  problem: BatchInfo["problem"];
};

export function summarizeDrop(batches: readonly BatchInfo[], ids: readonly string[]): DropSummary {
  const own = ids.map((id) => batches.find((batch) => batch.id === id)).filter((batch): batch is BatchInfo => batch !== undefined);
  const summary: DropSummary = { phase: "done", found: 0, queued: 0, settled: 0, done: 0, failed: 0, bytes: 0, skipped: 0, pending: [], folders: [], problem: null };
  let scanning = false;
  let copying = false;
  let cancelled = own.length > 0;
  for (const batch of own) {
    summary.found += batch.found; summary.queued += batch.queued; summary.settled += settledOf(batch); summary.done += batch.counts.done;
    summary.failed += batch.counts.failed; summary.bytes += batch.bytes;
    summary.skipped += Object.values(batch.skipped).reduce((sum, value) => sum + value, 0);
    summary.pending.push(...batch.large.filter((root) => root.state === "pending").map((root) => ({ ...root, batchId: batch.id })));
    summary.folders.push(...batch.roots.filter((root) => root.kind === "directory").map(({ path, name, files }) => ({ path, name, files })));
    summary.problem ??= batch.problem;
    if (batch.phase === "scanning") scanning = true;
    if (batch.phase === "copying" || batch.large.some((root) => root.state === "copying")) copying = true;
    if (batch.phase !== "cancelled") cancelled = false;
  }
  summary.phase = own.length < ids.length || scanning ? "preparing" : summary.pending.length > 0 ? "choice" : copying ? "copying" : cancelled ? "cancelled" : "done";
  return summary;
}

export type UploadQueue = ReturnType<typeof createUploadQueue>;

type UploadApi = Pick<SuperDriveApi, "pathsForFiles" | "startUpload" | "cancelUpload" | "subscribePushes"> & Partial<Pick<SuperDriveApi, "discardUpload" | "decideUpload">>;

/**
 * Client side of the upload queue (20-frontend §3.3): turns a drop into upload batches in the main process and folds their progress pushes
 * (files found so far, the rows as they are released, the copy per item, the batch's exact counts) into a small state the tray, the toast and
 * the Sources list read. A start request answers at once; nothing here waits for a walk. Framework independent.
 */
export function createUploadQueue({ api, now = () => Date.now() }: { api: UploadApi; now?: () => number }) {
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
      if (!state.batches.some((batch) => batch.id === push.payload.batchId)) {
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

  async function startPaths(driveId: string, folder: string, paths: string[]): Promise<DropOutcome> {
    ensurePushes();
    const result = await api.startUpload({ driveId, folder, paths });
    if (!result.ok) { const error = toSuperDriveError(result); set({ ...state, error, dismissed: false }); return { batchIds: [], error }; }
    const { batchId } = result.value;
    set(withDerived({ ...state, batches: [...state.batches, newBatch(batchId, driveId, folder)], error: null, dismissed: false }));
    // What was pushed while the answer was on its way.
    for (const push of early.get(batchId) ?? []) set(applyUploadProgress(state, push, now()));
    early.delete(batchId);
    return { batchIds: [batchId], error: null };
  }

  /** The paths in requests of at most UPLOAD_PATHS_PER_REQUEST: a drop of many top-level items is several batches (each answers at once). */
  async function startAll(driveId: string, folder: string, paths: string[], unreadable: number): Promise<DropOutcome> {
    const outcome: DropOutcome = { batchIds: [], error: null };
    if (unreadable > 0) set(withDerived({ ...state, looseUnreadable: state.looseUnreadable + unreadable, dismissed: false }));
    for (let from = 0; from < paths.length; from += UPLOAD_PATHS_PER_REQUEST) {
      const part = await startPaths(driveId, folder, paths.slice(from, from + UPLOAD_PATHS_PER_REQUEST));
      outcome.batchIds.push(...part.batchIds);
      if (part.error) { outcome.error = part.error; break; }
    }
    return outcome;
  }

  return Object.freeze({
    getState: () => state,
    subscribe(listener: () => void) { listeners.add(listener); ensurePushes(); return () => { listeners.delete(listener); }; },

    /** Starts uploading dropped or picked files (and folders: the main process walks them) into `folder` of `driveId`. Answers once the batches exist. */
    async enqueueFiles(driveId: string, folder: string, files: ArrayLike<File>): Promise<DropOutcome> {
      const { paths, unreadable } = api.pathsForFiles(files);
      const normal = normalizeDropPaths(paths);
      return startAll(driveId, folder, normal.paths, unreadable + normal.rejected);
    },

    /** The answer for the big folders a batch found: "copy" streams them into the Drive, "skip" leaves them (they were linked as sources). */
    async decide(batchId: string, choice: "copy" | "skip", paths?: string[]) {
      const result = await api.decideUpload?.({ batchId, choice, ...(paths ? { paths } : {}) });
      if (result && !result.ok) set({ ...state, error: toSuperDriveError(result), dismissed: false });
      return result?.ok ?? false;
    },

    async cancel(itemId: string) {
      const item = state.items.find((entry) => entry.id === itemId);
      if (item) await api.cancelUpload({ batchId: item.batchId, itemId });
    },
    /** Stops a whole drop: the walk, the copies in flight and what is waiting. */
    async cancelBatches(batchIds: readonly string[]) {
      for (const batchId of batchIds) await api.cancelUpload({ batchId });
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
      const active = new Set(state.items.filter(isUploadActive).map((item) => item.batchId));
      for (const batch of state.batches) if (batch.phase === "scanning" || batch.phase === "copying" || batch.phase === "choice") active.add(batch.id);
      for (const batchId of active) await api.cancelUpload({ batchId });
    },
    /** Uploads the failed item's file again into the same folder. */
    async retry(itemId: string) {
      const item = state.items.find((entry) => entry.id === itemId);
      if (!item || item.state !== "failed") return;
      await startPaths(item.driveId, item.folder, [item.source]);
    },
    /** Closes the list: finished items and the skipped counts go, work still running stays (and reopens it on the next news). */
    dismiss() {
      const items = state.items.filter(isUploadActive);
      const live = state.batches.filter((batch) => batch.phase === "scanning" || batch.phase === "copying" || batch.phase === "choice");
      set(withDerived({ ...state, items, batches: live, looseUnreadable: 0, dismissed: true }));
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
