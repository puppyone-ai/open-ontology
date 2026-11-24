// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { classOf, classOfProfile, FILE_CLASSES, FILE_PROFILES, profileOf, reasonOf, useClassText, useProfileText } from "../../../../src/renderer/model/profile";
import { setProfileOverride, listProfileOverrides } from "../../../../src/renderer/model/profileOverride";
import { installBridgeReader } from "../../../../src/renderer/model/bridgeInvoke";
import { analyzeUnlinked, fileReason, reasonForRow, useUnlinkedText, type UnlinkedSource } from "../../../../src/renderer/model/unlinked";
import { catalog } from "../../../support/locales";

const wrap = ({ children }: { children: React.ReactNode }) => <TestLocalizationProvider messages={mergeCatalogNamespaces({ app: catalog("en", "app") })}>{children}</TestLocalizationProvider>;
const zh = ({ children }: { children: React.ReactNode }) => <TestLocalizationProvider locale="zh-Hans" messages={mergeCatalogNamespaces({ app: catalog("zh-Hans", "app") })}>{children}</TestLocalizationProvider>;
const ready = (over: Record<string, unknown> = {}) => ({ id: "f", drive_id: "d", state: "published", path: "a/f.md", ...over });

describe("document profile and class", () => {
  it("maps known profiles and reads anything else as other; reference replaced paper", () => {
    for (const p of FILE_PROFILES) expect(profileOf({ profile: p })).toBe(p);
    expect(FILE_PROFILES).toContain("reference");
    expect(FILE_PROFILES).not.toContain("paper");
    expect(profileOf({})).toBe("other");
    expect(profileOf({ profile: "paper" })).toBe("other");
    expect(profileOf({ profile: 7 })).toBe("other");
  });
  it("takes the class from the file and derives it from the profile otherwise", () => {
    for (const c of FILE_CLASSES) expect(classOf({ class: c })).toBe(c);
    expect(classOf({ class: "code", profile: "business" })).toBe("code");
    expect(classOf({ profile: "business" })).toBe("record");
    expect(classOf({ profile: "meeting" })).toBe("record");
    expect(classOf({ profile: "table" })).toBe("record");
    for (const p of ["instruction", "code", "reference", "media", "other"] as const) expect(classOfProfile(p)).toBe(p);
  });
  it("reads unknown values as other or none, never as an error", () => {
    expect(classOf({ class: "hologram" })).toBe("other");
    expect(classOf({ class: 5 })).toBe("other");
    expect(classOf({})).toBe("other");
    expect(reasonOf({ profile_reason: "ext" })).toBe("ext");
    expect(reasonOf({ profile_reason: "telepathy" })).toBeNull();
    expect(reasonOf({})).toBeNull();
  });
  it("names the profile, the class and the reason in plain words", () => {
    const profile = renderHook(() => useProfileText(), { wrapper: wrap });
    expect(profile.result.current({ profile: "reference" })).toBe("Reference material");
    expect(profile.result.current({ profile: "hologram" })).toBe("Other");
    const en = renderHook(() => useClassText(), { wrapper: wrap });
    expect(en.result.current.name({ class: "code" })).toBe("Code file");
    expect(en.result.current.line({ class: "code", profile_reason: "ext" })).toBe("Class: Code file, not in the ontology · Based on: file extension");
    expect(en.result.current.line({ class: "weird", profile_reason: "weird" })).toBe("Class: Archive file, not in the ontology");
    const cn = renderHook(() => useClassText(), { wrapper: zh });
    expect(cn.result.current.name({ class: "code" })).toBe("代码文件");
    expect(cn.result.current.line({ class: "code", profile_reason: "ext" })).toBe("类别：代码文件，不进入本体 · 依据：扩展名");
    expect(cn.result.current.line({ class: "record", profile_reason: "default" })).toContain("未能判定，按业务文档处理");
  });
});

