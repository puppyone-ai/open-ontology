import type { Drive, FileSummary, Share } from "./contract/docsApi.generated";
import { baseName, parentPath, parseFolderKey, type FolderKey } from "./superDriveRoute";
import type { EngineEvent, FileRecord, FolderPage, SuperDriveState } from "./superDriveState";

/**
 * Folds engine events into the renderer state (20-frontend §1.7.3). It is a
 * pure function over immutable state: arrays and maps are copied on first
 * write inside one call, so applying 10,000 events costs one copy of each
 * touched collection, not one per event.
 */
type Draft = {
  state: SuperDriveState;
  copied: { files: boolean; drives: boolean; folders: boolean; shares: boolean; review: boolean; usage: boolean; ontology: boolean };
  copiedFolders: Set<FolderKey>;
};

function startDraft(state: SuperDriveState): Draft {
  return {
    state: { ...state },
    copied: { files: false, drives: false, folders: false, shares: false, review: false, usage: false, ontology: false },
    copiedFolders: new Set(),
  };
}

const writableFiles = (draft: Draft) => {
  if (!draft.copied.files) { draft.state.files = { ...draft.state.files }; draft.copied.files = true; }
  return draft.state.files;
};
const writableFolders = (draft: Draft) => {
  if (!draft.copied.folders) { draft.state.folders = { ...draft.state.folders }; draft.copied.folders = true; }
  return draft.state.folders;
};
function writableFolder(draft: Draft, key: FolderKey): FolderPage {
  const folders = writableFolders(draft);
  if (!draft.copiedFolders.has(key)) {
    folders[key] = { ...folders[key], ids: [...folders[key].ids] };
    draft.copiedFolders.add(key);
  }
  return folders[key];
}
const writableDrives = (draft: Draft) => {
  if (!draft.copied.drives) {
    draft.state.drives = { ...draft.state.drives, byId: { ...draft.state.drives.byId }, order: [...draft.state.drives.order] };
    draft.copied.drives = true;
  }
  return draft.state.drives;
};
const writableShares = (draft: Draft) => {
  if (!draft.copied.shares) {
    draft.state.shares = { ...draft.state.shares, byId: { ...draft.state.shares.byId }, order: [...draft.state.shares.order], chains: { ...draft.state.shares.chains }, updates: { ...draft.state.shares.updates }, accessed: { ...draft.state.shares.accessed } };
    draft.copied.shares = true;
  }
  return draft.state.shares;
};

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const asString = (value: unknown): string | null => (typeof value === "string" ? value : null);
const asNumber = (value: unknown): number | null => (typeof value === "number" && Number.isFinite(value) ? value : null);

/** Every cached detail document of a file (the base one and the one with entities, facts and shares). */
export const FILE_EXTRAS_SUFFIX = "#x";
function dropFileDetails(state: SuperDriveState, fileId: string) {
  state.details.delete(fileId);
  state.details.delete(`${fileId}${FILE_EXTRAS_SUFFIX}`);
}

/** Folders of a Drive that list `path` unfiltered, in the default order (path); only those take live inserts. */
function defaultFolderKey(driveId: string, path: string): FolderKey {
  return `${driveId}\u0000${path}\u0000`;
}

function comparePaths(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function insertSorted(draft: Draft, folder: FolderPage, id: string, path: string) {
  const files = draft.state.files;
  let low = 0;
  let high = folder.ids.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (comparePaths(files[folder.ids[middle]]?.path ?? "", path) < 0) low = middle + 1; else high = middle;
  }
  folder.ids.splice(low, 0, id);
}

function adjustDriveCounts(draft: Draft, driveId: string, from: string | null, to: string | null) {
  const current = draft.state.drives.byId[driveId];
  if (!current || from === to) return;
  const drives = writableDrives(draft);
  const counts: Record<string, unknown> = { ...(current.counts ?? {}) };
  if (from) counts[from] = Math.max(0, (asNumber(counts[from]) ?? 0) - 1);
  if (to) counts[to] = (asNumber(counts[to]) ?? 0) + 1;
  drives.byId[driveId] = { ...current, counts };
}

