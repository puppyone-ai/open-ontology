/**
 * Page banners (20-frontend §4.7): which conditions show a banner, in which priority, at most two at a time. A pure
 * function of the engine snapshot, the connection and the active Drive, so the stack and its tests need no UI.
 */
import type { ConnectionSnapshot, EngineSnapshot } from "../../../shared/super-ontology/bridge";
import type { Drive } from "../contract/docsApi.generated";
import type { Tone } from "../superDriveStateModel";

export type BannerKind =
  | "engine_unavailable" | "disk_full" | "mass_delete_held" | "reconnecting" | "resyncing" | "permission_denied" | "sync_failed"
  | "list_incomplete" | "partial_failure" | "disk_low" | "engine_degraded" | "engine_restarting" | "model_limit" | "no_model"
  | "all_processing" | "drive_paused" | "drive_deleting" | "share_drift" | "reprocessing";

export type BannerActionId = "openEngine" | "viewFailures" | "retryAll" | "syncNow" | "confirmMassDelete" | "openUsage" | "resumeDrive" | "viewShares" | "viewOutdated";

export type Banner = {
  kind: BannerKind;
  /** 1 is the most urgent (§4.7). */
  priority: number;
  tone: Tone;
  messageId: string;
  values: Record<string, unknown>;
  actions: readonly BannerActionId[];
  /** The two urgent classes (disk full, held mass delete) cannot be closed. */
  dismissible: boolean;
  /** `role="alert"` once, for a crash; everything else is a polite status. */
  alert: boolean;
};

export const MAX_VISIBLE_BANNERS = 2;
const LOW_DISK_BYTES = 512 * 1024 * 1024;
const UNAVAILABLE_STATUSES = new Set(["crashed", "incompatible", "docker_missing", "docker_stopped", "embedded_missing", "oo_missing", "stopped"]);

export type BannerInput = {
  engine: EngineSnapshot | null;
  connection: (ConnectionSnapshot & { resyncing?: boolean }) | null;
  drive: Drive | null;
  /** Some Drive data is already on screen (banners are for "the engine went away", not for a first start). */
  hasData: boolean;
  /** Live Shares that need a refresh (D50). */
  driftedShares?: number;
  /** The model's daily call limit is reached for this Drive (`usage.changed`). */
  usageLimited?: boolean;
};

const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : 0);
const sum = (counts: Record<string, unknown> | undefined, ...states: string[]) => states.reduce((total, state) => total + num(counts?.[state]), 0);

function make(kind: BannerKind, priority: number, tone: Tone, actions: readonly BannerActionId[] = [], extra: Partial<Banner> = {}): Banner {
  return { kind, priority, tone, messageId: `super-drive.banner.${kind}`, values: {}, actions, dismissible: true, alert: false, ...extra };
}

