import { CONNECTION_STATES, ENGINE_STATUSES } from "../../shared/super-ontology/engine-contract.mjs";
import type {
  ConnectionSnapshot,
  DrivePatchRequest,
  EngineEvent,
  EngineSnapshot,
  PickPathPurpose,
  PickPathValue,
  UploadStartValue,
  AuditShareRecord,
  AuditSharesQuery,
  AuditVerifyResult,
  BatchResult,
  EngineBlock,
  EngineFileDetail,
  EnginePage,
  SuggestionWriteback,
  SuperOntologyChannelMap,
  SuperOntologyPush,
} from "../../shared/super-ontology/bridge";
import { installBridgeReader, invoke, readBridge, validated, type ApiResult } from "./bridgeInvoke";
import { ontologyApi, type OntologyApi } from "./superDriveOntologyApi";
import { isDrive, isFileSummary, isSuggestion, isSuggestionDigest, isSuggestionDiff, type Drive, type FileSummary, type Page, type Suggestion, type SuggestionDigest, type SuggestionDiff, type SyncInfo, type Tree } from "../../shared/super-ontology/docs-api.generated";

export type { ApiResult };

/** The only module in the Super Drive renderer that touches `window.superOntology`. */
installBridgeReader(() => window.superOntology ?? null);

export function isSuperDriveBridgeAvailable(): boolean {
  return readBridge() !== null;
}

export function isEngineSnapshot(value: unknown): value is EngineSnapshot {
  if (!value || typeof value !== "object") return false;
  const snapshot = value as Partial<EngineSnapshot>;
  return Number.isSafeInteger(snapshot.revision)
    && typeof snapshot.status === "string"
    && (ENGINE_STATUSES as readonly string[]).includes(snapshot.status)
    && typeof snapshot.connection === "object"
    && snapshot.connection !== null
    && (CONNECTION_STATES as readonly string[]).includes(snapshot.connection.state);
}

const asSnapshot = (value: unknown): EngineSnapshot | null => (isEngineSnapshot(value) ? value : null);
const asDrive = (value: unknown): Drive | null => (isDrive(value) ? value : null);
const isPage = (value: unknown): value is Page<unknown> => {
  const page = value as Partial<Page<unknown>> | null;
  return typeof page === "object" && page !== null && Array.isArray(page.items)
    && (page.next_cursor === null || typeof page.next_cursor === "string") && Number.isSafeInteger(page.as_of_seq);
};
const asTree = (value: unknown): Tree | null => {
  const tree = value as Partial<Tree> | null;
  return tree && Array.isArray(tree.dirs) && isPage(tree.files) ? (tree as Tree) : null;
};
const asFilePage = (value: unknown): Page<FileSummary> | null => {
  return isPage(value) && value.items.every((item) => isFileSummary(item)) ? (value as Page<FileSummary>) : null;
};

export type FilesQuery = {
  driveId: string;
  path?: string;
  recursive?: boolean;
  states?: string[];
  q?: string;
  sort?: "path" | "state_since";
  limit?: number;
  cursor?: string;
};

type DriveCreateCommon = { operationId: string; name: string; exclude?: string[]; allowPrefixes?: string[]; denyPrefixes?: string[]; modelEnabled?: boolean; llmDailyCalls?: number };
/** The object-store backends of docs-api 1.4: all S3 compatible; oss, cos and r2 derive their endpoint. */
export type ObjectStoreBackend = "s3" | "oss" | "cos" | "r2";
export type DriveCreateInput =
  | (DriveCreateCommon & { backend?: "fs"; rootGrantId: string })
  | (DriveCreateCommon & {
    backend: ObjectStoreBackend; endpoint?: string; region?: string; bucket: string; prefix?: string; pathStyle?: boolean;
    /** R2 only: the 32-hex account id. */
    accountId?: string;
    /** OSS and COS only: the in-cloud endpoint. */
    internal?: boolean;
    /** Write-only: handed to the main process once, which stores them in private files. */
    accessKeyId: string; secretAccessKey: string; sessionToken?: string;
  });

