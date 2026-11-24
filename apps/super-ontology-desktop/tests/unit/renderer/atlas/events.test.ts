import { describe, expect, it } from "vitest";
import type { EngineEvent } from "../../../../src/shared/super-ontology/bridge";
import { FLOOD, opsForEvent, opsForEvents, type EventContext } from "../../../../src/renderer/features/atlas/events";

const at = "2026-11-19T10:00:00Z";
const ev = (type: string, data: Record<string, unknown>, extra: Partial<EngineEvent> = {}): EngineEvent => ({ type, at, data, seq: 1, ...extra });
const file = (id: string, state: string, path = "docs/a.md", drive = "d1") => ev("file.state_changed", { file: { id, drive_id: drive, path, state } });
const context = (members: string[] = [], groups: string[] = [], mode: EventContext["mode"] = "source"): EventContext => ({ mode, hasMember: (id) => members.includes(id), hasGroup: (id) => groups.includes(id) });

describe("atlas events", () => {
  it("a new file buds a cell in its Drive, a known one pulses, a deleted one retracts", () => {
    expect(opsForEvent(file("1", "queued"), context([], ["g:drive:d1"]))).toEqual([{ type: "bud", group: undefined, member: { id: "f:1", group: "g:drive:d1", label: "a.md", ref: { kind: "file", fileId: "1", path: "docs/a.md" }, weight: 0, born: null } }]);
    expect(opsForEvent(file("1", "parsing"), context(["f:1"], ["g:drive:d1"]))).toEqual([{ type: "pulse", id: "f:1" }]);
    expect(opsForEvent(file("1", "deleted"), context(["f:1"]))).toEqual([{ type: "retract", id: "f:1" }]);
    expect(opsForEvent(file("2", "deleted"), context())).toEqual([]);
  });

  it("a file in a Drive the picture does not know yet brings the group along", () => {
    const [op] = opsForEvent(file("9", "queued", "x.md", "abcdef12-0000"), context());
    expect(op).toMatchObject({ type: "bud", group: { id: "g:drive:abcdef12-0000", kind: "drive" } });
  });

  it("removal and rename events act on known files only", () => {
    expect(opsForEvent(ev("file.removed", { file_id: "1" }), context(["f:1"]))).toEqual([{ type: "retract", id: "f:1" }]);
    expect(opsForEvent(ev("file.removed", { file_id: "7" }), context(["f:1"]))).toEqual([]);
    expect(opsForEvent(ev("file.renamed", { file_id: "1", old_path: "a", new_path: "b" }), context(["f:1"]))).toEqual([{ type: "pulse", id: "f:1" }]);
  });

  it("file events do not draw files when the picture is grouped by type", () => {
    expect(opsForEvent(file("1", "queued"), context([], [], "type"))).toEqual([]);
  });

  it("a new ontology version breathes and asks for a reload", () => {
    expect(opsForEvent(ev("ontology.version_published", { n: 4 }), context())).toEqual([{ type: "breathe" }, { type: "refresh" }]);
  });

  it("events that move data without saying what ask for one reload, however many arrive", () => {
    const events = [ev("review.decided", {}), ev("drive.sync_completed", {}), ev("files.bulk_changed", { drive_id: "d1", count: 5 }), ev("ontology.version_published", { n: 5 })];
    const ops = opsForEvents(events, context());
    expect(ops.filter((op) => op.type === "refresh")).toHaveLength(1);
    expect(ops.filter((op) => op.type === "breathe")).toHaveLength(1);
  });

  it("a flood of changes becomes one breath and one reload, not a thousand buds", () => {
    const events = Array.from({ length: FLOOD + 20 }, (_, i) => file(String(i), "queued"));
    expect(opsForEvents(events, context())).toEqual([{ type: "breathe" }, { type: "refresh" }]);
    expect(opsForEvents(events.slice(0, 10), context())).toHaveLength(10);
  });

  it("ignores events it does not understand and malformed payloads", () => {
    expect(opsForEvent(ev("usage.changed", {}), context())).toEqual([]);
    expect(opsForEvent(ev("file.state_changed", { file: 5 }), context())).toEqual([]);
    expect(opsForEvent(ev("file.removed", {}), context())).toEqual([]);
  });
});
