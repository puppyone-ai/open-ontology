import type { ConnectionSnapshot, EngineEvent, EngineFileDetail, EngineSnapshot, SuperOntologyPush } from "../../shared/super-ontology/bridge";
import type { Drive, FileSummary } from "../../shared/super-ontology/docs-api.generated";
import { createOperationId } from "./operationIds";
import { sourceSeconds } from "./details/mediaModel";
import { toSuperDriveError, type SuperDriveError } from "./superDriveErrors";
import { applyEvents, FILE_EXTRAS_SUFFIX } from "./superDriveEvents";
import {
  folderKey,
  navigateToFolder,
  parseFolderKey,
  selectDriveRoute,
  type FolderFilter,
  type FolderKey,
  type SuperDriveRoute,
} from "./superDriveRoute";
import { createEmptyFolderPage, createInitialState, type FolderPage, type Notice, type SuperDriveState } from "./superDriveState";
import type { ApiResult, DriveCreateInput, SuperDriveApi } from "./superDriveApi";
import { createOntologyActions } from "./superDriveOntologyStore";

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

export const FOLDER_PAGE_LIMIT = 500;
/** More events than this in one push, or any events queued behind such a push, are folded in slices (F-S13-01). */
export const FLOOD_THRESHOLD = 500;
/** Events folded per turn while a flood is being worked off; the list renders once per slice instead of once per push. */
export const FLOOD_SLICE = 1_000;
const FLOOD_PAUSE_MS = 16;
const FLOOD_QUIET_MS = 400;
const LIVE_ENGINE_STATUSES = ["healthy", "degraded"];

export type Scheduler = {
  setTimeout: (callback: () => void, ms: number) => unknown;
  clearTimeout: (handle: unknown) => void;
  now: () => number;
};

export type ActionResult<T = void> = { ok: true; value: T } | { ok: false; error: SuperDriveError };

export type DriveEdit = {
  name?: string;
  paused?: boolean;
  modelEnabled?: boolean;
  llmDailyCalls?: number;
  exclude?: string[];
  autoPublishBp?: number;
};

export type SuperDriveStore = ReturnType<typeof createSuperDriveStore>;

const ok = <T,>(value: T): ActionResult<T> => ({ ok: true, value });

function failureOf(result: Extract<ApiResult<unknown>, { ok: false }>): ActionResult<never> {
  return { ok: false, error: toSuperDriveError(result) };
}

export const isLiveEngine = (engine: EngineSnapshot | null): boolean => engine !== null && LIVE_ENGINE_STATUSES.includes(engine.status);

/**
 * The renderer store (20-frontend §1.7): a read replica of the engine plus
 * event folding, cursor handling and reload-on-gap. Framework independent;
 * React subscribes through `useSyncExternalStore` (see SuperDriveProvider).
 */
