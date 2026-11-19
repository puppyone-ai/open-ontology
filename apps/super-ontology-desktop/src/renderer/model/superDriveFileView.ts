/**
 * One file's full presentation (20-frontend §4.1): the state row plus every refinement the table lists, computed from the
 * file's fields alone. `FileStateBadge`, the details pane and the banners all read this, so a state is described in one place.
 */
import type { FileSummary } from "./contract/docsApi.generated";
import {
  BLOCKED_REASONS, FILE_STATE_PRESENTATION, presentedFileState, type IconName, type PresentedFileState, type Tone,
} from "./superDriveStateModel";

export type FileActionId =
  | "openDetails" | "cancelUpload" | "retryUpload" | "viewQueue" | "viewParsed" | "viewEntities" | "share" | "goToReview" | "viewUsage"
  | "openEngine" | "resumeDrive" | "viewReason" | "retry" | "retrySafe" | "showInFinder" | "learnFormats" | "openOriginal" | "viewCurrent" | "viewLastState"
  | "updateSelected";

export type FileViewVariant =
  | "plain" | "blocked" | "publishedWithReview" | "publishedLimited" | "parseRetrying" | "parseFailed" | "quarantined" | "permissionDenied"
  | "unsupportedWithReason" | "duplicateOf";

export type FileView = {
  state: PresentedFileState;
  variant: FileViewVariant;
  icon: IconName;
  tone: Tone;
  busy: boolean;
  /** Message id and its values; `{reason}` is a message id to resolve, never raw engine text. */
  messageId: string;
  values: Record<string, unknown>;
  /** Message id of the reason (an `errors.*` or blocked reason text) shown in the sentence, or null. */
  reasonMessageId: string | null;
  actions: readonly FileActionId[];
  /** The "retry" button applies (the engine says `retryable`, and the state is one a user retries). */
  canRetry: boolean;
  /** Safe-mode retry is offered (OOM or crash codes). */
  canRetrySafe: boolean;
  /** The engine will retry by itself; no button. */
  autoRetry: boolean;
  /** Pending-review count shown beside the state. */
  reviewCount: number;
  /** The filename is struck through (deleted). */
  strike: boolean;
};

export type FileViewInput = Pick<FileSummary, "state"> & Partial<Pick<FileSummary, "state_code" | "retryable" | "blocked_reason" | "pending_review" | "progress">> & {
  /** Parse attempt number when known (details pane history); the list summary has none. */
  attempt?: number | null;
  /** Percent while the client uploads. */
  uploadPercent?: number;
  /** Name of the file a `duplicate` repeats, when it is known on screen; without it the text says "another file". */
  duplicateName?: string | null;
};

const SAFE_MODE_CODES = new Set(["DOCS_PARSE_OOM", "DOCS_PARSE_CRASH", "DOCS_PARSE_QUARANTINED"]);
export const AUTO_RETRY_LIMIT = 3;
const errorId = (code: string) => `super-drive.errors.${code}`;

const BLOCKED_ACTIONS: Readonly<Record<(typeof BLOCKED_REASONS)[number], FileActionId>> = {
  parser_unavailable: "openEngine", disk_low: "openEngine", db_degraded: "openEngine", llm_limit: "viewUsage", drive_paused: "resumeDrive",
};

const STATE_ACTIONS: Readonly<Record<PresentedFileState, readonly FileActionId[]>> = {
  uploading: ["cancelUpload"], stored: ["openDetails"], queued: ["viewQueue"], parsing: [], parsed: ["viewParsed"], extracting: [], indexed: ["viewEntities"],
  published: ["viewEntities", "share"], parse_failed: ["viewReason", "showInFinder"], extract_partial: ["viewReason"], unsupported: ["learnFormats"],
  too_large: ["showInFinder"], encrypted: ["showInFinder"], duplicate: ["openOriginal"], superseded: ["viewCurrent"], deleted: ["viewLastState"],
  outdated: ["updateSelected"], unknown: ["openDetails"],
};

