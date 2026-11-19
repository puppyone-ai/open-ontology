/** Renderer-visible types of the Super Drive bridge (20-frontend §1.5). */

export type EngineStatus =
  | "idle" | "probing" | "unsupported_platform" | "oo_missing" | "docker_missing" | "docker_stopped" | "embedded_missing"
  | "stopped" | "starting" | "healthy" | "degraded" | "restarting" | "crashed" | "incompatible" | "stopping";

export type ConnectionState = "connected" | "reconnecting" | "resyncing" | "offline";

export type EnginePreflight = {
  platform: "ok" | "unsupported";
  oo: { found: boolean; version: string | null; source: "path" | "user" | null };
  docker: { installed: boolean; running: boolean; version: string | null };
  diskFreeBytes: number | null;
  /** Which runtime this setup check chose (F-S14-01): `docker` (compose) or `lite` (embedded PostgreSQL, no Docker). Absent from an older main process. */
  runtime?: "docker" | "lite";
  /** What the person chose: `auto` takes Docker when it runs and the embedded runtime when it does not. */
  runtimeChoice?: "auto" | "docker" | "lite";
  /** The embedded runtime of the app; only for `runtime: "lite"`. */
  embedded?: { found: boolean; postgres: { found: boolean; version: string | null }; sidecar: { found: boolean } } | null;
};

export type EngineSnapshot = {
  /** Monotonic; the renderer drops older snapshots. */
  revision: number;
  status: EngineStatus;
  reason: string | null;
  code: string | null;
  /** `null` until the environment has been probed (idle/probing). Deviates from 20 §1.5.1, see the F-S1-03 report. */
  preflight: EnginePreflight | null;
  /** The control API health document, verbatim, once the engine answers. */
  health: Record<string, unknown> | null;
  restarts: { count: number; nextAttemptAt: string | null };
  ownedByDesktop: boolean;
  keepEngineOnQuit: boolean;
  startedAt: string | null;
  connection: { state: ConnectionState; attempt: number; nextRetryAt: string | null; lastSeq: number };
};

export type SuperOntologyFailure = {
  ok: false;
  error: { code: string; retryable: boolean; details: Record<string, unknown>; httpStatus?: number };
};

export type SuperOntologyResult<T> = { ok: true; value: T; replayed?: boolean } | SuperOntologyFailure;

/** Request and `value` types per implemented channel. Later cards extend this map. */
export type PickPathPurpose = "drive-root" | "mount-target" | "oo-executable";
export type PickPathValue = { grantId: string; displayPath: string } | { cancelled: true };

/** Engine documents are carried verbatim (snake_case); the renderer narrows them with the generated guards. */
export type EngineDrive = { id: string; name: string; backend: string; status: string; revision: number; [key: string]: unknown };
export type EnginePage<T> = { items: T[]; next_cursor: string | null; as_of_seq: number };
export type EngineFileSummary = { id: string; drive_id: string; path: string; state: string; [key: string]: unknown };
export type EngineTree = { dirs: { path: string; files: number; counts: Record<string, number> }[]; files: EnginePage<EngineFileSummary> };
/** A parsed block (oo.doc-ir/v1 block plus the `src` provenance label); anchors differ per format. */
export type EngineAnchor =
  | { kind: "text"; line_start: number; line_end: number; char_start: number; char_end: number }
  | { kind: "docx"; para_start: number; para_end: number; table_index?: number; cell_range?: string }
  | { kind: "slide"; slide: number; shape_id?: number; para_start?: number; para_end?: number; notes?: boolean }
  | { kind: "pdf"; page: number; bbox: [number, number, number, number]; charspan?: [number, number] }
  | { kind: "cells"; sheet: string | null; range: string };