export function computeBanners({ engine, connection, drive, hasData, driftedShares = 0, usageLimited = false }: BannerInput): Banner[] {
  const banners: Banner[] = [];
  const status = engine?.status ?? "idle";
  const health = (engine?.health ?? null) as { components?: Record<string, { status?: string; code?: string | null } | undefined>; disk?: { free_bytes?: number } } | null;
  const codes = new Set(Object.values(health?.components ?? {}).map((component) => component?.code).filter((code): code is string => typeof code === "string"));
  const freeBytes = health?.disk?.free_bytes;
  const lastSync = drive?.last_sync ?? null;
  const driveCode = drive?.status_code ?? null;

  if (hasData && UNAVAILABLE_STATUSES.has(status)) banners.push(make("engine_unavailable", 1, "danger", ["openEngine"], { alert: status === "crashed" }));

  if (codes.has("DOCS_ENGINE_DISK_FULL") || driveCode === "DOCS_ENGINE_DISK_FULL" || freeBytes === 0) banners.push(make("disk_full", 2, "danger", ["openEngine"], { dismissible: false }));
  if (lastSync?.status === "held") {
    const removed = num(lastSync.removed);
    banners.push(make("mass_delete_held", 2, "warning", ["confirmMassDelete"], {
      dismissible: false, values: { generation: lastSync.generation ?? 0, count: removed },
      ...(removed > 0 ? { messageId: "super-drive.banner.mass_delete_held_count" } : {}),
    }));
  }
  if (drive?.status === "deleting") banners.push(make("drive_deleting", 2, "info", [], { dismissible: false }));

  if (hasData && (connection?.resyncing || connection?.state === "resyncing")) banners.push(make("resyncing", 3, "info"));
  else if (hasData && connection && connection.state === "reconnecting") banners.push(make("reconnecting", 3, "warning"));

  if (driveCode === "DOCS_SOURCE_PERMISSION_DENIED" || driveCode === "DOCS_SOURCE_ROOT_MISSING") {
    const root = typeof drive?.config?.root === "string" ? drive.config.root : "";
    banners.push(make("permission_denied", 4, "danger", ["syncNow"], { values: { path: root }, ...(root ? { messageId: "super-drive.banner.permission_denied_path" } : {}) }));
  }
  if (lastSync?.status === "failed") banners.push(make("sync_failed", 4, "danger", ["syncNow"], { values: { code: lastSync.error_code ?? "" } }));
  if (lastSync?.error_code === "DOCS_SOURCE_LIST_INCOMPLETE" || driveCode === "DOCS_SOURCE_LIST_INCOMPLETE") banners.push(make("list_incomplete", 4, "warning", ["syncNow"]));

  const failed = sum(drive?.counts, "parse_failed", "extract_partial");
  if (failed > 0) banners.push(make("partial_failure", 5, "warning", ["viewFailures", "retryAll"], { values: { count: failed } }));

  if (!codes.has("DOCS_ENGINE_DISK_FULL") && (codes.has("DOCS_ENGINE_DISK_LOW") || (typeof freeBytes === "number" && freeBytes > 0 && freeBytes < LOW_DISK_BYTES))) banners.push(make("disk_low", 6, "warning", ["openEngine"], {
    ...(typeof freeBytes === "number" ? { values: { freeBytes }, messageId: "super-drive.banner.disk_low_free" } : {}),
  }));
  if (status === "degraded") banners.push(make("engine_degraded", 6, "warning", ["openEngine"]));
  if (status === "restarting" && hasData) banners.push(make("engine_restarting", 6, "warning"));

  if (driftedShares > 0) banners.push(make("share_drift", 7, "warning", ["viewShares"], { values: { count: driftedShares } }));
  if (driveCode?.startsWith("DOCS_EXTRACT_LLM_LIMIT_") === true || usageLimited) banners.push(make("model_limit", 8, "info", ["openUsage"]));
  if (health?.components?.model?.status === "not_configured" && (drive?.files_total ?? 0) > 0 && drive?.model_enabled !== true) banners.push(make("no_model", 8, "neutral"));
  const processing = sum(drive?.counts, "stored", "queued", "parsing", "parsed", "extracting", "indexed");
  if ((drive?.files_total ?? 0) > 0 && processing === drive?.files_total) banners.push(make("all_processing", 8, "info", [], { values: { count: processing } }));
  const outdated = sum(drive?.counts, "outdated");
  if (outdated > 0) banners.push(make("reprocessing", 8, "info", ["viewOutdated"], { values: { count: outdated } }));
  if (drive?.status === "paused") banners.push(make("drive_paused", 8, "neutral", ["resumeDrive"]));

  // Stable: by priority, then in the order found (which is the order of appearance of the checks above).
  return banners.map((banner, index) => ({ banner, index })).sort((a, b) => a.banner.priority - b.banner.priority || a.index - b.index).map(({ banner }) => banner);
}

/** Why write actions (upload, retry, sync) are off right now: a full disk or a Drive being removed (§4.6). */
export type WriteBlock = "disk_full" | "drive_deleting";

export function writeBlockOf(banners: readonly Banner[]): WriteBlock | null {
  if (banners.some((banner) => banner.kind === "disk_full")) return "disk_full";
  if (banners.some((banner) => banner.kind === "drive_deleting")) return "drive_deleting";
  return null;
}

/** What to show: at most two, the rest counted. Dismissed banners are skipped unless the class cannot be closed. */
export function visibleBanners(banners: readonly Banner[], dismissed: ReadonlySet<BannerKind>): { shown: Banner[]; hidden: Banner[] } {
  const open = banners.filter((banner) => !banner.dismissible || !dismissed.has(banner.kind));
  return { shown: open.slice(0, MAX_VISIBLE_BANNERS), hidden: open.slice(MAX_VISIBLE_BANNERS) };
}

/** A banner dismissed earlier shows again once its condition went away and came back: keep only still-present kinds. */
export function pruneDismissed(dismissed: ReadonlySet<BannerKind>, banners: readonly Banner[]): ReadonlySet<BannerKind> {
  const present = new Set(banners.map((banner) => banner.kind));
  const kept = [...dismissed].filter((kind) => present.has(kind));
  return kept.length === dismissed.size ? dismissed : new Set(kept);
}
