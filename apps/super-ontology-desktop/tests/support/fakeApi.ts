import { vi } from "vitest";
import type { ApiResult, SuperDriveApi } from "@so-model/superDriveApi";
import type { Drive, FileSummary, Share } from "@so-contract/docs-api.generated";
import type { EngineSnapshot, SuperOntologyPush } from "@so-shared/bridge";

export const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
export const OTHER_DRIVE_ID = "00000000-0000-4000-8000-000000000002";

export const okResult = <T,>(value: T): ApiResult<T> => ({ ok: true, value });
export const failResult = (code: string, extra: { retryable?: boolean; details?: Record<string, unknown> } = {}): ApiResult<never> => ({
  ok: false, error: { code, retryable: extra.retryable ?? false, details: extra.details ?? {} },
});

export function makeDrive(overrides: Partial<Drive> = {}): Drive {
  return {
    id: DRIVE_ID, name: "Docs", backend: "fs", config: { root: "/Users/example/Docs" }, status: "active", revision: 1,
    counts: {}, created_at: "2026-11-17T00:00:00Z", ...overrides,
  } as Drive;
}

export function makeFile(path: string, overrides: Partial<FileSummary> = {}): FileSummary {
  const name = path.slice(path.lastIndexOf("/") + 1);
  return {
    id: `f-${path}`, drive_id: DRIVE_ID, path, name, state: "queued", state_since: "2026-11-17T00:00:00Z",
    retryable: false, pending_review: 0, ...overrides,
  } as FileSummary;
}

export function makeShare(id: string, overrides: Partial<Share> = {}): Share {
  return {
    id, pack_id: `pack-${id}`, pack_digest: "0".repeat(64), drive_id: DRIVE_ID, scope: ["Projects/a"], role: "suggester", task: `Task ${id}`, budget: 800, version: 17,
    state: "active", mount: { target_dir: "/tmp/mount", present: true }, created_at: "2026-11-17T00:00:00Z", expires_at: "2026-11-24T00:00:00Z", revision: 1, ...overrides,
  } as Share;
}

export function makeSnapshot(overrides: Partial<EngineSnapshot> = {}): EngineSnapshot {
  return {
    revision: 1, status: "healthy", reason: null, code: null, preflight: null, health: { events_head: 0 },
    restarts: { count: 0, nextAttemptAt: null }, ownedByDesktop: true, keepEngineOnQuit: false, startedAt: null,
    connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 }, ...overrides,
  };
}