export type EngineBlock = {
  id: string; ordinal: number; kind: string; level?: number; text: string; hpath: string[]; anchor: EngineAnchor;
  table?: { rows?: unknown[][]; [key: string]: unknown }; flags?: string[]; src?: string;
};
/** `files/{fid}` with its optional includes; the summary fields come first. */
export type EngineFileDetail = EngineFileSummary & {
  versions?: { id: string; b3: string; created_at: string; superseded_at: string | null }[];
  parse?: { parser_id?: string; parser_version?: string; block_count?: number; page_count?: number | null; warnings?: string[] } | null;
  llm?: { status?: string; calls?: number; cache_hits?: number } | null;
  history?: { seq: number; from?: string | null; to: string; code?: string | null; attempt?: number | null; at: string }[];
};
/** Suggestions (docs-api 1.3): carried verbatim; the renderer narrows them with the generated guards. */
export type EngineSuggestion = { id: string; share_id: string; kind: string; status: string; revision: number; [key: string]: unknown };
export type EngineSuggestionDigest = { day: string; counts: Record<string, number>; total: number; in_effect: EngineSuggestion[]; pending: EngineSuggestion[]; [key: string]: unknown };
export type EngineSuggestionDiff = { suggestion_id: string; share_id: string; kind: string; writable: boolean; writer: "desktop"; changes: Record<string, unknown>[]; [key: string]: unknown };
/** What Desktop wrote into the Drive when it accepted a suggestion of an `editor` Share. */
export type SuggestionWriteback = { written: { path: string; operations: { block_id: string | null; operation: string; first_line: number }[]; skipped: { reason: string; block_id: string | null }[] }[]; evidence: string[] };
export type EngineSyncInfo = { status?: string; generation?: number; [key: string]: unknown } | null;
export type DrivePatchRequest = {
  name?: string; model_enabled?: boolean; llm_daily_calls?: number; paused?: boolean; exclude?: string[];
  review?: { auto_publish_bp: number };
};

/** Persistent events carry `seq`; transient ones (`file.progress`) do not. Payload shapes follow events.v1.schema.json. */
export type EngineEvent = { seq?: number; type: string; at: string; drive_id?: string | null; data: Record<string, unknown> };
export type ConnectionSnapshot = { state: ConnectionState; attempt: number; nextRetryAt: string | null; lastSeq: number };
export type ResyncReason = "cursor_expired" | "engine_restarted" | "gap";
export type UploadProgressPush = { batchId: string; items: { itemId: string; state: UploadItemState | string; bytesDone: number; bytesTotal: number; code?: string; drivePath?: string }[] };

export type UploadItemState = "waiting" | "copying" | "done" | "failed" | "cancelled";
export type UploadStartValue = {
  batchId: string;
  items: { itemId: string; name: string; relative: string; size: number; source: string }[];
  skipped: { hidden: number; links: number };
  /** Dropped paths that could not be read (missing, or not an absolute path). */
  unreadable: number;
};

/** Per file outcome of a retry batch (contract BatchResult). */
export type BatchResult = { results: { id: string; status: "ok" | "conflict" | "not_found" | "invalid" | "not_retryable"; code?: string }[] };

/** An Agent token (docs-api 1.6). The `ooa_` secret is shown once; the renderer keeps it only while the screen that shows it is open. */
export type AgentScope = "docs:read" | "docs:suggest" | "docs:write";
export type AgentTokenIssued = { id: string; token: string; name: string; scopes: string[]; drive_ids: string[]; all_drives: boolean; expires_in_seconds: number; expires_at: string;
  /** Where the app also saved the secret (owner-only file), and the loopback address an Agent calls; both are added by the main process. */
  token_file?: string | null; endpoint?: string | null };
export type AgentTokenInfo = { id: string; name: string; scopes: string[]; drive_ids?: string[]; all_drives?: boolean; expires_at?: string | null; revoked_at?: string | null; last_used_at?: string | null; created_at?: string };

