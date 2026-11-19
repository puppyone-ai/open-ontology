/**
 * Single source of truth for presentation (20-frontend §4): which icon, tone,
 * message and actions each state gets. Components render from here and never
 * decide per state themselves, so the state matrix and the UI cannot drift.
 */
import type { EngineSnapshot } from "../../shared/super-ontology/bridge";
import { ENGINE_STATUSES } from "../../shared/super-ontology/engine-contract.mjs";
import { FILE_STATE_VALUES, FRESHNESS_STATE_VALUES, type FileState, type FreshnessState } from "../../shared/super-ontology/docs-api.generated";

export type Tone = "neutral" | "info" | "success" | "warning" | "danger";

/** Icon names are lucide-react exports; `SuperDriveIcon` maps them to components. */
export type IconName =
  | "Server" | "LoaderCircle" | "Monitor" | "SquareTerminal" | "Box" | "CircleCheck" | "TriangleAlert"
  | "RotateCw" | "CircleAlert" | "ShieldAlert" | "CircleHelp"
  | "CloudUpload" | "File" | "Clock3" | "FileText" | "ListTree" | "CircleSlash2" | "FileWarning" | "LockKeyhole"
  | "CopyPlus" | "History" | "Trash2" | "RefreshCw" | "ListChecks" | "Upload" | "Pause";

export type EngineActionId =
  | "checkEnvironment" | "start" | "stop" | "restart" | "retry" | "installOo" | "chooseOo"
  | "openDockerDownload" | "retryAfterDockerInstall" | "retryAfterDockerStart" | "viewLogs" | "openStateFolder" | "useDocker";

export type EnginePresentation = {
  status: EngineSnapshot["status"];
  /** Full-sentence message (engine page and entry state). */
  messageId: string;
  values: Record<string, unknown>;
  /** Short message for the pill. */
  pillId: string;
  tone: Tone;
  icon: IconName;
  /** The status animates (a spinner glyph). */
  busy: boolean;
  primary: EngineActionId | null;
  secondary: readonly EngineActionId[];
  /** Error code whose message explains a `crashed` or failed start. */
  reasonCode: string | null;
};

type Row = {
  icon: IconName;
  tone: Tone;
  busy?: boolean;
  primary: EngineActionId | null;
  secondary?: readonly EngineActionId[];
};

/** §4.4 rows; `restarting` and `degraded` refine the message below. */
const ROWS: Readonly<Record<EngineSnapshot["status"], Row>> = {
  idle: { icon: "Server", tone: "neutral", primary: "checkEnvironment" },
  probing: { icon: "LoaderCircle", tone: "neutral", busy: true, primary: null },
  unsupported_platform: { icon: "Monitor", tone: "neutral", primary: null },
  oo_missing: { icon: "SquareTerminal", tone: "warning", primary: "installOo", secondary: ["chooseOo", "retry"] },
  docker_missing: { icon: "Box", tone: "warning", primary: "openDockerDownload", secondary: ["retryAfterDockerInstall"] },
  docker_stopped: { icon: "Box", tone: "warning", primary: "retryAfterDockerStart" },
  // The Docker-free runtime (F-S14-01) is chosen but this build does not carry its embedded database: check again, or use Docker.
  embedded_missing: { icon: "Box", tone: "warning", primary: "retry", secondary: ["useDocker"] },
  stopped: { icon: "Server", tone: "neutral", primary: "start" },
  starting: { icon: "LoaderCircle", tone: "info", busy: true, primary: null, secondary: ["viewLogs"] },
  healthy: { icon: "CircleCheck", tone: "success", primary: null, secondary: ["stop", "restart"] },
  degraded: { icon: "TriangleAlert", tone: "warning", primary: null, secondary: ["restart", "viewLogs"] },
  restarting: { icon: "RotateCw", tone: "warning", primary: null, secondary: ["restart", "viewLogs"] },
  crashed: { icon: "CircleAlert", tone: "danger", primary: "retry", secondary: ["viewLogs", "openStateFolder"] },
  incompatible: { icon: "ShieldAlert", tone: "danger", primary: "chooseOo", secondary: ["viewLogs"] },
  stopping: { icon: "LoaderCircle", tone: "neutral", busy: true, primary: null },
};

export const ENGINE_STATUS_ORDER = ENGINE_STATUSES as readonly EngineSnapshot["status"][];

export const COMPONENT_NAMES = ["db", "parser", "model", "cas", "engine"] as const;
export type ComponentName = (typeof COMPONENT_NAMES)[number];
export const COMPONENT_STATUS_VALUES = ["ok", "starting", "degraded", "unavailable", "not_configured", "limited", "blocked"] as const;

