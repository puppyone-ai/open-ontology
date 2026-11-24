import type { ConnectionSnapshot, EngineActor, EngineEvent, EngineSnapshot } from "../../shared/super-ontology/bridge";
import type { Constraint, Drive, FileSummary, ModelConfig, ReviewGroup, ReviewItem, Share, ShareUpdate, TreeDir } from "../../shared/super-ontology/docs-api.generated";
import { LruMap } from "./lruMap";
import { INITIAL_ROUTE, type FolderKey, type SuperDriveRoute } from "./superDriveRoute";
import type { SuperDriveError } from "./superDriveErrors";

export type { EngineActor, EngineEvent, EngineSnapshot };

/** One entry of the "recent changes" feed: what moved, when, and who made it (shown as a small tag). */
export type ActivityEntry = { key: string; seq: number; type: string; at: string; actor: EngineActor | null; label: string | null };

/** Changes by anything but this window: counted so screens that read the database side load again, and listed with their actor. */
export type LiveState = {
  /** Bumped by every `db.*` event (a source added or removed with the CLI, an import or build finished). */
  dbRevision: number;
  activity: ActivityEntry[];
  /** The actor of the last change to each file, by file id. */
  fileActors: Record<string, EngineActor>;
};

/** Marks a row whose shown state is a guess waiting for the engine (20-frontend §1.7.5). */
export type OptimisticMark = { op: "retry"; prev: FileSummary };

export type FileRecord = FileSummary & {
  /** `seq` of the last event or page that updated this record; an older event never overwrites it. */
  seq: number;
  optimistic?: OptimisticMark;
};

export type FolderPage = {
  ids: string[];
  dirs: TreeDir[];
  nextCursor: string | null;
  /** Known only when the last page was reached; `null` while more pages exist. */
  total: number | null;
  asOfSeq: number;
  loading: boolean;
  /** The listing may be out of date (bulk change, reconnect): refetch when visible. */
  stale: boolean;
  error: SuperDriveError | null;
};

export type UsageToday = { day: string; calls: number; limit: number | null; limited: boolean; cacheHits?: number; filesLimited?: number };

export type OntologyVersionInfo = { n: number; asOf: string };

export type Notice = { id: string; kind: "caught_up" | "batch_result" | "info" | "review_result"; values: Record<string, unknown>; at: number };

/** One loaded listing of the review queue (groups per source, or the items of one source or kind). */
export type ReviewList<T> = {
  items: T[];
  nextCursor: string | null;
  /** `as_of_seq` of the page: a group decision must only touch what was queued before it (10 section 8.2). */
  asOfSeq: number;
  loaded: boolean;
  loading: boolean;
  /** `review.changed` arrived after the page was read. */
  stale: boolean;
  error: SuperDriveError | null;
};

export type ShareOperation = "refreshing" | "revoking" | "mounting";

export type SharesState = {
  byId: Record<string, Share>;
  order: string[];
  /** Root share id -> the chain of refreshed shares (oldest first). */
  chains: Record<string, string[]>;
  loaded: boolean;
  loading: boolean;
  /** A share event arrived that only names the share (the engine sends ids, not the Share): the list is read again. */
  stale: boolean;
  error: SuperDriveError | null;
  /** Update log entries per share (`share.update_appended` marks an entry stale until it is read again). */
  updates: Record<string, { items: ShareUpdate[]; loaded: boolean; stale: boolean; latestLevel: ShareUpdate["level"] | null }>;
  /** Pulls of an original by an Agent (`refs.accessed`), informational only. */
  accessed: Record<string, number>;
  pending: Record<string, ShareOperation>;
};

export type ModelConfigState = { config: ModelConfig | null; loaded: boolean; loading: boolean; error: SuperDriveError | null };

export type ConstraintsState = { items: Constraint[]; loaded: boolean; stale: boolean };

export type SuperDriveState = {
  engine: EngineSnapshot | null;
  connection: ConnectionSnapshot & { instanceId: string | null; resyncing: boolean };
  /** Highest persistent event seq applied; the cursor to continue from after a reload. */
  cursor: { appliedSeq: number };
  drives: { byId: Record<string, Drive>; order: string[]; loaded: boolean; loading: boolean; error: SuperDriveError | null };
  route: SuperDriveRoute;
  folders: Record<FolderKey, FolderPage>;
  files: Record<string, FileRecord>;
  details: LruMap<string, unknown>;
  entities: LruMap<string, unknown>;
  /** Pending review counts per Drive and kind, from `review.changed` and Drive documents. */
  review: {
    pendingByDrive: Record<string, Record<string, number>>;
    groups: Record<string, ReviewList<ReviewGroup>>;
    /** Items per source file or kind, keyed by `reviewKey`. */
    lists: Record<string, ReviewList<ReviewItem>>;
    /** A decision was accepted and the next ontology version is being published (`publish_pending`). */
    publishing: Record<string, boolean>;
  };
  shares: SharesState;
  modelConfig: ModelConfigState;
  constraints: ConstraintsState;
  usage: Record<string, UsageToday>;
  ontology: Record<string, OntologyVersionInfo>;
  notices: Notice[];
  live: LiveState;
  /** The engine action in flight (start, stop, preflight, ...) and the last failure of one, for the engine page. */
  engineAction: { busy: boolean; error: SuperDriveError | null };
  selection: { driveId: string | null; ids: ReadonlySet<string>; anchorId: string | null; allMatching: boolean };
  /**
   * A cold start is reporting thousands of changes at once (F-S13-01): they are folded a slice per turn while this is true, and the
   * rows do not animate. Absent means false.
   */
  flood?: boolean;
};

export const DETAILS_CACHE_LIMIT = 200;
export const ENTITIES_CACHE_LIMIT = 500;

export function createInitialState(): SuperDriveState {
  return {
    engine: null,
    connection: { state: "offline", attempt: 0, nextRetryAt: null, lastSeq: 0, instanceId: null, resyncing: false },
    cursor: { appliedSeq: 0 },
    drives: { byId: {}, order: [], loaded: false, loading: false, error: null },
    route: { ...INITIAL_ROUTE },
    folders: {},
    files: {},
    details: new LruMap(DETAILS_CACHE_LIMIT),
    entities: new LruMap(ENTITIES_CACHE_LIMIT),
    review: { pendingByDrive: {}, groups: {}, lists: {}, publishing: {} },
    shares: createInitialShares(),
    modelConfig: { config: null, loaded: false, loading: false, error: null },
    constraints: { items: [], loaded: false, stale: false },
    usage: {},
    ontology: {},
    notices: [],
    live: { dbRevision: 0, activity: [], fileActors: {} },
    engineAction: { busy: false, error: null },
    selection: { driveId: null, ids: new Set(), anchorId: null, allMatching: false },
  };
}

export function createEmptyFolderPage(): FolderPage {
  return { ids: [], dirs: [], nextCursor: null, total: null, asOfSeq: 0, loading: false, stale: false, error: null };
}

export function createInitialShares(): SharesState {
  return { byId: {}, order: [], chains: {}, loaded: false, loading: false, stale: false, error: null, updates: {}, accessed: {}, pending: {} };
}

export function createEmptyReviewList<T>(): ReviewList<T> {
  return { items: [], nextCursor: null, asOfSeq: 0, loaded: false, loading: false, stale: false, error: null };
}

/** Key of one review listing: the Drive plus the source file and the kind it is filtered by. */
export const reviewKey = (driveId: string, source: string | null = null, kind: string | null = null): string => `${driveId}\u0000${source ?? ""}\u0000${kind ?? ""}`;