/** Audit of Shares (docs-api 1.4): who shared what to which agent and when. Read-only; the engine's table is append-only and hash-chained. */
export type AuditShareRecord = {
  seq: number; at: string; actor: string; tenant?: string | null; drive_id?: string | null; share_id: string; agent?: string | null;
  grantor?: string | null; role?: string | null; scope: unknown; revoked_at?: string | null; refs_reads: number;
};
export type AuditSharesQuery = { driveId?: string; agent?: string; tenant?: string; since?: string; until?: string };
export type AuditVerifyResult = { intact: boolean; rows?: number; broken_at_seq?: number };

/** Documents of the L4 and L6 channels are carried verbatim; the renderer narrows them with the generated guards. */
export type EngineDocument = { [key: string]: unknown };
export type ReviewDecisionItem = { id: string; revision: number; decision: "accept" | "reject" | "edit"; edit?: Record<string, unknown>; note?: string };
export type ReviewBySource = { fileId: string; kinds?: string[]; decision: "accept" | "reject"; asOfSeq?: number };
export type ReviewDecisionResult = {
  results: { id?: string; status?: "ok" | "conflict" | "not_found" | "invalid" | "already_decided"; code?: string }[];
  accepted?: number; rejected?: number; publish_pending?: boolean;
};
export type CompileInput = {
  driveId: string; scope?: string[]; task?: string; seeds?: string[]; budget?: number; role?: "viewer" | "suggester" | "editor";
  version?: number; requireComplete?: boolean; waitMs?: number; minDriveRev?: number; vector?: "hashgram/1";
};
export type ModelConfigSaveInput = { provider: "openai-compatible" | "anthropic"; baseUrl: string; model: string; apiKey?: string; revision?: number };