/** A component that makes the engine look degraded. An unconfigured model does not (D18: it must work without one). */
export function degradedComponents(health: EngineSnapshot["health"]): ComponentName[] {
  const components = (health?.components ?? {}) as Record<string, { status?: string } | undefined>;
  return COMPONENT_NAMES.filter((name) => {
    const status = components[name]?.status;
    return status !== undefined && !["ok", "starting", "not_configured"].includes(status);
  });
}

export const COMPONENT_TONES: Readonly<Record<(typeof COMPONENT_STATUS_VALUES)[number], Tone>> = {
  ok: "success", starting: "info", degraded: "warning", unavailable: "danger", not_configured: "neutral", limited: "warning", blocked: "warning",
};

export function presentEngine(snapshot: EngineSnapshot | null): EnginePresentation {
  const status: EngineSnapshot["status"] = snapshot?.status ?? "idle";
  const row = ROWS[status] ?? ROWS.idle;
  let messageId = `super-drive.engine.status.${status}`;
  const values: Record<string, unknown> = {};
  if (status === "restarting") {
    if (snapshot?.reason === "drive_change") messageId = "super-drive.engine.status.applyingDriveChange";
    else if (snapshot?.reason === "process_exit") {
      messageId = "super-drive.engine.status.restarting";
      values.attempt = snapshot.restarts.count;
      values.seconds = snapshot.restarts.nextAttemptAt
        ? Math.max(0, Math.round((Date.parse(snapshot.restarts.nextAttemptAt) - Date.now()) / 1000))
        : 0;
    } else messageId = "super-drive.engine.status.restartingContainer";
  }
  if (status === "degraded") values.component = degradedComponents(snapshot?.health ?? null);
  const reasonCode = status === "crashed" || status === "incompatible" ? snapshot?.code ?? null : null;
  const tone: Tone = status === "restarting" && snapshot?.reason === "drive_change" ? "info" : row.tone;
  return {
    status, messageId, values, pillId: `super-drive.engine.pill.${status}`, tone, icon: row.icon,
    busy: row.busy === true, primary: row.primary, secondary: row.secondary ?? [], reasonCode,
  };
}

/** Engine states in which the Drive data can be read and written. */
export const isEngineReady = (snapshot: EngineSnapshot | null): boolean => snapshot?.status === "healthy" || snapshot?.status === "degraded";

// ---- file states (§4.1) -----------------------------------------------------------

/** `uploading` exists only on the client (S3); `unknown` is any state value this app does not know yet. */
export type PresentedFileState = FileState | "uploading" | "unknown";
export type FileStateGroup = "processing" | "published" | "failed" | "skipped" | "history";

export type FileStatePresentation = {
  icon: IconName;
  tone: Tone;
  /** The state animates (spinner glyph). */
  busy: boolean;
  group: FileStateGroup | null;
  /** Full-text message id; `{done}`/`{total}` come from `progress`, `{percent}` for uploading. */
  messageId: string;
};

const row = (icon: IconName, tone: Tone, group: FileStateGroup | null, messageId: string, busy = false): FileStatePresentation =>
  ({ icon, tone, busy, group, messageId: `super-drive.file.state.${messageId}` });

export const FILE_STATE_PRESENTATION: Readonly<Record<PresentedFileState, FileStatePresentation>> = {
  uploading: row("CloudUpload", "info", "processing", "uploading"),
  stored: row("File", "neutral", "processing", "stored"),
  queued: row("Clock3", "neutral", "processing", "queued"),
  parsing: row("LoaderCircle", "info", "processing", "parsing", true),
  parsed: row("FileText", "info", "processing", "parsed"),
  extracting: row("LoaderCircle", "info", "processing", "extracting", true),
  indexed: row("ListTree", "info", "processing", "indexed"),
  published: row("CircleCheck", "success", "published", "published"),
  parse_failed: row("CircleAlert", "danger", "failed", "parse_failed"),
  extract_partial: row("TriangleAlert", "warning", "failed", "extract_partial"),
  unsupported: row("CircleSlash2", "neutral", "skipped", "unsupported"),
  too_large: row("FileWarning", "warning", "skipped", "too_large"),
  encrypted: row("LockKeyhole", "warning", "skipped", "encrypted"),
  duplicate: row("CopyPlus", "neutral", "published", "duplicate"),
  superseded: row("History", "neutral", "history", "superseded"),
  deleted: row("Trash2", "neutral", "history", "deleted"),
  outdated: row("RefreshCw", "info", "history", "outdated"),
  unknown: row("CircleHelp", "neutral", null, "unknown"),
};