describe("files outside the ontology (D94)", () => {
  it("get not_admitted for instruction, code and other classes, with the reason", () => {
    expect(fileReason(ready({ class: "code", profile: "code", profile_reason: "ext" }), null)).toEqual({ kind: "not_admitted", class: "code", reason: "ext" });
    expect(fileReason(ready({ class: "instruction", profile: "instruction", profile_reason: "name" }), { entities: 0, connected: false })).toEqual({ kind: "not_admitted", class: "instruction", reason: "name" });
    expect(fileReason(ready({ profile: "other" }), null)).toEqual({ kind: "not_admitted", class: "other", reason: null });
  });
  it("leave records, references and media as they are; a record with no entities keeps no_entities", () => {
    expect(fileReason(ready({ class: "record", profile: "business" }), { entities: 0, connected: false })).toEqual({ kind: "no_entities" });
    expect(fileReason(ready({ class: "reference", profile: "reference" }), { entities: 1, connected: true })).toBeNull();
    expect(fileReason(ready({ class: "reference", profile: "reference" }), { entities: 0, connected: false })).toBeNull();
    expect(fileReason(ready({ class: "media", profile: "media" }), { entities: 0, connected: false })).toEqual({ kind: "no_entities" });
    // An engine that sends neither class nor profile leaves the file in.
    expect(fileReason(ready(), { entities: 0, connected: false })).toEqual({ kind: "no_entities" });
  });
  it("keep real problems: an unsupported or still processing code file is reported as such", () => {
    expect(fileReason(ready({ class: "code", state: "unsupported" }), null)).toMatchObject({ kind: "unsupported" });
    expect(fileReason(ready({ class: "code", state: "parsing" }), null)).toEqual({ kind: "processing" });
  });
  it("are not counted as not connected and are not read", async () => {
    const files = [ready({ id: "r1", class: "record", profile: "business" }), ready({ id: "c1", class: "code", profile: "code" }), ready({ id: "i1", class: "instruction", profile: "instruction" }), ready({ id: "o1", class: "other", profile: "other" }), ready({ id: "x1", class: "reference", profile: "reference" })];
    const read: string[] = [];
    const source: UnlinkedSource = {
      drives: async () => ({ ok: true, value: [{ id: "d", name: "D" }] }),
      files: async () => ({ ok: true, value: { items: files as never, next_cursor: null } }),
      detail: async (id) => { read.push(id); return { ok: true, value: { id, entities: [], facts: [] } as never }; },
      linkedFilePaths: async () => new Set(),
    };
    const report = await analyzeUnlinked(source);
    expect(report.files.map((f) => [f.fileId, f.reason.kind])).toEqual([["r1", "no_entities"]]);
    expect(read.sort()).toEqual(["r1", "x1"]);
    expect(reasonForRow(ready({ id: "c1", class: "code", profile_reason: "dir" }) as never, report)).toEqual({ kind: "not_admitted", class: "code", reason: "dir" });
    expect(reasonForRow(ready({ id: "x1", class: "reference" }) as never, report)).toBeNull();
  });
  it("read as a neutral sentence per class, and a record keeps the old wording", () => {
    const en = renderHook(() => useUnlinkedText(), { wrapper: wrap }).result.current;
    expect(en({ kind: "not_admitted", class: "code", reason: null })).toBe("Code file, not in the ontology");
    expect(en({ kind: "not_admitted", class: "instruction", reason: null })).toBe("Instruction file, attached as a working convention, not in the ontology");
    expect(en({ kind: "not_admitted", class: "other", reason: null })).toBe("Archive file, not in the ontology");
    expect(en({ kind: "no_entities" })).toBe("No entities found in this file");
    const cn = renderHook(() => useUnlinkedText(), { wrapper: zh }).result.current;
    expect(cn({ kind: "not_admitted", class: "code", reason: null })).toBe("代码文件，不进入本体");
    expect(cn({ kind: "not_admitted", class: "instruction", reason: null })).toBe("指令文件，作为工作约定挂在所属项目，不进入本体");
    expect(cn({ kind: "not_admitted", class: "other", reason: null })).toBe("归档文件，不进入本体");
    expect(cn({ kind: "no_entities" })).toBe("这个文件里没有找到实体");
    expect(cn({ kind: "group", of: "no_entities", count: 3 })).toBe("这些文件中没有找到实体");
    for (const c of ["code", "instruction", "other"] as const) expect(cn({ kind: "not_admitted", class: c, reason: null })).not.toMatch(/没有找到实体/);
  });
});

describe("class override through the bridge", () => {
  afterEach(() => installBridgeReader(() => null));
  it("sends the scope, path and profile, and null to hand the choice back", async () => {
    const calls: [string, unknown][] = [];
    installBridgeReader(() => ({ invoke: async (channel: string, request: unknown) => { calls.push([channel, request]); return { ok: true, value: channel === "profile-override-set" ? { affected: 3 } : { items: [{ scope: "dir", path: "src", profile: "code", class: "code" }] } }; } }) as never);
    expect(await setProfileOverride({ driveId: "d", scope: "dir", path: "src", profile: "code" })).toEqual({ ok: true, value: { affected: 3 } });
    await setProfileOverride({ driveId: "d", scope: "file", path: "src/a.ts", profile: null });
    expect(calls).toEqual([["profile-override-set", { driveId: "d", scope: "dir", path: "src", profile: "code" }], ["profile-override-set", { driveId: "d", scope: "file", path: "src/a.ts", profile: null }]]);
    expect(await listProfileOverrides("d")).toEqual({ ok: true, value: [{ scope: "dir", path: "src", profile: "code", class: "code" }] });
  });
  it("answers a failure when there is no bridge", async () => {
    expect(await setProfileOverride({ driveId: "d", scope: "file", path: "a", profile: null })).toMatchObject({ ok: false });
  });
});