/** A SuperDriveApi whose every method is a vi.fn with a benign default; override per test. */
export function createFakeApi(overrides: Partial<Record<keyof SuperDriveApi, unknown>> = {}) {
  let pushListener: ((push: SuperOntologyPush) => void) | null = null;
  const api = {
    isAvailable: vi.fn(() => true),
    openExternal: vi.fn(async () => undefined),
    readEngine: vi.fn(async () => okResult(makeSnapshot())),
    preflight: vi.fn(async () => okResult(makeSnapshot({ status: "stopped" }))),
    startEngine: vi.fn(async () => okResult(makeSnapshot({ status: "starting" }))),
    stopEngine: vi.fn(async () => okResult(makeSnapshot({ status: "stopped" }))),
    restartEngine: vi.fn(async () => okResult(makeSnapshot({ status: "starting" }))),
    setKeepEngine: vi.fn(async (keep: boolean) => okResult(makeSnapshot({ keepEngineOnQuit: keep }))),
    revealEngineFolder: vi.fn(async () => okResult({ revealed: true as const })),
    pickPath: vi.fn(async () => okResult({ cancelled: true as const })),
    setRuntime: vi.fn(async () => okResult(makeSnapshot({ status: "stopped" }))),
    setOoExecutable: vi.fn(async () => okResult(makeSnapshot({ status: "stopped" }))),
    listDrives: vi.fn(async () => okResult([makeDrive()])),
    getDrive: vi.fn(async () => okResult(makeDrive())),
    createDrive: vi.fn(async () => okResult(makeDrive())),
    updateDrive: vi.fn(async () => okResult(makeDrive({ revision: 2 }))),
    deleteDrive: vi.fn(async () => okResult(makeDrive({ status: "deleting" }))),
    syncDrive: vi.fn(async () => okResult({ status: "running" })),
    driveTree: vi.fn(async () => okResult({ dirs: [], files: { items: [], next_cursor: null, as_of_seq: 0 } })),
    listFiles: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 0 })),
    subscribeEvents: vi.fn(async () => okResult({ instanceId: "1:a", lastSeq: 0, connection: { state: "connected" as const, attempt: 0, nextRetryAt: null, lastSeq: 0 } })),
    eventsSince: vi.fn(async () => okResult({ events: [], lastSeq: 0 })),
    unsubscribeEvents: vi.fn(async () => okResult({ unsubscribed: true as const })),
    pathsForFiles: vi.fn((files: ArrayLike<File>) => ({ paths: Array.from(files).map((file) => `/drop/${file.name}`), unreadable: 0 })),
    startUpload: vi.fn(async () => okResult({ batchId: "00000000-0000-4000-8000-0000000000b1", streaming: true as const })),
    fileDetail: vi.fn(async (input: { fileId: string }) => okResult({ ...makeFile("a.md"), id: input.fileId, drive_id: DRIVE_ID, state: "published" as const, parse: null, history: [], versions: [] } as never)),
    fileBlocks: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    previewUrl: vi.fn(async () => okResult({ url: "so-drive://token/a.md", expiresAt: "2026-11-17T01:00:00Z" })),
    retryFiles: vi.fn(async (input: { fileIds: string[] }) => okResult({ results: input.fileIds.map((id) => ({ id, status: "ok" as const })) })),
    retryMatching: vi.fn(async () => okResult({ results: [], matched: 0 })),
    readCloudMode: vi.fn(async () => okResult({ mode: "local" as const, configured: true, host: "api.so.ai", remoteUrl: "https://api.so.ai/api/v1/oo-docs" })),
    setCloudMode: vi.fn(async (mode: "local" | "cloud") => okResult({ mode, configured: true, host: "api.so.ai", remoteUrl: "https://api.so.ai/api/v1/oo-docs" })),
    auditShares: vi.fn(async () => okResult({ items: [] })),
    auditVerify: vi.fn(async () => okResult({ intact: true, rows: 0 })),
    createCloudToken: vi.fn(async (input: { tenant: string; driveIds: string[]; expiresInSeconds?: number }) => okResult({ id: "00000000-0000-4000-8000-0000000000d1", token: "oodc_testtokentesttokentesttokentest0123", tenant: input.tenant, drive_ids: input.driveIds, expires_in_seconds: input.expiresInSeconds ?? 86_400 })),
    revokeCloudToken: vi.fn(async () => okResult({ revoked: true as const })),
    cancelUpload: vi.fn(async () => okResult({ cancelled: true as const })),
    discardUpload: vi.fn(async () => okResult({ removed: 0 })),
    decideUpload: vi.fn(async () => okResult({ copying: 0, skipped: 0 })),
    searchEntities: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    entityDetail: vi.fn(async (input: { entityIdOrKey: string }) => okResult({ id: input.entityIdOrKey, key: input.entityIdOrKey, type: "Person", name: "Entity", status: "active", aliases: [], facts: [], history: [], redirect_from: [], files: [], revision: 1 } as never)),
    resolveText: vi.fn(async () => okResult({ mentions: [] })),
    reviewGroups: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    reviewItems: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    decideReview: vi.fn(async () => okResult({ results: [], accepted: 0, rejected: 0, publish_pending: false })),
    compilePack: vi.fn(async () => okResult({ id: "pack-1", pack_digest: "0".repeat(64), version: 1, drive_id: DRIVE_ID, scope: [], manifest: {}, tokens: { brief: 10, budget: 800 }, brief_preview: "# BRIEF", created_at: "2026-11-17T00:00:00Z" } as never)),
    readPack: vi.fn(async () => okResult({ id: "pack-1", pack_digest: "0".repeat(64), version: 1, drive_id: DRIVE_ID, scope: [], manifest: {}, tokens: {}, brief_preview: "# BRIEF", created_at: "2026-11-17T00:00:00Z" } as never)),
    listShares: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    getShare: vi.fn(async () => okResult(makeShare("s1"))),
    mountShare: vi.fn(async () => okResult(makeShare("s-new"))),
    refreshShare: vi.fn(async () => okResult(makeShare("s-refreshed", { refreshed_from: "s1" }))),
    revokeShare: vi.fn(async () => okResult({ share: makeShare("s1", { state: "revoked" }), local_removed: true })),
    revealShare: vi.fn(async () => okResult({ revealed: true })),
    shareUpdates: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 1 })),
    refreshShareGroup: vi.fn(async () => okResult({ results: [] })),
    readUsage: vi.fn(async () => okResult({ drives: [] })),
    ontologyVersions: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 0 })),
    readModelConfig: vi.fn(async () => okResult({ configured: false })),
    saveModelConfig: vi.fn(async () => okResult({ configured: true, revision: 1 })),
    correctFact: vi.fn(async () => okResult({ id: "fact-1", predicate: "owns" } as never)),
    mergeEntities: vi.fn(async () => okResult({ id: "e1", key: "person:a", type: "Person", name: "A" } as never)),
    splitEntity: vi.fn(async () => okResult({ results: [] })),
    suggestionDigest: vi.fn(async () => okResult({ day: "2026-11-18", counts: {}, total: 0, in_effect: [], pending: [] } as never)),
    suggestionDiff: vi.fn(async () => failResult("DOCS_SUGGEST_NOT_FOUND")),
    decideSuggestion: vi.fn(async () => failResult("DOCS_SUGGEST_NOT_FOUND")),
    decideSuggestionsBySource: vi.fn(async () => okResult({ results: [], decided: 0, failed: 0 } as never)),
    decodeMedia: vi.fn(async () => okResult({ decoded: true as const, durationMs: 3000, frames: 3, hasAudio: true, still: false, helperVersion: "avf-1" })),
    cancelMediaDecode: vi.fn(async () => okResult({ cancelled: true })),
    listConstraints: vi.fn(async () => okResult({ items: [], next_cursor: null, as_of_seq: 0 })),
    subscribePushes: vi.fn((callback: (push: SuperOntologyPush) => void) => { pushListener = callback; return () => { pushListener = null; }; }),
    ...Object.fromEntries(Object.entries(overrides).map(([name, implementation]) => [name, vi.fn(implementation as (...args: unknown[]) => unknown)])),
  } as unknown as SuperDriveApi & { [K in keyof SuperDriveApi]: ReturnType<typeof vi.fn> };
  return { api, push: (push: SuperOntologyPush) => pushListener?.(push), hasListener: () => pushListener !== null };
}

export function manualScheduler() {
  let now = 0;
  let nextId = 1;
  const timers = new Map<number, { at: number; callback: () => void }>();
  return {
    scheduler: {
      setTimeout: (callback: () => void, ms: number) => { const id = nextId++; timers.set(id, { at: now + ms, callback }); return id; },
      clearTimeout: (handle: unknown) => { timers.delete(handle as number); },
      now: () => now,
    },
    advance(ms: number) {
      const target = now + ms;
      for (;;) {
        const due = [...timers.entries()].filter(([, timer]) => timer.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        now = due[1].at;
        timers.delete(due[0]);
        due[1].callback();
      }
      now = target;
    },
  };
}