function removeFromFolders(draft: Draft, fileId: string) {
  for (const key of Object.keys(draft.state.folders)) {
    const index = draft.state.folders[key].ids.indexOf(fileId);
    if (index !== -1) writableFolder(draft, key).ids.splice(index, 1);
  }
}

function onFileStateChanged(draft: Draft, event: EngineEvent, seq: number) {
  const incoming = event.data.file as FileSummary | undefined;
  if (!isRecord(incoming) || typeof incoming.id !== "string" || typeof incoming.path !== "string") return;
  const existing = draft.state.files[incoming.id];
  if (existing && seq <= existing.seq) return; // Older than what we already show.
  dropFileDetails(draft.state, incoming.id);
  const files = writableFiles(draft);
  const record: FileRecord = { ...incoming, seq };
  files[incoming.id] = record;
  adjustDriveCounts(draft, incoming.drive_id, existing?.state ?? null, incoming.state);
  if (existing) {
    if (existing.path !== incoming.path) { moveFile(draft, existing.id, existing.path, incoming.path); }
    return;
  }
  // A file we have not seen: live-insert into the unfiltered folder that lists its parent, if one is loaded.
  const key = defaultFolderKey(incoming.drive_id, parentPath(incoming.path));
  // A file with no record yet cannot be in any listing, so no membership scan is needed (it would be O(n) per event).
  if (draft.state.folders[key]) {
    insertSorted(draft, writableFolder(draft, key), incoming.id, incoming.path);
  }
  // A file in a folder that is new to a loaded listing adds a subfolder row there: refetch the ancestors whose folder rows lack it.
  for (let dirPath = parentPath(incoming.path); dirPath !== ""; dirPath = parentPath(dirPath)) {
    const holderKey = defaultFolderKey(incoming.drive_id, parentPath(dirPath));
    const holder = draft.state.folders[holderKey];
    if (holder && !holder.stale && !holder.dirs.some((dir) => dir.path === dirPath)) writableFolder(draft, holderKey).stale = true;
  }
  // Filtered or differently sorted listings cannot be patched safely: mark them for a refetch.
  for (const other of Object.keys(draft.state.folders)) {
    const parsed = parseFolderKey(other);
    if (parsed.driveId === incoming.drive_id && parsed.filterHash !== "" && !draft.state.folders[other].stale) {
      writableFolder(draft, other).stale = true;
    }
  }
}

function moveFile(draft: Draft, fileId: string, oldPath: string, newPath: string) {
  const files = draft.state.files;
  const record = files[fileId];
  if (!record) return;
  const driveId = record.drive_id;
  const oldKey = defaultFolderKey(driveId, parentPath(oldPath));
  const newKey = defaultFolderKey(driveId, parentPath(newPath));
  if (draft.state.folders[oldKey]) {
    const index = draft.state.folders[oldKey].ids.indexOf(fileId);
    if (index !== -1) writableFolder(draft, oldKey).ids.splice(index, 1);
  }
  if (draft.state.folders[newKey] && !draft.state.folders[newKey].ids.includes(fileId)) {
    insertSorted(draft, writableFolder(draft, newKey), fileId, newPath);
  }
}

function onFileRenamed(draft: Draft, event: EngineEvent, seq: number) {
  const fileId = asString(event.data.file_id);
  const newPath = asString(event.data.new_path);
  const oldPath = asString(event.data.old_path);
  if (!fileId || !newPath || !oldPath) return;
  const existing = draft.state.files[fileId];
  if (!existing || seq <= existing.seq) return;
  writableFiles(draft)[fileId] = { ...existing, path: newPath, name: baseName(newPath), seq };
  moveFile(draft, fileId, oldPath, newPath);
  // The cached detail still shows the old path.
  draft.state.details.delete(fileId);
}

