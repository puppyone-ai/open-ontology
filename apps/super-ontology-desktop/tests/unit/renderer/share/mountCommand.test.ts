import { describe, expect, it } from "vitest";
import { buildMountCommand, formatExpiry, reconcileCommand } from "@so-model/share/mountCommand";
import { clampBudget, compileInput, DEFAULT_SETTINGS, isValidScopePath, validateSettings } from "@so-model/share/shareSettings";

const drive = { id: "00000000-0000-4000-8000-000000000001", name: "Docs" };
const base = { drive, scope: ["Projects/a"], to: "/Users/example/tasks/proj-a", task: "Follow up plan two", budget: 800, role: "suggester" as const, expiresInSeconds: 604_800 };

describe("mount command (10 section 7.9)", () => {
  it("writes the documented line: drive, scopes, target, task, budget, role, expiry", () => {
    expect(buildMountCommand(base)).toBe("oo context mount --drive Docs --scope Projects/a --to /Users/example/tasks/proj-a --for 'Follow up plan two' --budget 800 --role suggester --expires 7d");
  });

  it("uses the Drive id when the name has spaces or non-ASCII characters", () => {
    expect(buildMountCommand({ ...base, drive: { ...drive, name: "我的资料" } })).toContain(`--drive ${drive.id}`);
    expect(buildMountCommand({ ...base, drive: { ...drive, name: "My docs" } })).toContain(`--drive ${drive.id}`);
  });

  it("repeats --scope per path, leaves it out for the whole Drive and quotes paths with spaces", () => {
    expect(buildMountCommand({ ...base, scope: ["a", "b c"] })).toContain("--scope a --scope 'b c'");
    expect(buildMountCommand({ ...base, scope: [] })).not.toContain("--scope");
  });

  it("escapes quotes in the task and keeps a placeholder until a folder is picked", () => {
    expect(buildMountCommand({ ...base, task: "it's done", to: null })).toContain("--to '<folder>' --for 'it'\\''s done'");
  });

  it("adds the optional flags only when they differ from the defaults", () => {
    const plain = buildMountCommand(base);
    expect(plain).not.toMatch(/--group|--refresh-policy|--require-complete|--version/u);
    const full = buildMountCommand({ ...base, group: "T-1042", refreshPolicy: "at-checkpoint", requireComplete: true, version: 12 });
    expect(full).toContain("--version 12 --group T-1042 --refresh-policy at-checkpoint --require-complete");
    expect(buildMountCommand({ ...base, refreshPolicy: "manual" })).not.toContain("--refresh-policy");
  });

  it("formats expiries as the largest whole unit", () => {
    expect([86_400, 259_200, 604_800, 2_592_000, 7_200, 5_400].map(formatExpiry)).toEqual(["1d", "3d", "7d", "30d", "2h", "90m"]);
  });

  it("prefers the engine's own command when it differs and says so", () => {
    expect(reconcileCommand("a", undefined)).toEqual({ command: "a", mismatch: false });
    expect(reconcileCommand("a", "a ")).toEqual({ command: "a ", mismatch: false });
    expect(reconcileCommand("a", "b")).toEqual({ command: "b", mismatch: true });
  });
});

describe("share settings", () => {
  it("defaults to a suggester, 7 days and 800 tokens", () => {
    expect(DEFAULT_SETTINGS).toMatchObject({ role: "suggester", expiresInSeconds: 604_800, budget: 800, scope: [], refreshPolicy: "manual", requireComplete: false });
  });

  it("builds the one compile request that both the preview and the mount send", () => {
    expect(compileInput("d", { ...DEFAULT_SETTINGS, scope: ["a"], task: "  t ", requireComplete: true })).toEqual({ driveId: "d", scope: ["a"], task: "t", budget: 800, role: "suggester", requireComplete: true });
    expect(compileInput("d", DEFAULT_SETTINGS, { waitMs: 20_000 })).toEqual({ driveId: "d", budget: 800, role: "suggester", waitMs: 20_000 });
  });

  it("accepts only contract drive paths", () => {
    expect(["a", "a/b", "a b/c"].map(isValidScopePath)).toEqual([true, true, true]);
    expect(["", "/a", "a//b", "a/../b", "./a", "a/."].map(isValidScopePath)).toEqual([false, false, false, false, false, false]);
  });

  it("validates task length, budget range, ticket id and scope", () => {
    expect(validateSettings(DEFAULT_SETTINGS)).toEqual([]);
    expect(validateSettings({ ...DEFAULT_SETTINGS, task: "x".repeat(2001), budget: 100, group: "bad id!", scope: ["../x"] }).sort()).toEqual(["budgetRange", "scopeInvalid", "taskTooLong", "ticketInvalid"]);
    expect(clampBudget(5)).toBe(200);
    expect(clampBudget(99_999)).toBe(20_000);
  });
});
