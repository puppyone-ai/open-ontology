import { describe, expect, it } from "vitest";
import { AtlasScene } from "../../../../src/renderer/features/atlas/scene";
import { syntheticAtlas } from "../../../../src/renderer/features/atlas/lab/fixtures";
import type { AtlasData, AtlasMember } from "../../../../src/renderer/features/atlas/model";

const small = (): AtlasData => syntheticAtlas({ groups: 4, members: 60, linkRatio: 1, versions: 4, seed: 3 });
const run = (scene: AtlasScene, seconds: number, dt = 1 / 60) => { for (let t = 0; t < seconds; t += dt) scene.step(dt); };

describe("atlas scene", () => {
  it("opens every group when the picture is small and folds them when it is large", () => {
    expect(new AtlasScene(small()).groups.every((g) => g.expanded)).toBe(true);
    const big = new AtlasScene(small(), { expandAllUpTo: 10 });
    expect(big.groups.every((g) => !g.expanded)).toBe(true);
    expect(big.nodes.filter((n) => n.kind === "member").every((n) => !n.shown && n.appear === 0)).toBe(true);
  });

  it("counts members and aggregates links between folded groups", () => {
    const data = small();
    const scene = new AtlasScene(data, { expandAllUpTo: 0 });
    expect(scene.memberCount).toBe(data.members.length);
    const crossing = data.links.filter((l) => data.members.find((m) => m.id === l.a)!.group !== data.members.find((m) => m.id === l.b)!.group);
    expect(scene.groupLinks.reduce((sum, link) => sum + link.count, 0)).toBe(crossing.length);
  });

  it("drops a link to a cell it does not know", () => {
    const data = small();
    data.links.push({ a: "e:missing", b: data.members[0]!.id, kind: "fact", weight: 1 });
    expect(new AtlasScene(data).links.length).toBe(data.links.length - 1);
  });

  it("opening a group buds its members out one after another, closing shrinks them back", () => {
    const scene = new AtlasScene(small(), { expandAllUpTo: 0 });
    const group = scene.groups[0]!;
    scene.setExpanded(group.id, true);
    const members = group.members.map((i) => scene.nodes[i]!);
    expect(members.every((m) => m.shown && m.appear === 0)).toBe(true);
    scene.step(0.1);
    expect(members[0]!.appear).toBeGreaterThan(0);
    expect(members[members.length - 1]!.appear).toBeLessThanOrEqual(members[0]!.appear);
    run(scene, 2);
    expect(members.every((m) => m.appear === 1)).toBe(true);
    scene.setExpanded(group.id, false);
    run(scene, 2);
    expect(members.every((m) => m.appear === 0 && !m.shown)).toBe(true);
    expect(group.rBase).toBeLessThan(80);
  });

  it("a new file buds a cell, pulses its group, and a second bud of the same id is ignored", () => {
    const scene = new AtlasScene(small());
    const group = scene.groups[0]!;
    const before = group.count;
    const member: AtlasMember = { id: "f:new", group: group.id, label: "new.md", ref: { kind: "file", fileId: "new", path: "new.md" }, weight: 0, born: null };
    scene.apply([{ type: "bud", member }]);
    expect(group.count).toBe(before + 1);
    const node = scene.node("f:new")!;
    expect(node.appear).toBe(0);
    expect(group.pulse).toBeGreaterThan(0);
    expect(node.x).toBe(group.x);
    run(scene, 1);
    expect(node.appear).toBe(1);
    scene.apply([{ type: "bud", member }]);
    expect(group.count).toBe(before + 1);
  });

  it("a bud for an unknown group creates the group", () => {
    const scene = new AtlasScene(small());
    const groups = scene.groups.length;
    scene.apply([{ type: "bud", group: { id: "g:new", kind: "drive", label: "New", tone: "ink" }, member: { id: "f:x", group: "g:new", label: "x", ref: { kind: "file", fileId: "x", path: "x" }, weight: 0, born: null } }]);
    expect(scene.groups.length).toBe(groups + 1);
    expect(scene.node("g:new")!.count).toBe(1);
  });

  it("a change pulses once and decays", () => {
    const scene = new AtlasScene(small());
    const id = scene.nodes.find((n) => n.kind === "member")!.id;
    scene.apply([{ type: "pulse", id }]);
    expect(scene.node(id)!.pulse).toBe(1);
    scene.step(0.3);
    expect(scene.node(id)!.pulse).toBeLessThan(1);
    expect(scene.node(id)!.pulse).toBeGreaterThan(0);
    run(scene, 1);
    expect(scene.node(id)!.pulse).toBe(0);
  });

  it("a retraction shrinks and fades the cell, then removes it; the group count drops at once", () => {
    const scene = new AtlasScene(small());
    const node = scene.nodes.find((n) => n.kind === "member")!;
    const owner = scene.owner(node);
    const count = owner.count;
    scene.apply([{ type: "retract", id: node.id }]);
    expect(owner.count).toBe(count - 1);
    scene.step(0.4);
    expect(node.retract).toBeGreaterThan(0.3);
    expect(scene.grown(node)).toBeLessThan(1);
    expect(node.alive).toBe(true);
    run(scene, 1);
    expect(node.alive).toBe(false);
    expect(scene.isVisible(node)).toBe(false);
    expect(scene.memberCount).toBe(small().members.length - 1);
  });

  it("a new ontology version makes the whole picture breathe once", () => {
    const scene = new AtlasScene(small());
    scene.apply([{ type: "breathe" }]);
    expect(scene.breathe).toBe(1);
    run(scene, 3);
    expect(scene.breathe).toBe(0);
  });

  it("play growth replays the versions in order, one group opening after another otherwise", () => {
    const scene = new AtlasScene(small());
    const steps = scene.playGrowth(0.5);
    expect(steps).toBe(4);
    expect(scene.nodes.every((n) => !n.shown)).toBe(true);
    const shownAt: number[] = [];
    for (let v = 0; v < 5; v += 1) { run(scene, 0.5); shownAt.push(scene.nodes.filter((n) => n.kind === "member" && n.shown).length); }
    for (let i = 1; i < shownAt.length; i += 1) expect(shownAt[i]!).toBeGreaterThanOrEqual(shownAt[i - 1]!);
    run(scene, 3);
    expect(scene.growing).toBe(false);
    expect(scene.nodes.filter((n) => n.kind === "member").every((n) => n.shown)).toBe(true);
    expect(scene.groups.every((g) => g.shown && g.expanded)).toBe(true);

    const plain = syntheticAtlas({ groups: 3, members: 30, seed: 5 });
    for (const m of plain.members) m.born = null;
    const flat = new AtlasScene(plain);
    expect(flat.playGrowth(0.5)).toBe(3);
  });

  it("finishing growth jumps to the whole picture", () => {
    const scene = new AtlasScene(small());
    scene.playGrowth();
    scene.finishGrowth();
    expect(scene.growing).toBe(false);
    expect(scene.nodes.every((n) => n.appear === 1 && n.shown)).toBe(true);
  });

  it("finds cells by name, case-insensitively, and the hit test finds the cell under a point", () => {
    const scene = new AtlasScene(small());
    const target = scene.nodes.find((n) => n.kind === "member")!;
    expect(scene.search(target.label.toUpperCase()).map((n) => n.id)).toContain(target.id);
    expect(scene.search("")).toEqual([]);
    expect(scene.hit(target.x, target.y)?.id).toBe(target.id);
    expect(scene.hit(1e6, 1e6)).toBeNull();
    const group = scene.groups[0]!;
    expect(scene.hit(group.x + group.r * 0.99, group.y)?.kind === "member" || scene.hit(group.x + group.r * 0.99, group.y)?.id === group.id).toBe(true);
  });

  it("reports bounds that hold every shown cell", () => {
    const scene = new AtlasScene(small());
    const b = scene.bounds()!;
    for (const g of scene.groups) { expect(g.x - g.rBase).toBeGreaterThanOrEqual(b.minX - 1e-6); expect(g.x + g.rBase).toBeLessThanOrEqual(b.maxX + 1e-6); }
  });

  it("applies positions by index and ignores NaN", () => {
    const scene = new AtlasScene(small());
    const positions = new Float32Array(scene.size * 2).fill(Number.NaN);
    positions[0] = 5; positions[1] = 6;
    const other = scene.nodes[1]!;
    const x = other.x;
    scene.setPositions(positions);
    expect(scene.nodes[0]!.x).toBe(5);
    expect(other.x).toBe(x);
  });
});