function onFileRemoved(draft: Draft, event: EngineEvent) {
  const fileId = asString(event.data.file_id);
  if (!fileId || !draft.state.files[fileId]) return;
  const existing = draft.state.files[fileId];
  dropFileDetails(draft.state, fileId);
  delete writableFiles(draft)[fileId];
  removeFromFolders(draft, fileId);
  // A pane open on a file that is gone would only show an error.
  if (draft.state.route.detailsFileId === fileId) draft.state.route = { ...draft.state.route, detailsFileId: null };
  adjustDriveCounts(draft, existing.drive_id, existing.state, null);
  if (draft.state.selection.ids.has(fileId)) {
    const ids = new Set(draft.state.selection.ids);
    ids.delete(fileId);
    draft.state.selection = { ...draft.state.selection, ids, anchorId: draft.state.selection.anchorId === fileId ? null : draft.state.selection.anchorId };
  }
}

function onBulkChanged(draft: Draft, event: EngineEvent) {
  const driveId = asString(event.data.drive_id) ?? asString(event.drive_id);
  if (!driveId) return;
  for (const key of Object.keys(draft.state.folders)) {
    if (parseFolderKey(key).driveId === driveId && !draft.state.folders[key].stale) writableFolder(draft, key).stale = true;
  }
}

function onDrive(draft: Draft, event: EngineEvent) {
  if (event.type === "drive.deleted") {
    const driveId = asString(event.data.drive_id) ?? asString(isRecord(event.data.drive) ? event.data.drive.id : null);
    if (!driveId || !draft.state.drives.byId[driveId]) return;
    const drives = writableDrives(draft);
    delete drives.byId[driveId];
    drives.order = drives.order.filter((id) => id !== driveId);
    return;
  }
  const drive = event.data.drive as Drive | undefined;
  if (!isRecord(drive) || typeof drive.id !== "string") return;
  const drives = writableDrives(draft);
  const previous = drives.byId[drive.id];
  drives.byId[drive.id] = previous ? { ...previous, ...drive } : drive;
  if (!previous) drives.order.push(drive.id);
}

function onSync(draft: Draft, event: EngineEvent) {
  const driveId = asString(event.data.drive_id) ?? asString(event.drive_id);
  const sync = event.data.sync;
  const current = driveId ? draft.state.drives.byId[driveId] : undefined;
  if (!driveId || !current || !isRecord(sync)) return;
  writableDrives(draft).byId[driveId] = { ...current, last_sync: sync as Drive["last_sync"] };
}

function onFreshness(draft: Draft, event: EngineEvent) {
  const driveId = asString(event.data.drive_id) ?? asString(event.drive_id);
  const freshness = isRecord(event.data.freshness) ? event.data.freshness : event.data;
  const current = driveId ? draft.state.drives.byId[driveId] : undefined;
  if (!driveId || !current || typeof freshness.state !== "string") return;
  // The freshness carries the Drive's pending review total: items that arrive with new facts (no review.changed is sent for them)
  // show in the sidebar count, and a queue that was read before is read again.
  const pending = asNumber(freshness.pending_review);
  const counts = draft.state.review.pendingByDrive[driveId];
  const known = counts ? Object.values(counts).reduce((sum, value) => sum + value, 0) : current.pending_review ?? 0;
  writableDrives(draft).byId[driveId] = { ...current, freshness: freshness as unknown as Drive["freshness"], ...(pending === null ? {} : { pending_review: pending }) };
  if (pending !== null && pending !== known) {
    if (!draft.copied.review) {
      draft.state.review = { ...draft.state.review, pendingByDrive: { ...draft.state.review.pendingByDrive }, groups: { ...draft.state.review.groups }, lists: { ...draft.state.review.lists } };
      draft.copied.review = true;
    }
    delete draft.state.review.pendingByDrive[driveId];
    const groups = draft.state.review.groups[driveId];
    if (groups && !groups.stale) draft.state.review.groups[driveId] = { ...groups, stale: true };
    for (const [key, list] of Object.entries(draft.state.review.lists)) {
      if (key.startsWith(`${driveId}\u0000`) && !list.stale) draft.state.review.lists[key] = { ...list, stale: true };
    }
  }
}