export type SuperOntologyChannelMap = {
  "engine-read": { request: { operationId?: string }; value: EngineSnapshot };
  "engine-preflight": { request: { operationId?: string }; value: EngineSnapshot };
  "engine-start": { request: { keepEngineOnQuit?: boolean; operationId?: string }; value: EngineSnapshot };
  "engine-set-runtime": { request: { runtime: "auto" | "docker" | "lite"; operationId?: string }; value: EngineSnapshot };
  "engine-stop": { request: { operationId?: string }; value: EngineSnapshot };
  "engine-restart": { request: { operationId?: string }; value: EngineSnapshot };
  "engine-set-keep": { request: { keepEngineOnQuit: boolean; operationId?: string }; value: EngineSnapshot };
  "engine-reveal-logs": { request: { target?: "logs" | "state"; operationId?: string }; value: { revealed: true } };
  "drives-list": { request: { operationId?: string }; value: EngineDrive[] };
  "drive-get": { request: { driveId: string; operationId?: string }; value: EngineDrive };
  /** Creates (or returns) the default managed Drive "My files": a folder inside the app's own data directory, so dropping files needs no setup. */
  "my-files-ensure": { request: { operationId: string }; value: EngineDrive };
  "drive-create": {
    request:
      | { operationId: string; name: string; backend: "fs"; rootGrantId: string; exclude?: string[]; allowPrefixes?: string[]; denyPrefixes?: string[]; modelEnabled?: boolean; llmDailyCalls?: number }
      | {
        operationId: string; name: string; backend: "s3" | "oss" | "cos" | "r2";
        /** Required for s3; oss, cos and r2 derive it from `region` or `accountId` and take it only as an override. */
        endpoint?: string; region?: string; bucket: string; prefix?: string; pathStyle?: boolean;
        /** R2: the 32-hex account id. OSS and COS: the in-cloud endpoint. */
        accountId?: string; internal?: boolean;
        /** Write-only: the main process turns these into private files and sends the engine only a `file:` reference. */
        accessKeyId: string; secretAccessKey: string; sessionToken?: string;
        /** Drive access policy: whole-segment path prefixes; deny always wins. */
        allowPrefixes?: string[]; denyPrefixes?: string[];
        exclude?: string[]; modelEnabled?: boolean; llmDailyCalls?: number;
      };
    value: EngineDrive;
  };
  "drive-update": { request: { driveId: string; revision: number; patch: DrivePatchRequest; operationId?: string }; value: EngineDrive };
  "drive-delete": { request: { driveId: string; revision: number; operationId?: string }; value: EngineDrive };
  "drive-sync": { request: { operationId: string; driveId: string; deep?: boolean; confirmMassDelete?: number }; value: EngineSyncInfo };
  "drive-tree": { request: { driveId: string; path: string; operationId?: string }; value: EngineTree };
  "files-list": {
    request: {
      driveId: string; path?: string; recursive?: boolean; states?: string[]; q?: string; sort?: "path" | "state_since";
      limit?: number; cursor?: string; operationId?: string;
    };
    value: EnginePage<EngineFileSummary>;
  };
  "events-subscribe": { request: { sinceSeq?: number; operationId?: string }; value: { instanceId: string | null; lastSeq: number; connection: ConnectionSnapshot } };
  "events-since": { request: { instanceId: string; seq: number; operationId?: string }; value: { events: EngineEvent[]; lastSeq: number } };
  "events-unsubscribe": { request: { operationId?: string }; value: { unsubscribed: true } };
  "pick-path": { request: { purpose: PickPathPurpose; operationId?: string }; value: PickPathValue };
  "engine-set-executable": { request: { grantId: string; operationId?: string }; value: EngineSnapshot };
  "upload-start": { request: { driveId: string; folder: string; paths: string[]; operationId?: string }; value: UploadStartValue };
  "file-detail": { request: { fileId: string; include?: ("history" | "entities" | "facts" | "shares")[]; operationId?: string }; value: EngineFileDetail };
  "file-blocks": { request: { fileId: string; page?: number; limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineBlock> };
  "file-preview-url": { request: { fileId: string; operationId?: string }; value: { url: string; expiresAt: string } };
  "suggestions-list": { request: { status?: string; shareId?: string; day?: string; limit?: number; operationId?: string }; value: EnginePage<EngineSuggestion> };
  "suggestions-digest": { request: { day?: string; operationId?: string }; value: EngineSuggestionDigest };
  "suggestion-diff": { request: { id: string; operationId?: string }; value: EngineSuggestionDiff };
  "suggestion-decide": { request: { id: string; decision: "accept" | "reject" | "undo"; revision?: number; note?: string; operationId?: string }; value: { suggestion: EngineSuggestion; writeback: SuggestionWriteback | null } };
  "suggestions-decide-source": { request: { shareId: string; decision: "accept" | "reject"; operationId?: string }; value: { results: { id: string; status: string; written?: number; code?: string }[]; decided: number; failed: number } };
  "files-retry": { request: { operationId: string; fileIds: string[]; mode?: "normal" | "safe" }; value: BatchResult };
  "files-retry-matching": {
    request: { operationId: string; driveId: string; path?: string; recursive?: boolean; states?: string[]; q?: string; mode?: "normal" | "safe" };
    value: BatchResult & { matched: number };
  };
  "agent-token-create": { request: { name: string; scopes: AgentScope[]; driveIds?: string[]; allDrives?: boolean; expiresInSeconds?: number; operationId?: string }; value: AgentTokenIssued };
  "agent-token-list": { request: { operationId?: string }; value: { items: AgentTokenInfo[] } };
  "agent-token-revoke": { request: { tokenId: string; operationId?: string }; value: { revoked: true } };
  "audit-shares": { request: AuditSharesQuery & { operationId?: string }; value: { items: AuditShareRecord[] } };
  "audit-verify": { request: { operationId?: string }; value: AuditVerifyResult };
  "upload-cancel": { request: { batchId: string; itemId?: string; operationId?: string }; value: { cancelled: true } };
  "entities-search": { request: { q: string; driveId?: string; type?: string; limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineDocument> };
  "entity-detail": { request: { entityIdOrKey: string; includeHistory?: boolean; operationId?: string }; value: EngineDocument };
  "resolve": { request: { text: string; driveId?: string; types?: string[]; limit?: number; operationId?: string }; value: EngineDocument };
  "review-list": {
    request: { driveId: string; kind?: string; source?: string; status?: string; groupBy?: "source"; limit?: number; cursor?: string; operationId?: string };
    value: EnginePage<EngineDocument>;
  };
  "review-decide": { request: { operationId: string; items?: ReviewDecisionItem[]; bySource?: ReviewBySource }; value: ReviewDecisionResult };
  "pack-compile": { request: { operationId: string; compile: CompileInput }; value: EngineDocument };
  "pack-read": { request: { packId: string; operationId?: string }; value: EngineDocument };
  "shares-list": { request: { driveId?: string; states?: string[]; limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineDocument> };
  "share-get": { request: { shareId: string; operationId?: string }; value: EngineDocument };
  "share-mount": {
    request: { operationId: string; compile: CompileInput; targetGrantId: string; expiresInSeconds: number; group?: string; refreshPolicy?: "manual" | "at-checkpoint" | "pinned" };
    value: EngineDocument;
  };
  "share-refresh": { request: { operationId: string; shareId: string }; value: EngineDocument };
  "share-revoke": { request: { operationId: string; shareId: string; reason?: string }; value: EngineDocument };
  "share-reveal": { request: { shareId: string; operationId?: string }; value: { revealed: boolean } };
  "share-updates": { request: { shareId: string; limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineDocument> };
  "share-group-refresh": { request: { operationId: string; groupId: string }; value: BatchResult };
  "usage-read": { request: { driveId?: string; days?: number; operationId?: string }; value: EngineDocument };
  "ontology-versions": { request: { limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineDocument> };
  "model-config-read": { request: { operationId?: string }; value: EngineDocument };
  "model-config-save": { request: ModelConfigSaveInput & { operationId?: string }; value: EngineDocument };
  "fact-correct": { request: { operationId: string; factId: string; revision: number; action: "mark_wrong" | "set_value"; value?: Record<string, unknown>; reason?: string }; value: EngineDocument };
  "entity-merge": { request: { operationId: string; into: string; from: string[]; revision: number }; value: EngineDocument };
  "entity-split": { request: { operationId: string; entityId: string; revision: number; groups: { aliases?: string[]; facts?: string[] }[] }; value: BatchResult };
  "media-decode": { request: { fileId: string; operationId?: string }; value: { decoded: true; durationMs: number; frames: number; hasAudio: boolean; still: boolean; helperVersion: string } };
  "media-decode-cancel": { request: { fileId: string; operationId?: string }; value: { cancelled: boolean } };
  "constraints-list": { request: { limit?: number; cursor?: string; operationId?: string }; value: EnginePage<EngineDocument> };
};

export type SuperOntologyChannel = keyof SuperOntologyChannelMap;

export type SuperOntologyPush =
  | { channel: "engine-changed"; payload: EngineSnapshot }
  | { channel: "events"; payload: { instanceId: string | null; events: EngineEvent[]; lastSeq: number } }
  | { channel: "connection-changed"; payload: ConnectionSnapshot }
  | { channel: "resync-required"; payload: { instanceId: string | null; reason: ResyncReason; headSeq?: number } }
  | { channel: "upload-progress"; payload: UploadProgressPush };

export type SuperOntologyBridge = {
  invoke: <C extends SuperOntologyChannel>(
    channel: C,
    request?: SuperOntologyChannelMap[C]["request"],
  ) => Promise<SuperOntologyResult<SuperOntologyChannelMap[C]["value"]>>;
  subscribe: (callback: (push: SuperOntologyPush) => void) => () => void;
  /** Host paths of dropped or picked files (preload only); `unreadable` counts files with no path. */
  pathsForFiles: (files: ArrayLike<File>) => { paths: string[]; unreadable: number };
  /** Opens an http(s) URL in the system browser. */
  openExternal?: (url: string) => Promise<void>;
};