/** What the store depends on; tests pass a fake with the same shape. */
export type SuperDriveApi = OntologyApi & {
  isAvailable(): boolean;
  openExternal(url: string): Promise<void>;
  readEngine(): Promise<ApiResult<EngineSnapshot>>;
  preflight(): Promise<ApiResult<EngineSnapshot>>;
  startEngine(options?: { keepEngineOnQuit?: boolean }): Promise<ApiResult<EngineSnapshot>>;
  stopEngine(): Promise<ApiResult<EngineSnapshot>>;
  restartEngine(): Promise<ApiResult<EngineSnapshot>>;
  setKeepEngine(keepEngineOnQuit: boolean): Promise<ApiResult<EngineSnapshot>>;
  revealEngineFolder(target: "logs" | "state"): Promise<ApiResult<{ revealed: true }>>;
  pickPath(purpose: PickPathPurpose): Promise<ApiResult<PickPathValue>>;
  setOoExecutable(grantId: string): Promise<ApiResult<EngineSnapshot>>;
  /** Docker, the embedded runtime, or whichever works (F-S14-01); the main process checks the setup again. */
  setRuntime(runtime: "auto" | "docker" | "lite"): Promise<ApiResult<EngineSnapshot>>;
  listDrives(): Promise<ApiResult<Drive[]>>;
  getDrive(driveId: string): Promise<ApiResult<Drive>>;
  createDrive(input: DriveCreateInput): Promise<ApiResult<Drive>>;
  /** The default managed Drive "My files" (created on first use). */
  ensureMyFiles(input: { operationId: string }): Promise<ApiResult<Drive>>;
  updateDrive(input: { driveId: string; revision: number; patch: DrivePatchRequest }): Promise<ApiResult<Drive>>;
  deleteDrive(input: { driveId: string; revision: number }): Promise<ApiResult<Drive>>;
  syncDrive(input: { operationId: string; driveId: string; deep?: boolean; confirmMassDelete?: number }): Promise<ApiResult<SyncInfo | { status: string }>>;
  driveTree(input: { driveId: string; path: string }): Promise<ApiResult<Tree>>;
  listFiles(query: FilesQuery): Promise<ApiResult<Page<FileSummary>>>;
  subscribeEvents(input?: { sinceSeq?: number }): Promise<ApiResult<{ instanceId: string | null; lastSeq: number; connection: ConnectionSnapshot }>>;
  eventsSince(input: { instanceId: string; seq: number }): Promise<ApiResult<{ events: EngineEvent[]; lastSeq: number }>>;
  unsubscribeEvents(): Promise<ApiResult<{ unsubscribed: true }>>;
  /** Push channels (`engine-changed`, `events`, ...); returns the unsubscribe function. */
  subscribePushes(callback: (push: SuperOntologyPush) => void): () => void;
  /** Host paths of dropped or picked files; files without a path (browser drags) are only counted. */
  pathsForFiles(files: ArrayLike<File>): { paths: string[]; unreadable: number };
  startUpload(input: { driveId: string; folder: string; paths: string[] }): Promise<ApiResult<UploadStartValue>>;
  cancelUpload(input: { batchId: string; itemId?: string }): Promise<ApiResult<{ cancelled: true }>>;
  discardUpload(input: { batchId: string; folderPath: string }): Promise<ApiResult<{ removed: number }>>;
  fileDetail(input: { fileId: string; include?: ("history" | "entities" | "facts" | "shares")[] }): Promise<ApiResult<EngineFileDetail>>;
  fileBlocks(input: { fileId: string; page?: number; limit?: number; cursor?: string }): Promise<ApiResult<EnginePage<EngineBlock>>>;
  /** Capability URL of the original (fs Drives only). */
  previewUrl(input: { fileId: string }): Promise<ApiResult<{ url: string; expiresAt: string }>>;
  retryFiles(input: { operationId: string; fileIds: string[]; mode?: "normal" | "safe" }): Promise<ApiResult<BatchResult>>;
  listSuggestions(query?: { status?: string; shareId?: string; day?: string; limit?: number }): Promise<ApiResult<Page<Suggestion>>>;
  suggestionDigest(day?: string): Promise<ApiResult<SuggestionDigest>>;
  suggestionDiff(id: string): Promise<ApiResult<SuggestionDiff>>;
  decideSuggestion(input: { id: string; decision: "accept" | "reject" | "undo"; revision?: number; note?: string }): Promise<ApiResult<{ suggestion: Suggestion; writeback: SuggestionWriteback | null }>>;
  decideSuggestionsBySource(input: { shareId: string; decision: "accept" | "reject" }): Promise<ApiResult<{ results: { id: string; status: string; written?: number; code?: string }[]; decided: number; failed: number }>>;
  /** Where Shares are created (F-S12-01). Absent on an older bridge, which is the local mode. */
  /** Audit of Shares (L7): read-only, with the filters of docs-api 1.4. */
  auditShares(query?: AuditSharesQuery): Promise<ApiResult<{ items: AuditShareRecord[] }>>;
  auditVerify(): Promise<ApiResult<AuditVerifyResult>>;
  /** The token comes back once; the caller shows it and forgets it. */
  retryMatching(input: { operationId: string; driveId: string; path?: string; recursive?: boolean; states?: string[]; q?: string; mode?: "normal" | "safe" }): Promise<ApiResult<BatchResult & { matched: number }>>;
};