function onReview(draft: Draft, event: EngineEvent) {
  const driveId = asString(event.data.drive_id) ?? asString(event.drive_id);
  const pending = event.data.pending;
  // `drive_id` null: items that belong to no Drive; no Drive's queue changed.
  if (!driveId || !isRecord(pending)) return;
  const counts: Record<string, number> = {};
  for (const [kind, value] of Object.entries(pending)) counts[kind] = asNumber(value) ?? 0;
  if (!draft.copied.review) {
    draft.state.review = { ...draft.state.review, pendingByDrive: { ...draft.state.review.pendingByDrive }, groups: { ...draft.state.review.groups }, lists: { ...draft.state.review.lists } };
    draft.copied.review = true;
  }
  draft.state.review.pendingByDrive[driveId] = counts;
  // The queue changed after its listings were read: mark them stale so a visible page refetches (never patched in place).
  const groups = draft.state.review.groups[driveId];
  if (groups && !groups.stale) draft.state.review.groups[driveId] = { ...groups, stale: true };
  for (const [key, list] of Object.entries(draft.state.review.lists)) {
    if (key.startsWith(`${driveId}\u0000`) && !list.stale) draft.state.review.lists[key] = { ...list, stale: true };
  }
  const drive = draft.state.drives.byId[driveId];
  if (drive) writableDrives(draft).byId[driveId] = { ...drive, pending_review: Object.values(counts).reduce((sum, value) => sum + value, 0) };
}

function onShare(draft: Draft, event: EngineEvent) {
  if (event.type === "share.mount_removed") {
    const id = asString(event.data.share_id);
    const share = id ? draft.state.shares.byId[id] : undefined;
    if (!id || !share) return;
    const shares = writableShares(draft);
    const mount = isRecord((share as { mount?: unknown }).mount) ? (share as { mount: Record<string, unknown> }).mount : {};
    shares.byId[id] = { ...share, mount: { ...mount, present: false } } as Share;
    return;
  }
  const share = event.data.share as Share | undefined;
  if (!isRecord(share) || typeof share.id !== "string") return;
  const shares = writableShares(draft);
  if (!shares.byId[share.id]) shares.order.unshift(share.id);
  shares.byId[share.id] = share;
  const parent = asString((share as { refreshed_from?: unknown }).refreshed_from);
  if (parent) {
    const root = Object.keys(shares.chains).find((key) => shares.chains[key].includes(parent)) ?? parent;
    const chain = shares.chains[root] ?? [root];
    if (!chain.includes(share.id)) shares.chains[root] = [...chain, share.id];
  }
}

/** `share.update_appended {share_id, seq, level}`: the log of that share is stale and its newest level is known at once. */
function onShareUpdateAppended(draft: Draft, event: EngineEvent) {
  const shareId = asString(event.data.share_id);
  const level = asString(event.data.level);
  if (!shareId) return;
  const shares = writableShares(draft);
  const previous = shares.updates[shareId];
  const known = level === "info" || level === "warn" || level === "block" ? level : previous?.latestLevel ?? null;
  // The newest entry is the one that counts, but a block stays visible until the entry log is read again.
  shares.updates[shareId] = { items: previous?.items ?? [], loaded: previous?.loaded ?? false, stale: true, latestLevel: known };
}

/** `refs.accessed {share_id, count}`: an Agent pulled originals; shown as information only. */
function onRefsAccessed(draft: Draft, event: EngineEvent) {
  const shareId = asString(event.data.share_id);
  const count = asNumber(event.data.count);
  if (!shareId || count === null) return;
  const shares = writableShares(draft);
  shares.accessed[shareId] = (shares.accessed[shareId] ?? 0) + count;
}

/** `ontology.constraint_violated`: the constraint list and the conflict queue changed. */
function onConstraintViolated(draft: Draft) {
  if (!draft.state.constraints.stale) draft.state.constraints = { ...draft.state.constraints, stale: true };
}