export function createSuperDriveStore({
  api,
  scheduler = { setTimeout: (callback, ms) => setTimeout(callback, ms), clearTimeout: (handle) => clearTimeout(handle as number), now: () => Date.now() },
  createId = createOperationId,
}: { api: SuperDriveApi; scheduler?: Scheduler; createId?: (prefix?: string) => string }) {
  let state: SuperDriveState = createInitialState();
  const listeners = new Set<() => void>();
  let stopPushes: (() => void) | null = null;
  let started = false;
  const folderRequests = new Map<FolderKey, number>();
  let requestCounter = 0;
  let resyncPromise: Promise<void> | null = null;
  let catchUp: { count: number; timer: unknown } | null = null;
  let noticeCounter = 0;
  // Events of a flood that are not folded yet, in order, with the cursor of the push that carried the last of them.
  let backlog: EngineEvent[] = [];
  let backlogMeta: { lastSeq: number; instanceId: string | null | undefined } | null = null;
  let backlogTimer: unknown = null;
  let floodQuietTimer: unknown = null;

  const getState = () => state;
  function setState(update: (current: SuperDriveState) => SuperDriveState) {
    const next = update(state);
    if (next === state) return;
    state = next;
    for (const listener of [...listeners]) listener();
  }
  const patch = (partial: Partial<SuperDriveState>) => setState((current) => ({ ...current, ...partial }));

  // ---- engine ----------------------------------------------------------------

  function setEngine(snapshot: EngineSnapshot) {
    const previous = state.engine;
    if (previous && snapshot.revision < previous.revision) return;
    patch({ engine: snapshot });
    const wasLive = isLiveEngine(previous);
    if (isLiveEngine(snapshot) && !wasLive) {
      void refreshAfterEngineLive();
    }
  }

  async function refreshAfterEngineLive() {
    const subscribed = await api.subscribeEvents();
    if (subscribed.ok) applyConnection(subscribed.value.connection, subscribed.value.instanceId);
    if (!state.drives.loaded && !state.drives.loading) await loadDrives();
  }

  async function engineAction(run: () => Promise<ApiResult<EngineSnapshot>>): Promise<ActionResult<EngineSnapshot>> {
    patch({ engineAction: { busy: true, error: null } });
    const result = await run();
    if (!result.ok) {
      const error = toSuperDriveError(result);
      patch({ engineAction: { busy: false, error } });
      return { ok: false, error };
    }
    setEngine(result.value);
    patch({ engineAction: { busy: false, error: null } });
    return ok(result.value);
  }

  // ---- connection and events ---------------------------------------------------

  function applyConnection(connection: ConnectionSnapshot, instanceId?: string | null) {
    setState((current) => ({
      ...current,
      connection: {
        ...current.connection,
        ...connection,
        instanceId: instanceId === undefined ? current.connection.instanceId : instanceId,
        resyncing: connection.state === "resyncing" ? true : current.connection.resyncing,
      },
    }));
  }

  function addNotice(notice: Omit<Notice, "id" | "at">, ttlMs = 3_000) {
    noticeCounter += 1;
    const id = `notice-${noticeCounter}`;
    setState((current) => ({ ...current, notices: [...current.notices, { ...notice, id, at: scheduler.now() }] }));
    scheduler.setTimeout(() => dismissNotice(id), ttlMs);
  }

  function dismissNotice(id: string) {
    setState((current) => (current.notices.some((notice) => notice.id === id)
      ? { ...current, notices: current.notices.filter((notice) => notice.id !== id) }
      : current));
  }

  function beginCatchUp() {
    if (catchUp) scheduler.clearTimeout(catchUp.timer);
    catchUp = { count: 0, timer: null };
    armCatchUp();
  }

  function armCatchUp() {
    if (!catchUp) return;
    scheduler.clearTimeout(catchUp.timer);
    catchUp.timer = scheduler.setTimeout(() => {
      const finished = catchUp;
      catchUp = null;
      if (finished && finished.count > 0) addNotice({ kind: "caught_up", values: { count: finished.count } });
    }, 400);
  }

  function handlePush(push: SuperOntologyPush) {
    switch (push.channel) {
      case "engine-changed": setEngine(push.payload); break;
      case "events": {
        const { events, lastSeq, instanceId } = push.payload;
        if (catchUp) { catchUp.count += events.filter((event) => Number.isSafeInteger(event.seq)).length; armCatchUp(); }
        // The ordinary push is folded at once. A flood (a cold start with thousands of changes) is cut into slices, one per turn:
        // one huge fold would hold the renderer for a hundred milliseconds or more, and the list would render once per push.
        if (backlog.length === 0 && state.flood !== true && events.length <= FLOOD_THRESHOLD) { foldEvents(events, lastSeq, instanceId); break; }
        backlog.push(...events);
        backlogMeta = { lastSeq, instanceId };
        setFlood(true);
        drainBacklog();
        break;
      }
      case "connection-changed": {
        const previous = state.connection.state;
        applyConnection(push.payload);
        if (previous === "reconnecting" && push.payload.state === "connected") beginCatchUp();
        break;
      }
      case "resync-required": void resync(push.payload.reason); break;
      default: break;
    }
  }

  function foldEvents(events: EngineEvent[], lastSeq: number, instanceId: string | null | undefined) {
    setState((current) => ({
      ...applyEvents(current, events),
      connection: { ...current.connection, lastSeq: Math.max(current.connection.lastSeq, lastSeq), instanceId: instanceId ?? current.connection.instanceId },
    }));
    scheduleBulkRefetch();
  }

  function setFlood(value: boolean) {
    if (value && floodQuietTimer) { scheduler.clearTimeout(floodQuietTimer); floodQuietTimer = null; }
    if ((state.flood === true) !== value) patch({ flood: value });
  }

  function drainBacklog() {
    if (backlogTimer !== null || backlog.length === 0) return;
    const slice = backlog.splice(0, FLOOD_SLICE);
    const last = backlog.length === 0 && backlogMeta ? backlogMeta : null;
    // A slice moves the cursor only as far as its own events; the push's cursor is applied with the slice that ends it.
    const seqs = slice.map((event) => event.seq).filter((seq): seq is number => Number.isSafeInteger(seq));
    foldEvents(slice, last ? last.lastSeq : Math.max(0, ...seqs), last ? last.instanceId : undefined);
    if (last) backlogMeta = null;
    // Every slice is followed by a pause, so a push that arrives right behind it waits its turn and the list renders in between.
    backlogTimer = scheduler.setTimeout(() => {
      backlogTimer = null;
      if (backlog.length > 0) { drainBacklog(); return; }
      floodQuietTimer = scheduler.setTimeout(() => { floodQuietTimer = null; if (backlog.length === 0) setFlood(false); }, FLOOD_QUIET_MS);
    }, FLOOD_PAUSE_MS);
  }

  /** A reload or a stop drops what is still waiting: the reload reads the truth again. */
  function dropBacklog() {
    backlog = [];
    backlogMeta = null;
    if (backlogTimer !== null) { scheduler.clearTimeout(backlogTimer); backlogTimer = null; }
    if (floodQuietTimer !== null) { scheduler.clearTimeout(floodQuietTimer); floodQuietTimer = null; }
    if (state.flood === true) patch({ flood: false });
  }

  let bulkTimer: unknown = null;
  /** `files.bulk_changed` marks folders stale; the visible one is refetched after 300 ms of quiet. */
  function scheduleBulkRefetch() {
    const current = currentFolderKey();
    if (!current || !state.folders[current]?.stale || bulkTimer) return;
    bulkTimer = scheduler.setTimeout(() => {
      bulkTimer = null;
      const key = currentFolderKey();
      if (key && state.folders[key]?.stale) void reloadFolder(key);
    }, 300);
  }

  /** Full reload after a gap (20-frontend §1.7.4): drives, current folder, then continue events from the newest as_of_seq. */
  function resync(_reason: string): Promise<void> {
    resyncPromise ??= (async () => {
      dropBacklog();
      setState((current) => ({ ...current, connection: { ...current.connection, resyncing: true } }));
      try {
        if (isLiveEngine(state.engine)) {
          await loadDrives();
          const key = currentFolderKey();
          let maxAsOf = 0;
          if (key) {
            const reloaded = await reloadFolder(key);
            maxAsOf = Math.max(maxAsOf, reloaded?.asOfSeq ?? 0);
            setState((current) => {
              const folders = { ...current.folders };
              for (const other of Object.keys(folders)) if (other !== key) folders[other] = { ...folders[other], stale: true };
              return { ...current, folders };
            });
          }
          if (maxAsOf === 0) maxAsOf = Number(state.engine?.health?.events_head) || 0;
          const subscribed = await api.subscribeEvents(maxAsOf > 0 ? { sinceSeq: maxAsOf } : {});
          if (subscribed.ok) {
            applyConnection(subscribed.value.connection, subscribed.value.instanceId);
            setState((current) => ({ ...current, cursor: { appliedSeq: Math.max(current.cursor.appliedSeq, maxAsOf) } }));
          }
        }
      } finally {
        setState((current) => ({ ...current, connection: { ...current.connection, resyncing: false } }));
        resyncPromise = null;
      }
    })();
    return resyncPromise;
  }

  // ---- drives --------------------------------------------------------------------

  function setDrives(drives: Drive[]) {
    setState((current) => {
      const byId: Record<string, Drive> = {};
      for (const drive of drives) byId[drive.id] = drive;
      const order = drives.map((drive) => drive.id);
      let route = current.route;
      if (!route.driveId || !byId[route.driveId]) route = selectDriveRoute(route, order[0] ?? null);
      return { ...current, drives: { byId, order, loaded: true, loading: false, error: null }, route };
    });
  }

  async function loadDrives(): Promise<ActionResult<Drive[]>> {
    setState((current) => ({ ...current, drives: { ...current.drives, loading: true } }));
    const result = await api.listDrives();
    if (!result.ok) {
      const error = toSuperDriveError(result);
      setState((current) => ({ ...current, drives: { ...current.drives, loading: false, error } }));
      return { ok: false, error };
    }
    setDrives(result.value);
    return ok(result.value);
  }

  function upsertDrive(drive: Drive) {
    setState((current) => {
      const exists = current.drives.byId[drive.id] !== undefined;
      return {
        ...current,
        drives: {
          ...current.drives, loaded: true,
          byId: { ...current.drives.byId, [drive.id]: drive },
          order: exists ? current.drives.order : [...current.drives.order, drive.id],
        },
      };
    });
  }

  const PATCH_FIELDS = {
    name: (edit: DriveEdit) => edit.name,
    paused: (edit: DriveEdit) => edit.paused,
    model_enabled: (edit: DriveEdit) => edit.modelEnabled,
    llm_daily_calls: (edit: DriveEdit) => edit.llmDailyCalls,
    exclude: (edit: DriveEdit) => edit.exclude,
  };

  /** Optimistic PATCH with rollback (20-frontend §1.7.5); a 412 reloads the Drive and says it changed elsewhere. */
  async function updateDrive(driveId: string, edit: DriveEdit): Promise<ActionResult<Drive>> {
    const previous = state.drives.byId[driveId];
    if (!previous) return { ok: false, error: { code: "DOCS_SOURCE_DRIVE_NOT_FOUND", retryable: false, details: {} } };
    const body: Record<string, unknown> = {};
    for (const [key, read] of Object.entries(PATCH_FIELDS)) {
      const value = read(edit);
      if (value !== undefined) body[key] = value;
    }
    if (edit.autoPublishBp !== undefined) body.review = { auto_publish_bp: edit.autoPublishBp };
    const optimistic: Drive = {
      ...previous,
      ...(edit.name === undefined ? {} : { name: edit.name }),
      ...(edit.paused === undefined ? {} : { status: edit.paused ? "paused" : "active" }),
      ...(edit.modelEnabled === undefined ? {} : { model_enabled: edit.modelEnabled }),
      ...(edit.llmDailyCalls === undefined ? {} : { llm_daily_calls: edit.llmDailyCalls }),
      ...(edit.exclude === undefined ? {} : { config: { ...previous.config, exclude: edit.exclude } }),
    };
    upsertDrive(optimistic);
    const result = await api.updateDrive({ driveId, revision: previous.revision, patch: body });
    if (result.ok) {
      upsertDrive(result.value);
      return ok(result.value);
    }
    upsertDrive(previous);
    if (result.error.code === "DOCS_ENGINE_PRECONDITION_FAILED") {
      const latest = await api.getDrive(driveId);
      if (latest.ok) upsertDrive(latest.value);
    }
    return failureOf(result);
  }

  async function createDrive(input: DistributiveOmit<DriveCreateInput, "operationId"> & { operationId?: string }): Promise<ActionResult<Drive>> {
    const result = await api.createDrive({ ...input, operationId: input.operationId ?? createId() });
    if (!result.ok) return failureOf(result);
    upsertDrive(result.value);
    setState((current) => ({ ...current, route: selectDriveRoute({ ...current.route, section: "files" }, result.value.id) }));
    void loadFolder({ driveId: result.value.id, path: "" });
    return ok(result.value);
  }

  async function ensureMyFiles(): Promise<ActionResult<Drive>> {
    const result = await api.ensureMyFiles({ operationId: createId() });
    if (!result.ok) return failureOf(result);
    upsertDrive(result.value);
    void loadFolder({ driveId: result.value.id, path: "" });
    return ok(result.value);
  }

  async function deleteDrive(driveId: string): Promise<ActionResult<Drive>> {
    const current = state.drives.byId[driveId];
    if (!current) return { ok: false, error: { code: "DOCS_SOURCE_DRIVE_NOT_FOUND", retryable: false, details: {} } };
    const result = await api.deleteDrive({ driveId, revision: current.revision });
    if (!result.ok) return failureOf(result);
    upsertDrive(result.value);
    return ok(result.value);
  }

  async function syncDrive(driveId: string, options: { deep?: boolean; confirmMassDelete?: number; operationId?: string } = {}) {
    const result = await api.syncDrive({ operationId: options.operationId ?? createId(), driveId, deep: options.deep, confirmMassDelete: options.confirmMassDelete });
    return result.ok ? ok(result.value) : failureOf(result);
  }

  // ---- file detail ---------------------------------------------------------------

  /** The detail document (with history) from the cache or the engine; an event for the file drops the cached copy. */
  async function loadFileDetail(fileId: string): Promise<ActionResult<EngineFileDetail>> {
    const cached = state.details.get(fileId) as EngineFileDetail | undefined;
    if (cached) return ok(cached);
    const result = await api.fileDetail({ fileId, include: ["history"] });
    if (!result.ok) return failureOf(result);
    state.details.set(fileId, result.value);
    setState((current) => ({ ...current }));
    return ok(result.value);
  }

  /** The detail document with its entities, facts and shares (the "Entities" and "Shares" tabs); dropped with the base one when the file changes. */
  async function loadFileExtras(fileId: string): Promise<ActionResult<EngineFileDetail>> {
    const key = `${fileId}${FILE_EXTRAS_SUFFIX}`;
    const cached = state.details.get(key) as EngineFileDetail | undefined;
    if (cached) return ok(cached);
    const result = await api.fileDetail({ fileId, include: ["entities", "facts", "shares"] });
    if (!result.ok) return failureOf(result);
    state.details.set(key, result.value);
    setState((current) => ({ ...current }));
    return ok(result.value);
  }

  /**
   * Finds the file a `src:<path>#<anchor>` label points at (BRIEF lines, evidence) and opens its parsed content on that block.
   * The path is looked up in the loaded records first, then in the Drive by name; a label that matches no file reports so.
   */
  async function locateSource(driveId: string, label: string): Promise<ActionResult<{ fileId: string }>> {
    const body = label.replace(/^src:/u, "");
    const hash = body.lastIndexOf("#");
    const path = hash === -1 ? body : body.slice(0, hash);
    const known = Object.values(state.files).find((file) => file.drive_id === driveId && file.path === path);
    let fileId = known?.id ?? null;
    if (!fileId) {
      const found = await api.listFiles({ driveId, q: path.slice(path.lastIndexOf("/") + 1), recursive: true, limit: 50 });
      if (!found.ok) return failureOf(found);
      fileId = found.value.items.find((file) => file.path === path)?.id ?? null;
    }
    if (!fileId) return { ok: false, error: { code: "DOCS_ENGINE_NOT_FOUND", retryable: false, details: {} } };
    const seconds = sourceSeconds(label);
    openDetails(fileId, { tab: "blocks", focus: { src: label.startsWith("src:") ? label : `src:${label}`, ...(seconds === null ? {} : { seconds }) } });
    return ok({ fileId });
  }

  // ---- batch retry ---------------------------------------------------------------

  const RETRY_BATCH_SIZE = 500;

  /** Puts the files into the state the engine will report next; a later event replaces the record and drops the mark. */
  function markRetrying(ids: readonly string[]) {
    setState((current) => {
      const files = { ...current.files };
      for (const id of ids) {
        const record = files[id];
        if (!record || record.optimistic) continue;
        const { optimistic: _unused, ...prev } = record;
        files[id] = { ...record, state: "queued", state_code: null, retryable: false, progress: null, blocked_reason: null, optimistic: { op: "retry", prev: prev as FileSummary } };
      }
      return { ...current, files };
    });
  }

  /** Restores a file only while the guess is still on screen: an event that already arrived is newer than the guess. */
  function rollbackRetrying(ids: readonly string[]) {
    setState((current) => {
      let files = current.files;
      for (const id of ids) {
        const record = files[id];
        if (!record?.optimistic) continue;
        if (files === current.files) files = { ...current.files };
        files[id] = { ...(record.optimistic.prev as FileSummary), seq: record.seq };
      }
      return files === current.files ? current : { ...current, files };
    });
  }

  /**
   * Retries files: marks them queued at once, sends batches of at most 500 and rolls back, per file, whatever the engine
   * did not accept. A batch that fails as a whole rolls back its files and returns the error (a timeout is "not confirmed":
   * repeat the call with the same `operationId`).
   */
  async function retryFiles(fileIds: readonly string[], { mode = "normal", operationId }: { mode?: "normal" | "safe"; operationId?: string } = {}): Promise<ActionResult<{ accepted: number; rejected: number }>> {
    const ids = [...new Set(fileIds)];
    if (ids.length === 0) return ok({ accepted: 0, rejected: 0 });
    const base = operationId ?? createId();
    markRetrying(ids);
    let accepted = 0;
    let rejected = 0;
    for (let start = 0, batch = 0; start < ids.length; start += RETRY_BATCH_SIZE, batch += 1) {
      const slice = ids.slice(start, start + RETRY_BATCH_SIZE);
      const result = await api.retryFiles({ operationId: `${base}-${batch}`, fileIds: slice, mode });
      if (!result.ok) {
        rollbackRetrying(ids.slice(start));
        return failureOf(result);
      }
      const refused = result.value.results.filter((entry) => entry.status !== "ok").map((entry) => entry.id);
      rollbackRetrying(refused);
      rejected += refused.length;
      accepted += slice.length - refused.length;
    }
    addNotice({ kind: "batch_result", values: { accepted, rejected } }, 6_000);
    return ok({ accepted, rejected });
  }

  /** "Select all N matching": the main process pages through the same filter and retries every retryable file. */
  async function retryMatching(
    filter: { driveId: string; path?: string; recursive?: boolean; states?: readonly string[]; q?: string },
    { mode = "normal", operationId }: { mode?: "normal" | "safe"; operationId?: string } = {},
  ): Promise<ActionResult<{ matched: number; accepted: number; rejected: number }>> {
    const result = await api.retryMatching({
      operationId: operationId ?? createId(), driveId: filter.driveId, path: filter.path, recursive: filter.recursive,
      states: filter.states ? [...filter.states] : undefined, q: filter.q, mode,
    });
    if (!result.ok) return failureOf(result);
    const rejected = result.value.results.filter((entry) => entry.status !== "ok").length;
    const accepted = result.value.results.length - rejected;
    addNotice({ kind: "batch_result", values: { accepted, rejected } }, 6_000);
    const key = currentFolderKey();
    if (key) void reloadFolder(key);
    return ok({ matched: result.value.matched, accepted, rejected });
  }

  // ---- folders -------------------------------------------------------------------

  function currentFolderKey(): FolderKey | null {
    const { route, drives } = state;
    const driveId = route.driveId ?? drives.order[0] ?? null;
    return driveId ? folderKey(driveId, route.path) : null;
  }

  function recordsFor(items: FileSummary[], seq: number) {
    setState((current) => {
      const files = { ...current.files };
      for (const item of items) {
        const existing = files[item.id];
        // A newer event already moved this file past the page's snapshot.
        if (existing && existing.seq > seq) continue;
        files[item.id] = { ...item, seq };
      }
      return { ...current, files };
    });
  }

  function patchFolder(key: FolderKey, update: (page: FolderPage) => FolderPage) {
    setState((current) => ({
      ...current,
      folders: { ...current.folders, [key]: update(current.folders[key] ?? createEmptyFolderPage()) },
    }));
  }

  /** First page of a folder: subfolders from `tree`, files from `files-list` (keyset pages of 500). */
  async function loadFolder(input: { driveId: string; path: string; filter?: FolderFilter; force?: boolean }): Promise<FolderPage | null> {
    const { driveId, path, filter } = input;
    const key = folderKey(driveId, path, filter);
    const existing = state.folders[key];
    if (existing?.loading && !input.force) return existing;
    if (existing && !existing.stale && !existing.error && !input.force && existing.asOfSeq > 0) return existing;
    const token = ++requestCounter;
    folderRequests.set(key, token);
    patchFolder(key, (page) => ({ ...page, loading: true, error: null }));
    const flat = filter?.recursive === true;
    const [tree, files] = await Promise.all([
      flat ? Promise.resolve(null) : api.driveTree({ driveId, path }),
      api.listFiles({
        driveId, path, recursive: filter?.recursive, states: filter?.states ? [...filter.states] : undefined, q: filter?.q || undefined,
        sort: filter?.sort, limit: FOLDER_PAGE_LIMIT,
      }),
    ]);
    if (folderRequests.get(key) !== token) return state.folders[key] ?? null; // A newer request owns this key.
    const failed = [tree, files].find((result) => result && !result.ok) as Extract<ApiResult<unknown>, { ok: false }> | undefined;
    if (failed) {
      patchFolder(key, (page) => ({ ...page, loading: false, error: toSuperDriveError(failed) }));
      return state.folders[key];
    }
    const page = (files as Extract<ApiResult<{ items: FileSummary[]; next_cursor: string | null; as_of_seq: number }>, { ok: true }>).value;
    recordsFor(page.items, page.as_of_seq);
    patchFolder(key, () => ({
      ids: page.items.map((item) => item.id),
      dirs: tree && tree.ok ? tree.value.dirs : [],
      nextCursor: page.next_cursor,
      total: page.next_cursor === null ? page.items.length : null,
      asOfSeq: page.as_of_seq,
      loading: false,
      stale: false,
      error: null,
    }));
    return state.folders[key];
  }

  const reloadFolder = (key: FolderKey) => {
    const { driveId, path } = parseFolderKey(key);
    return loadFolder({ driveId, path, filter: filterFromKey(key), force: true });
  };

  function filterFromKey(key: FolderKey): FolderFilter | undefined {
    const { filterHash } = parseFolderKey(key);
    if (!filterHash) return undefined;
    const [states, q, recursive, sort] = JSON.parse(filterHash) as [string, string, boolean, "path" | "state_since"];
    return { states: states ? states.split(",") : undefined, q: q || undefined, recursive, sort };
  }

  async function loadMore(key: FolderKey): Promise<void> {
    const page = state.folders[key];
    if (!page || page.loading || page.nextCursor === null) return;
    const { driveId, path } = parseFolderKey(key);
    const filter = filterFromKey(key);
    patchFolder(key, (current) => ({ ...current, loading: true }));
    const result = await api.listFiles({
      driveId, path, recursive: filter?.recursive, states: filter?.states ? [...filter.states] : undefined, q: filter?.q,
      sort: filter?.sort, limit: FOLDER_PAGE_LIMIT, cursor: page.nextCursor,
    });
    if (!result.ok) {
      patchFolder(key, (current) => ({ ...current, loading: false, error: toSuperDriveError(result) }));
      return;
    }
    recordsFor(result.value.items, result.value.as_of_seq);
    patchFolder(key, (current) => {
      const known = new Set(current.ids);
      const ids = [...current.ids, ...result.value.items.map((item) => item.id).filter((id) => !known.has(id))];
      return {
        ...current, ids, loading: false, error: null, nextCursor: result.value.next_cursor,
        total: result.value.next_cursor === null ? ids.length : null,
      };
    });
  }

  // ---- route and selection ----------------------------------------------------------

  function setRoute(update: (route: SuperDriveRoute) => SuperDriveRoute) {
    setState((current) => ({
      ...current,
      route: update(current.route),
      selection: { driveId: current.selection.driveId, ids: new Set(), anchorId: null, allMatching: false },
    }));
  }

  function openFolder(path: string, filter?: FolderFilter) {
    setRoute((route) => navigateToFolder(route, path));
    const driveId = state.route.driveId ?? state.drives.order[0];
    if (driveId) void loadFolder({ driveId, path, filter });
  }

  function openDrive(driveId: string) {
    setRoute((route) => selectDriveRoute({ ...route, section: "files" }, driveId));
    void loadFolder({ driveId, path: "" });
  }

  function openSection(section: SuperDriveRoute["section"]) {
    setState((current) => ({ ...current, route: { ...current.route, section } }));
  }

  function setView(view: SuperDriveRoute["view"]) {
    setState((current) => ({ ...current, route: { ...current.route, view } }));
  }

  function setSelection(ids: Iterable<string>, anchorId: string | null = null, allMatching = false) {
    setState((current) => ({
      ...current,
      selection: { driveId: current.route.driveId, ids: new Set(ids), anchorId, allMatching },
    }));
  }

  let focusCounter = 0;
  /** Opens (or closes, with `null`) the details of a file; `tab` and `focus` land on a tab and a place inside it (evidence, a time). */
  function openDetails(fileId: string | null, options: { tab?: SuperDriveRoute["detailsTab"]; focus?: Omit<NonNullable<SuperDriveRoute["detailsFocus"]>, "nonce"> } = {}) {
    setState((current) => ({
      ...current,
      route: {
        ...current.route, detailsFileId: fileId, entityKey: fileId ? null : current.route.entityKey,
        detailsTab: fileId ? options.tab ?? (fileId === current.route.detailsFileId ? current.route.detailsTab : "overview") : "overview",
        detailsFocus: fileId && options.focus ? { ...options.focus, nonce: ++focusCounter } : null,
      },
    }));
  }

  function setDetailsTab(tab: SuperDriveRoute["detailsTab"]) {
    setState((current) => ({ ...current, route: { ...current.route, detailsTab: tab } }));
  }

  /** Opens an entity in the details column; `null` closes it. */
  function openEntity(entityKey: string | null) {
    setState((current) => ({ ...current, route: { ...current.route, entityKey, detailsFocus: null } }));
  }

  /** Brings a Share into view in the share list. */
  function focusShare(shareId: string | null) {
    setState((current) => ({ ...current, route: { ...current.route, section: shareId ? "shares" : current.route.section, focusShareId: shareId } }));
  }

  const ontology = createOntologyActions({ api, getState, setState, createId, addNotice, scheduler });

  // ---- lifecycle ---------------------------------------------------------------------

  async function start(): Promise<void> {
    if (started) return;
    started = true;
    stopPushes = api.subscribePushes(handlePush);
    const engine = await api.readEngine();
    if (engine.ok) setEngine(engine.value);
    const subscribed = await api.subscribeEvents();
    if (subscribed.ok) applyConnection(subscribed.value.connection, subscribed.value.instanceId);
    if (isLiveEngine(state.engine) && !state.drives.loaded) await loadDrives();
  }

  function stop() {
    if (!started) return;
    started = false;
    dropBacklog();
    stopPushes?.();
    stopPushes = null;
    void api.unsubscribeEvents();
  }

  return {
    api,
    getState,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    start,
    stop,
    handlePush,
    resync,
    actions: {
      refreshEngine: () => engineAction(api.readEngine),
      runPreflight: () => engineAction(api.preflight),
      startEngine: (options?: { keepEngineOnQuit?: boolean }) => engineAction(() => api.startEngine(options)),
      stopEngine: () => engineAction(api.stopEngine),
      restartEngine: () => engineAction(api.restartEngine),
      setKeepEngine: (value: boolean) => engineAction(() => api.setKeepEngine(value)),
      setRuntime: (runtime: "auto" | "docker" | "lite") => engineAction(() => api.setRuntime(runtime)),
      async chooseOoExecutable(): Promise<ActionResult<EngineSnapshot | null>> {
        const picked = await api.pickPath("oo-executable");
        if (!picked.ok) return failureOf(picked);
        if ("cancelled" in picked.value) return ok(null);
        return engineAction(() => api.setOoExecutable((picked.value as { grantId: string }).grantId));
      },
      revealEngineFolder: (target: "logs" | "state") => api.revealEngineFolder(target),
      openExternal: (url: string) => api.openExternal(url),
      loadDrives,
      createDrive,
      ensureMyFiles,
      updateDrive,
      deleteDrive,
      syncDrive,
      retryFiles,
      retryMatching,
      loadFileDetail,
      loadFileExtras,
      getShares: () => state.shares.byId,
      locateSource,
      fileBlocks: (input: { fileId: string; page?: number; limit?: number; cursor?: string }) => api.fileBlocks(input),
      previewUrl: (fileId: string) => api.previewUrl({ fileId }),
      loadFolder,
      reloadFolder,
      loadMore,
      openFolder,
      openDrive,
      openSection,
      setView,
      setSelection,
      openDetails,
      setDetailsTab,
      openEntity,
      focusShare,
      dismissNotice,
      pickDriveRoot: () => api.pickPath("drive-root"),
      pickMountTarget: () => api.pickPath("mount-target"),
      ...ontology,
    },
  };
}

