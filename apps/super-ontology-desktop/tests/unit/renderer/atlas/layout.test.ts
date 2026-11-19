import { describe, expect, it } from "vitest";
import { collapsedRadius, expandedRadius, GOLDEN_ANGLE, MEMBER_MAX_RADIUS, memberRadius, phyllotaxisAnchors, spiralPoint } from "../../../../src/renderer/features/atlas/layout";

describe("atlas layout", () => {
  it("uses the golden angle", () => {
    expect(GOLDEN_ANGLE).toBeCloseTo(2.399963, 5);
  });

  it("places members on a spiral whose neighbours never overlap", () => {
    const points = Array.from({ length: 400 }, (_, i) => spiralPoint(i));
    let closest = Infinity;
    for (let i = 0; i < points.length; i += 1) for (let j = i + 1; j < points.length; j += 1) closest = Math.min(closest, Math.hypot(points[i]!.x - points[j]!.x, points[i]!.y - points[j]!.y));
    // Two of the largest cells fit side by side with a little air.
    expect(closest).toBeGreaterThan(MEMBER_MAX_RADIUS * 2);
  });

  it("grows the group radius with the square root of its members, so area follows count", () => {
    const area = (n: number) => Math.PI * expandedRadius(n) ** 2;
    expect(area(400) / area(100)).toBeGreaterThan(3);
    expect(area(400) / area(100)).toBeLessThan(4.6);
    expect(collapsedRadius(10_000)).toBeLessThanOrEqual(64);
    expect(collapsedRadius(1)).toBeGreaterThan(collapsedRadius(0));
  });

  it("sizes a member by its weight, between the minimum and the maximum", () => {
    expect(memberRadius(0)).toBeLessThan(memberRadius(5));
    expect(memberRadius(5)).toBeLessThan(memberRadius(500));
    expect(memberRadius(1e9)).toBeCloseTo(MEMBER_MAX_RADIUS, 5);
  });

  it("puts the biggest group in the middle of a phyllotaxis layout and is deterministic", () => {
    const radii = [30, 80, 20, 45, 45];
    const a = phyllotaxisAnchors(radii);
    expect(a[1]).toEqual({ x: 0, y: 0 });
    expect(phyllotaxisAnchors(radii)).toEqual(a);
    expect(phyllotaxisAnchors([])).toEqual([]);
    // Ties keep their input order, so the picture never depends on sort stability.
    expect(Math.hypot(a[3]!.x, a[3]!.y)).toBeLessThan(Math.hypot(a[4]!.x, a[4]!.y));
  });
});
