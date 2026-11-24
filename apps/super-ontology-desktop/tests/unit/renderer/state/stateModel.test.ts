import { describe, expect, it } from "vitest";
import { FILE_STATE_VALUES, type FileState } from "@so-contract/docs-api.generated";
import { presentFile } from "@so-model/superDriveFileView";
import {
  FILE_FILTER_GROUPS, FILE_STATE_GROUPS, FILE_STATE_PRESENTATION, fileGroupOf, statesOfGroups,
} from "@so-model/superDriveStateModel";

const file = (state: string, extra: Record<string, unknown> = {}) => presentFile({ state: state as FileState, retryable: false, pending_review: 0, ...extra });

describe("full file state matrix (§4.1)", () => {
  it("describes all 16 contract states, uploading and unknown, with a tone, an icon and actions", () => {
    expect(FILE_STATE_VALUES).toHaveLength(16);
    for (const state of [...FILE_STATE_VALUES, "uploading", "melting"]) {
      const view = file(state);
      expect(view.messageId, state).toMatch(/^super-drive\.file\.state\./u);
      expect(view.icon, state).toBeTruthy();
      expect(FILE_STATE_PRESENTATION[view.state], state).toBeDefined();
    }
    expect(file("melting", { state: "melting" })).toMatchObject({ state: "unknown", values: { state: "melting" } });
    expect(file("uploading").values).toEqual({ percent: "0%" });
    expect(presentFile({ state: "uploading" as never, uploadPercent: 42 }).values).toEqual({ percent: "42%" });
  });

  it("shows why a queued file waits, with the action that fits the reason", () => {
    const view = file("queued", { blocked_reason: "parser_unavailable" });
    expect(view).toMatchObject({ variant: "blocked", tone: "warning", reasonMessageId: "super-drive.file.state.blocked.parser_unavailable", actions: ["openEngine"] });
    expect(file("queued", { blocked_reason: "llm_limit" }).actions).toEqual(["viewUsage"]);
    expect(file("queued", { blocked_reason: "drive_paused" }).actions).toEqual(["resumeDrive"]);
    expect(file("queued", { blocked_reason: "disk_low" }).actions).toEqual(["openEngine"]);
    expect(file("queued", { blocked_reason: "something_new" }).variant).toBe("plain");
    expect(file("stored", { blocked_reason: "disk_low" }).variant).toBe("plain");
  });

  it("counts reviews on a published file and notes a model limit", () => {
    expect(file("published", { pending_review: 3 })).toMatchObject({ variant: "publishedWithReview", values: { count: 3 }, tone: "success" });
    expect(file("published", { state_code: "DOCS_EXTRACT_LLM_LIMIT_FILE" })).toMatchObject({ variant: "publishedLimited", messageId: "super-drive.file.state.publishedLimited" });
    expect(file("published", { state_code: "DOCS_EXTRACT_LLM_LIMIT_DRIVE", pending_review: 2 }).variant).toBe("publishedLimited");
    expect(file("published")).toMatchObject({ variant: "plain", reviewCount: 0 });
  });

  it("separates automatic retry, manual retry, safe mode, quarantine and permission", () => {
    expect(file("parse_failed", { retryable: true, attempt: 1, state_code: "DOCS_PARSE_TIMEOUT" })).toMatchObject({ variant: "parseRetrying", autoRetry: true, canRetry: false, values: { attempt: 1 } });
    const manual = file("parse_failed", { retryable: true, attempt: 3, state_code: "DOCS_PARSE_TIMEOUT" });
    expect(manual).toMatchObject({ variant: "parseFailed", canRetry: true, canRetrySafe: false, reasonMessageId: "super-drive.errors.DOCS_PARSE_TIMEOUT" });
    expect(file("parse_failed", { retryable: true, state_code: "DOCS_PARSE_OOM" })).toMatchObject({ canRetry: true, canRetrySafe: true });
    expect(file("parse_failed", { retryable: true, state_code: "DOCS_PARSE_CRASH" }).actions).toContain("retrySafe");
    expect(file("parse_failed", { retryable: true, attempt: 1, state_code: "DOCS_PARSE_QUARANTINED" })).toMatchObject({ variant: "quarantined", icon: "ShieldAlert", canRetry: false, canRetrySafe: true });
    expect(file("parse_failed", { retryable: true, state_code: "DOCS_SOURCE_PERMISSION_DENIED" })).toMatchObject({ variant: "permissionDenied", icon: "LockKeyhole", canRetry: true });
    expect(file("parse_failed", { retryable: false, state_code: "DOCS_PARSE_CORRUPT" })).toMatchObject({ canRetry: false, tone: "danger" });
    expect(file("parse_failed", { retryable: true })).toMatchObject({ messageId: "super-drive.file.state.parse_failed", reasonMessageId: null });
  });

  it("keeps partial extraction retryable and skipped states explained", () => {
    expect(file("extract_partial", { retryable: true })).toMatchObject({ canRetry: true, tone: "warning" });
    expect(file("unsupported", { state_code: "DOCS_PARSE_SCANNED_PDF" })).toMatchObject({ variant: "unsupportedWithReason", reasonMessageId: "super-drive.errors.DOCS_PARSE_SCANNED_PDF" });
    expect(file("unsupported").variant).toBe("plain");
    expect(file("deleted").strike).toBe(true);
    expect(file("duplicate").actions).toEqual(["openOriginal"]);
    expect(file("too_large").tone).toBe("warning");
    expect(file("encrypted").tone).toBe("warning");
  });
});

describe("filter groups", () => {
  it("lists five groups plus other, maps states and leaves unknown values to other", () => {
    expect(FILE_FILTER_GROUPS).toEqual(["processing", "published", "failed", "skipped", "history", "other"]);
    expect(fileGroupOf("duplicate")).toBe("published");
    expect(fileGroupOf("encrypted")).toBe("skipped");
    expect(fileGroupOf("melting")).toBe("other");
    expect(statesOfGroups(["failed", "skipped"])).toEqual([...FILE_STATE_GROUPS.failed, ...FILE_STATE_GROUPS.skipped]);
    expect(statesOfGroups(["other"])).toEqual([]);
  });
});
