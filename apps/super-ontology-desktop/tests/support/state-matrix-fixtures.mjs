// One fixture per row of the file state matrix (20-frontend §4.1), shared by the contract test and the visual smoke so that
// "every row has a screenshot" and "every contract state has a row" are the same list. Texts are the English catalog.
export const DRIVE_ID = "00000000-0000-4000-8000-0000000000d1";
export const ORIGINAL_ID = "00000000-0000-4000-8000-0000000000a0";

/** `contractState`: the FileState value the row stands for; `view`: how §4.1 names the refinement. `text`: substring of the row. */
export const STATE_MATRIX_ROWS = Object.freeze([
  { key: "uploading", file: { state: "uploading" }, text: "Uploading", contractState: null },
  { key: "stored", file: { state: "stored" }, text: "Stored", contractState: "stored" },
  { key: "queued", file: { state: "queued" }, text: "Queued", contractState: "queued" },
  { key: "blocked-parser_unavailable", file: { state: "queued", blocked_reason: "parser_unavailable" }, text: "Waiting: Parser unavailable", contractState: "queued" },
  { key: "blocked-disk_low", file: { state: "queued", blocked_reason: "disk_low" }, text: "Waiting: Low disk space", contractState: "queued" },
  { key: "blocked-db_degraded", file: { state: "queued", blocked_reason: "db_degraded" }, text: "Waiting: Database unavailable", contractState: "queued" },
  { key: "blocked-llm_limit", file: { state: "queued", blocked_reason: "llm_limit" }, text: "Waiting: Model rate-limited, will resume", contractState: "queued" },
  { key: "blocked-drive_paused", file: { state: "queued", blocked_reason: "drive_paused" }, text: "Waiting: Drive paused", contractState: "queued" },
  { key: "parsing", file: { state: "parsing", progress: { unit: "page", done: 3, total: 12 } }, text: "Parsing", contractState: "parsing" },
  { key: "parsed", file: { state: "parsed" }, text: "Parsed", contractState: "parsed" },
  { key: "extracting", file: { state: "extracting", progress: { unit: "batch", done: 2, total: 5 } }, text: "Extracting", contractState: "extracting" },
  { key: "indexed", file: { state: "indexed" }, text: "Indexed, publishing", contractState: "indexed" },
  { key: "published", file: { state: "published" }, text: "Published", contractState: "published" },
  { key: "published-review", file: { state: "published", pending_review: 2 }, text: "Published · 2 to review", contractState: "published" },
  { key: "published-limited", file: { state: "published", state_code: "DOCS_EXTRACT_LLM_LIMIT_DAILY" }, text: "Published · model limit reached", contractState: "published" },
  { key: "parse_failed-retrying", file: { state: "parse_failed", retryable: true, state_code: "DOCS_PARSE_TIMEOUT" }, text: "Parse failed", contractState: "parse_failed", attempt: 1, detailsText: "Parse failed, retrying automatically (1 of 3)" },
  { key: "parse_failed", file: { state: "parse_failed", retryable: true, state_code: "DOCS_PARSE_CORRUPT" }, text: "Parse failed · The file is damaged", contractState: "parse_failed" },
  { key: "parse_failed-quarantined", file: { state: "parse_failed", retryable: true, state_code: "DOCS_PARSE_QUARANTINED" }, text: "Quarantined after repeatedly crashing the parser", contractState: "parse_failed" },
  { key: "parse_failed-permission", file: { state: "parse_failed", retryable: true, state_code: "DOCS_SOURCE_PERMISSION_DENIED" }, text: "No permission to read", contractState: "parse_failed" },
  { key: "extract_partial", file: { state: "extract_partial", retryable: true }, text: "Partly extracted", contractState: "extract_partial" },
  { key: "unsupported", file: { state: "unsupported", state_code: "DOCS_PARSE_UNSUPPORTED_FORMAT" }, text: "Not supported", contractState: "unsupported" },
  { key: "too_large", file: { state: "too_large" }, text: "Over the size limit", contractState: "too_large" },
  { key: "encrypted", file: { state: "encrypted" }, text: "Encrypted, can't be read", contractState: "encrypted" },
  { key: "duplicate", file: { state: "duplicate", duplicate_of: ORIGINAL_ID }, text: "Same as original.md; results reused", contractState: "duplicate" },
  { key: "superseded", file: { state: "superseded" }, text: "Replaced by a newer version", contractState: "superseded" },
  { key: "deleted", file: { state: "deleted" }, text: "Removed from the Drive · evidence withdrawn", contractState: "deleted" },
  { key: "outdated", file: { state: "outdated" }, text: "Update available", contractState: "outdated" },
  { key: "unknown", file: { state: "melting" }, text: "Unknown state (melting)", contractState: null },
]);

/** The seven freshness states and an unknown one, with what the toolbar shows (§7.3 F-S3-08). */
export const FRESHNESS_MATRIX_ROWS = Object.freeze([
  { key: "up_to_date", freshness: { state: "up_to_date" }, text: "Up to date · v7" },
  { key: "syncing", freshness: { state: "syncing" }, text: "Reading changes" },
  { key: "processing", freshness: { state: "processing", processing: 4 }, text: "Processing 4 files" },
  { key: "publishing", freshness: { state: "publishing" }, text: "Publishing a new version" },
  { key: "waiting_review", freshness: { state: "waiting_review", pending_review: 3 }, text: "3 items to review" },
  { key: "degraded", freshness: { state: "degraded", reason_code: "DOCS_ENGINE_DISK_LOW" }, text: "Processing is limited" },
  { key: "held", freshness: { state: "held" }, text: "Cleanup is paused until you confirm" },
  { key: "unknown", freshness: { state: "melting" }, text: "Freshness unknown (melting)" },
]);

export function fileSummary(row, index) {
  const name = `${String(index + 1).padStart(2, "0")}-${row.key}.md`;
  return {
    id: `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`, drive_id: DRIVE_ID, path: name, name, size: 2048 + index * 512,
    format: "md", state_since: new Date(Date.now() - (index + 1) * 60_000).toISOString(), retryable: false, pending_review: 0, ...row.file,
  };
}

export function matrixDrive(freshness = { state: "up_to_date" }) {
  return {
    id: DRIVE_ID, name: "State matrix", backend: "fs", config: { root: "/tmp/state-matrix" }, status: "active", revision: 1,
    counts: {}, files_total: STATE_MATRIX_ROWS.length + 1, created_at: "2026-11-17T00:00:00Z",
    freshness: { ontology_version: 7, as_of: new Date().toISOString(), processing: 0, pending_review: 0, reason_code: null, ...freshness },
  };
}

/** The document the fake engine serves: every matrix file in the root folder, plus the file a duplicate points at. */
export function matrixDocs(freshness) {
  const files = STATE_MATRIX_ROWS.map(fileSummary);
  files.push({ ...fileSummary({ key: "original", file: { state: "published" } }, 99), id: ORIGINAL_ID, path: "original.md", name: "original.md" });
  // The automatic retry is told by the attempt of the last history entry, so only the details can show it.
  const history = Object.fromEntries(STATE_MATRIX_ROWS.flatMap((row, index) => (row.attempt === undefined ? [] : [[fileSummary(row, index).id, [{ seq: 1, to: "parse_failed", code: row.file.state_code, attempt: row.attempt, at: new Date().toISOString() }]]])));
  return { drives: [matrixDrive(freshness)], files, dirs: [], history, as_of_seq: 5 };
}