export const superDriveApi: SuperDriveApi = {
  ...ontologyApi,
  isAvailable: isSuperDriveBridgeAvailable,
  openExternal: async (url) => { await window.superOntology?.openExternal?.(url); },
  readEngine: async () => validated(await invoke("engine-read", {}), asSnapshot),
  preflight: async () => validated(await invoke("engine-preflight", {}), asSnapshot),
  startEngine: async (options = {}) => validated(await invoke("engine-start", options), asSnapshot),
  stopEngine: async () => validated(await invoke("engine-stop", {}), asSnapshot),
  restartEngine: async () => validated(await invoke("engine-restart", {}), asSnapshot),
  setKeepEngine: async (keepEngineOnQuit) => validated(await invoke("engine-set-keep", { keepEngineOnQuit }), asSnapshot),
  revealEngineFolder: (target) => invoke("engine-reveal-logs", { target }),
  pickPath: (purpose) => invoke("pick-path", { purpose }),
  setOoExecutable: async (grantId) => validated(await invoke("engine-set-executable", { grantId }), asSnapshot),
  setRuntime: async (runtime) => validated(await invoke("engine-set-runtime", { runtime }), asSnapshot),
  listDrives: async () => validated(await invoke("drives-list", {}), (value) => (Array.isArray(value) && value.every(isDrive) ? (value as Drive[]) : null)),
  getDrive: async (driveId) => validated(await invoke("drive-get", { driveId }), asDrive),
  ensureMyFiles: async (input) => validated(await invoke("my-files-ensure", input), asDrive),
  createDrive: async (input) => validated(await invoke("drive-create", { ...input, backend: input.backend ?? "fs" } as SuperOntologyChannelMap["drive-create"]["request"]), asDrive),
  updateDrive: async ({ driveId, revision, patch }) => validated(await invoke("drive-update", { driveId, revision, patch }), asDrive),
  deleteDrive: async ({ driveId, revision }) => validated(await invoke("drive-delete", { driveId, revision }), asDrive),
  syncDrive: (input) => invoke("drive-sync", input) as Promise<ApiResult<SyncInfo | { status: string }>>,
  driveTree: async (input) => validated(await invoke("drive-tree", input), asTree),
  listFiles: async (query) => validated(await invoke("files-list", query), asFilePage),
  subscribeEvents: (input = {}) => invoke("events-subscribe", input),
  eventsSince: (input) => invoke("events-since", input),
  unsubscribeEvents: () => invoke("events-unsubscribe", {}),
  subscribePushes: (callback) => readBridge()?.subscribe(callback) ?? (() => undefined),
  pathsForFiles: (files) => readBridge()?.pathsForFiles?.(files) ?? { paths: [], unreadable: Array.from(files ?? []).length },
  startUpload: (input) => invoke("upload-start", input),
  cancelUpload: (input) => invoke("upload-cancel", input),
  discardUpload: (input) => invoke("upload-discard", input),
  fileDetail: (input) => invoke("file-detail", input),
  fileBlocks: (input) => invoke("file-blocks", input),
  previewUrl: (input) => invoke("file-preview-url", input),
  retryFiles: (input) => invoke("files-retry", input),
  retryMatching: (input) => invoke("files-retry-matching", input),
  listSuggestions: async (query = {}) => validated(await invoke("suggestions-list", query), (value) => (isPage(value) && value.items.every(isSuggestion) ? (value as unknown as Page<Suggestion>) : null)),
  suggestionDigest: async (day) => validated(await invoke("suggestions-digest", day ? { day } : {}), (value) => (isSuggestionDigest(value) ? (value as unknown as SuggestionDigest) : null)),
  suggestionDiff: async (id) => validated(await invoke("suggestion-diff", { id }), (value) => (isSuggestionDiff(value) ? (value as unknown as SuggestionDiff) : null)),
  decideSuggestion: async (input) => validated(await invoke("suggestion-decide", input), (value) => (isSuggestion(value.suggestion) ? ({ suggestion: value.suggestion as unknown as Suggestion, writeback: value.writeback }) : null)),
  decideSuggestionsBySource: (input) => invoke("suggestions-decide-source", input),
  auditShares: (query = {}) => invoke("audit-shares", query),
  auditVerify: () => invoke("audit-verify", {}),
};

/** Kept for callers that only need the snapshot (the S1 shell). */
export const readEngineSnapshot = superDriveApi.readEngine;
