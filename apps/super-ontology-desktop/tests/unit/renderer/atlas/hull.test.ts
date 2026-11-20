import { describe, expect, it } from "vitest";
import { convexHull, foldedCorners, foldedPad, hullArcs, hullBounds } from "../../../../src/renderer/features/atlas/hull";

describe("atlas hull geometry", () => {
  it("keeps only the outer points of a cloud, in a fixed order", () => {
    const cloud = [{ x: 0, y: 0 }, { x: 4, y: 0 }, { x: 4, y: 4 }, { x: 0, y: 4 }, { x: 2, y: 2 }, { x: 1, y: 3 }, { x: 4, y: 0 }];
    const hull = convexHull(cloud);
    expect(hull).toHaveLength(4);
    expect(convexHull([...cloud].reverse())).toEqual(hull);
    expect(convexHull([{ x: 1, y: 1 }])).toEqual([{ x: 1, y: 1 }]);
    expect(convexHull([{ x: 0, y: 0 }, { x: 3, y: 0 }, { x: 1, y: 0 }])).toHaveLength(2);
  });

  it("rounds each corner by the angle the outline turns there, so the turns add up to one full circle", () => {
    const arcs = hullArcs(convexHull([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }]));
    expect(arcs).toHaveLength(4);
    expect(arcs.reduce((sum, a) => sum + (a.a1 - a.a0), 0)).toBeCloseTo(Math.PI * 2, 6);
    for (const a of arcs) expect(a.a1 - a.a0).toBeCloseTo(Math.PI / 2, 6);
    expect(hullArcs([{ x: 3, y: 4 }])).toEqual([{ x: 3, y: 4, a0: 0, a1: Math.PI * 2 }]);
    const pair = hullArcs(convexHull([{ x: 0, y: 0 }, { x: 5, y: 0 }]));
    expect(pair.reduce((sum, a) => sum + (a.a1 - a.a0), 0)).toBeCloseTo(Math.PI * 2, 6);
  });

  it("never turns a straight vertex into a whole circle", () => {
    const lattice = Array.from({ length: 40 }, (_, i) => ({ x: (i % 8) * 18 + (Math.floor(i / 8) % 2) * 9, y: Math.floor(i / 8) * 15.588457268119896 }));
    for (const arc of hullArcs(convexHull(lattice))) expect(arc.a1 - arc.a0).toBeLessThan(Math.PI * 1.01);
    const total = hullArcs(convexHull(lattice)).reduce((sum, a) => sum + (a.a1 - a.a0), 0);
    expect(total).toBeCloseTo(Math.PI * 2, 5);
  });

  it("bounds the rounded outline by the padding and gives a folded group a square", () => {
    expect(hullBounds([{ x: 0, y: 0 }, { x: 10, y: 6 }], 2)).toEqual({ minX: -2, minY: -2, maxX: 12, maxY: 8 });
    const corners = foldedCorners(50);
    expect(corners).toHaveLength(4);
    expect(foldedPad(50)).toBeGreaterThan(0);
    const b = hullBounds(corners, foldedPad(50));
    expect(b.maxX - b.minX).toBeCloseTo(b.maxY - b.minY, 6);
  });
});