export function presentFile(input: FileViewInput): FileView {
  const state = input.state === ("uploading" as string) ? "uploading" : presentedFileState(input.state);
  const base = FILE_STATE_PRESENTATION[state];
  const code = input.state_code ?? null;
  const reviewCount = input.pending_review ?? 0;
  const view = (patch: Partial<FileView>): FileView => ({
    state, variant: "plain", icon: base.icon, tone: base.tone, busy: base.busy, messageId: base.messageId,
    values: state === "uploading" ? { percent: `${input.uploadPercent ?? 0}%` } : state === "unknown" ? { state: input.state } : {},
    reasonMessageId: null, actions: STATE_ACTIONS[state], canRetry: false, canRetrySafe: false, autoRetry: false, reviewCount,
    strike: state === "deleted", ...patch,
  });

  if (state === "queued" && input.blocked_reason && (BLOCKED_REASONS as readonly string[]).includes(input.blocked_reason)) {
    const reason = input.blocked_reason as (typeof BLOCKED_REASONS)[number];
    return view({ variant: "blocked", tone: "warning", messageId: "super-drive.file.state.blocked", reasonMessageId: `super-drive.file.state.blocked.${reason}`, actions: [BLOCKED_ACTIONS[reason]] });
  }
  if (state === "published") {
    if (code?.startsWith("DOCS_EXTRACT_LLM_LIMIT_")) return view({ variant: "publishedLimited", messageId: "super-drive.file.state.publishedLimited", actions: ["viewUsage", "viewEntities", "share"] });
    if (reviewCount > 0) {
      return view({ variant: "publishedWithReview", messageId: "super-drive.file.state.publishedWithReview", values: { count: reviewCount }, actions: ["goToReview", "viewEntities", "share"] });
    }
  }
  if (state === "parse_failed") {
    const retryable = input.retryable === true;
    if (retryable && typeof input.attempt === "number" && input.attempt < AUTO_RETRY_LIMIT && code !== "DOCS_PARSE_QUARANTINED") {
      return view({ variant: "parseRetrying", icon: "RotateCw", tone: "warning", messageId: "super-drive.file.state.parseRetrying", values: { attempt: input.attempt }, autoRetry: true, actions: ["viewReason"] });
    }
    if (code === "DOCS_PARSE_QUARANTINED") {
      return view({ variant: "quarantined", icon: "ShieldAlert", messageId: errorId(code), reasonMessageId: errorId(code), canRetry: false, canRetrySafe: true, actions: ["retrySafe", "viewReason"] });
    }
    if (code === "DOCS_SOURCE_PERMISSION_DENIED") {
      return view({ variant: "permissionDenied", icon: "LockKeyhole", messageId: errorId(code), reasonMessageId: errorId(code), canRetry: retryable, actions: ["showInFinder", "retry", "viewReason"] });
    }
    const safe = code !== null && SAFE_MODE_CODES.has(code);
    return view({
      variant: "parseFailed", canRetry: retryable, canRetrySafe: safe, reasonMessageId: code ? errorId(code) : null,
      messageId: code ? "super-drive.file.state.parse_failed.withReason" : base.messageId,
      actions: [...(retryable ? ["retry" as const] : []), ...(safe ? ["retrySafe" as const] : []), "showInFinder", "viewReason"],
    });
  }
  if (state === "duplicate" && input.duplicateName) {
    return view({ variant: "duplicateOf", messageId: "super-drive.file.state.duplicateOf", values: { name: input.duplicateName ?? "" }, actions: ["openOriginal"] });
  }
  if (state === "extract_partial") return view({ canRetry: input.retryable === true, actions: [...(input.retryable === true ? ["retry" as const] : []), "viewReason"] });
  if (state === "unsupported" && code) {
    return view({ variant: "unsupportedWithReason", messageId: "super-drive.file.state.unsupported.withReason", reasonMessageId: errorId(code) });
  }
  return view({});
}