export const FILE_STATE_GROUPS: Readonly<Record<FileStateGroup, readonly FileState[]>> = {
  processing: ["stored", "queued", "parsing", "parsed", "extracting", "indexed"],
  published: ["published", "duplicate"],
  failed: ["parse_failed", "extract_partial"],
  skipped: ["unsupported", "too_large", "encrypted"],
  history: ["superseded", "deleted", "outdated"],
};

export const BLOCKED_REASONS = ["parser_unavailable", "disk_low", "db_degraded", "llm_limit", "drive_paused"] as const;

/** Maps an engine value to a presented state; an unknown value stays readable as "unknown" with its raw text. */
export function presentedFileState(state: string | undefined | null): PresentedFileState {
  return (FILE_STATE_VALUES as readonly string[]).includes(state ?? "") ? (state as FileState) : "unknown";
}

// ---- freshness (00 §5.5, D48) -------------------------------------------------------

export type FreshnessPresentation = { icon: IconName; tone: Tone; busy: boolean; messageId: string };

const fresh = (icon: IconName, tone: Tone, messageId: string, busy = false): FreshnessPresentation =>
  ({ icon, tone, busy, messageId: `super-drive.freshness.${messageId}` });

export const FRESHNESS_PRESENTATION: Readonly<Record<FreshnessState | "unknown", FreshnessPresentation>> = {
  up_to_date: fresh("CircleCheck", "success", "up_to_date"),
  syncing: fresh("RefreshCw", "info", "syncing", true),
  processing: fresh("LoaderCircle", "info", "processing", true),
  publishing: fresh("Upload", "info", "publishing", true),
  waiting_review: fresh("ListChecks", "warning", "waiting_review"),
  degraded: fresh("TriangleAlert", "warning", "degraded"),
  held: fresh("Pause", "danger", "held"),
  unknown: fresh("CircleHelp", "neutral", "unknown"),
};

/** When several conditions hold, the first of these is shown (00 §5.5). */
export const FRESHNESS_PRIORITY: readonly FreshnessState[] = ["held", "degraded", "syncing", "processing", "publishing", "waiting_review", "up_to_date"];

export function presentedFreshnessState(state: string | undefined | null): FreshnessState | "unknown" {
  return (FRESHNESS_STATE_VALUES as readonly string[]).includes(state ?? "") ? (state as FreshnessState) : "unknown";
}

/** The freshness a Drive-level summary shows: the highest-priority state among Drives (unknown values never win). */
export function highestPriorityFreshness(states: ReadonlyArray<string | undefined | null>): FreshnessState | null {
  for (const candidate of FRESHNESS_PRIORITY) if (states.includes(candidate)) return candidate;
  return null;
}

/**
 * What a folder row shows (§7.3 F-S3-08, "folder roll-up"). The engine reports freshness per Drive and review counts per
 * folder, so a folder takes the Drive's own state (held, degraded, syncing, processing and publishing hold for everything
 * under it) and waiting review when something below it waits; the higher priority of the two wins. `up_to_date` means no mark.
 */
export function rollUpFolderFreshness(driveState: string | undefined | null, dirCounts: { pending_review?: number } | undefined): FreshnessState | "unknown" | null {
  const drive = driveState ? presentedFreshnessState(driveState) : null;
  if (drive === "unknown") return "unknown";
  const states: FreshnessState[] = [];
  // Waiting for review is a fact about files, so a folder shows it only when its own count says so.
  if (drive && drive !== "waiting_review") states.push(drive);
  if ((dirCounts?.pending_review ?? 0) > 0) states.push("waiting_review");
  const top = highestPriorityFreshness(states);
  return top === null || top === "up_to_date" ? null : top;
}

// ---- filter groups (§4.1 last paragraph) ----------------------------------------------

/** Order of the filter menu; "other" holds states this app does not know yet. */
export const FILE_FILTER_GROUPS = ["processing", "published", "failed", "skipped", "history", "other"] as const;
export type FileFilterGroup = (typeof FILE_FILTER_GROUPS)[number];

export function fileGroupOf(state: string | undefined | null): FileFilterGroup {
  return FILE_STATE_PRESENTATION[presentedFileState(state)].group ?? "other";
}

/** `state=` values for the chosen groups; "other" has none (the engine cannot filter by an unknown value), so it is client-side only. */
export function statesOfGroups(groups: readonly FileFilterGroup[]): FileState[] {
  return groups.flatMap((group) => (group === "other" ? [] : [...FILE_STATE_GROUPS[group]]));
}