function onUsage(draft: Draft, event: EngineEvent) {
  const driveId = asString(event.data.drive_id) ?? asString(event.drive_id);
  if (!driveId) return;
  if (!draft.copied.usage) { draft.state.usage = { ...draft.state.usage }; draft.copied.usage = true; }
  draft.state.usage[driveId] = {
    day: asString(event.data.day) ?? "",
    calls: asNumber(event.data.calls) ?? 0,
    limit: asNumber(event.data.limit),
    limited: event.data.limited === true,
  };
}

function onOntologyVersion(draft: Draft, event: EngineEvent) {
  const version = asNumber(event.data.n);
  const asOf = asString(event.data.as_of) ?? event.at;
  const driveId = asString(event.drive_id) ?? "*";
  if (version === null) return;
  if (!draft.copied.ontology) { draft.state.ontology = { ...draft.state.ontology }; draft.copied.ontology = true; }
  draft.state.ontology[driveId] = { n: version, asOf };
  // The ontology is one history for the engine (the event names no Drive): a Drive entry read earlier moves up too.
  for (const [key, known] of Object.entries(draft.state.ontology)) if (known.n < version) draft.state.ontology[key] = { n: version, asOf };
  if (draft.state.review.publishing[driveId] || draft.state.review.publishing["*"]) {
    draft.state.review = { ...draft.state.review, publishing: { ...draft.state.review.publishing, [driveId]: false, "*": false } };
  }
  // The version is also what an entity cache is keyed on: all entities are stale after a publish.
  draft.state.entities.clear();
}

const PROGRESS_UNITS = ["page", "sheet", "line", "batch"];

function onProgress(draft: Draft, event: EngineEvent) {
  const fileId = asString(event.data.file_id);
  const record = fileId ? draft.state.files[fileId] : undefined;
  const unit = asString(event.data.unit);
  if (!fileId || !record || !unit || !PROGRESS_UNITS.includes(unit)) return;
  writableFiles(draft)[fileId] = {
    ...record,
    progress: { unit: unit as "page" | "sheet" | "line" | "batch", done: asNumber(event.data.done) ?? 0, total: asNumber(event.data.total) },
  };
}

/** Event types the store reacts to; anything else is ignored on purpose (newer engines may send more). */
export function applyEvents(state: SuperDriveState, events: readonly EngineEvent[]): SuperDriveState {
  if (events.length === 0) return state;
  const draft = startDraft(state);
  let appliedSeq = state.cursor.appliedSeq;
  for (const event of events) {
    const seq = Number.isSafeInteger(event.seq) ? (event.seq as number) : null;
    if (seq !== null && seq > appliedSeq) appliedSeq = seq;
    switch (event.type) {
      case "file.progress": onProgress(draft, event); break; // Transient: no seq, no cursor.
      case "file.state_changed": if (seq !== null) onFileStateChanged(draft, event, seq); break;
      case "file.renamed": if (seq !== null) onFileRenamed(draft, event, seq); break;
      case "file.removed": onFileRemoved(draft, event); break;
      case "files.bulk_changed": onBulkChanged(draft, event); break;
      case "drive.created": case "drive.updated": case "drive.deleted": onDrive(draft, event); break;
      case "drive.sync_started": case "drive.sync_completed": case "drive.sync_failed": case "drive.sync_held": onSync(draft, event); break;
      case "drive.freshness_changed": onFreshness(draft, event); break;
      case "review.changed": onReview(draft, event); break;
      case "share.created": case "share.state_changed": case "share.mount_removed": onShare(draft, event); break;
      case "share.update_appended": onShareUpdateAppended(draft, event); break;
      case "refs.accessed": onRefsAccessed(draft, event); break;
      case "ontology.constraint_violated": onConstraintViolated(draft); break;
      case "usage.changed": onUsage(draft, event); break;
      case "ontology.version_published": onOntologyVersion(draft, event); break;
      default: break;
    }
  }
  draft.state.cursor = { appliedSeq };
  return draft.state;
}
