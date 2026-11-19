import { describe, expect, it } from "vitest";
import { SimCore, type SimNodeSpec } from "../../../../src/renderer/features/atlas/simulation";
import { spiralPoint } from "../../../../src/renderer/features/atlas/layout";

/** Two groups with members at spiral offsets. */
function specs(members = 30): SimNodeSpec[] {
  const out: SimNodeSpec[] = [{ idx: 0, kind: "group", r: 90, ax: -120, ay: 0 }, { idx: 1, kind: "group", r: 90, ax: 120, ay: 0 }];
  for (let i = 0; i < members; i += 1) {
    const p = spiralPoint(i);
    out.push({ idx: 2 + i, kind: "member", r: 5, group: i % 2, ox: p.x, oy: p.y });
  }
  return out;
}
const positions = (core: SimCore, count: number) => Array.from({ length: count }, (_, i) => core.position(i)!);

describe("layout simulation", () => {
  it("settles, and groups end up at their anchors", () => {
    const core = new SimCore();
    core.setNodes(specs(), "target");
    const steps = core.settle();
    expect(core.settled).toBe(true);
    expect(steps).toBeLessThan(400);
    expect(Math.abs(core.position(0)!.x - -120)).toBeLessThan(25);
    expect(Math.abs(core.position(1)!.x - 120)).toBeLessThan(25);
  });

  it("is deterministic: the same input gives the same picture", () => {
    const run = () => { const core = new SimCore(); core.setNodes(specs(), "target"); core.settle(); return positions(core, 32); };
    expect(run()).toEqual(run());
  });

  it("keeps members of one group from overlapping", () => {
    const core = new SimCore();
    core.setNodes(specs(40), "target");
    core.settle();
    const group0 = Array.from({ length: 40 }, (_, i) => i).filter((i) => i % 2 === 0).map((i) => core.position(2 + i)!);
    let closest = Infinity;
    for (let i = 0; i < group0.length; i += 1) for (let j = i + 1; j < group0.length; j += 1) closest = Math.min(closest, Math.hypot(group0[i]!.x - group0[j]!.x, group0[i]!.y - group0[j]!.y));
    expect(closest).toBeGreaterThan(9.5);
  });

  it("members follow their group", () => {
    const core = new SimCore();
    core.setNodes(specs(), "target");
    core.settle();
    const group = core.position(0)!;
    const member = core.position(2)!; // idx 2 is member 0 of group 0, at spiral point 0
    const p = spiralPoint(0);
    expect(Math.hypot(member.x - (group.x + p.x), member.y - (group.y + p.y))).toBeLessThan(12);
  });

  it("a new member starts at its group and flows to its place; the others keep their positions", () => {
    const core = new SimCore();
    core.setNodes(specs(10), "target");
    core.settle();
    const before = positions(core, 12);
    const p = spiralPoint(10);
    core.setNodes([...specs(10), { idx: 12, kind: "member", r: 5, group: 0, ox: p.x, oy: p.y }], "bud");
    expect(core.position(12)).toEqual(core.position(0));
    expect(positions(core, 12)).toEqual(before);
    core.settle();
    const group = core.position(0)!;
    expect(Math.hypot(core.position(12)!.x - (group.x + p.x), core.position(12)!.y - (group.y + p.y))).toBeLessThan(12);
  });

  it("fills a positions buffer by index and leaves unknown cells alone", () => {
    const core = new SimCore();
    core.setNodes(specs(4), "target");
    const out = new Float32Array(20).fill(Number.NaN);
    core.copyPositions(out);
    expect(out[0]).toBeCloseTo(-120, 0);
    expect(Number.isNaN(out[2 * 6])).toBe(true);
  });
});
