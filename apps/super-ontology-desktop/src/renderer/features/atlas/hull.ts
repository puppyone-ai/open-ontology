/**
 * Hull geometry of a group (25 section 7.3): the convex hull of its members, grown by a padding into a rounded outline. Pure numbers, no
 * canvas, so the canvas drawing and the SVG snapshot share one shape.
 */
export type Pt = { x: number; y: number };
export type HullArc = { x: number; y: number; a0: number; a1: number };

const cross = (o: Pt, a: Pt, b: Pt): number => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);

/** Convex hull (Andrew's monotone chain), counter-clockwise in the numeric plane; one point for one point, two for two. */
export function convexHull(points: readonly Pt[]): Pt[] {
  const sorted = [...points].sort((p, q) => p.x - q.x || p.y - q.y);
  const unique = sorted.filter((p, i) => i === 0 || p.x !== sorted[i - 1]!.x || p.y !== sorted[i - 1]!.y);
  if (unique.length <= 2) return unique;
  const lower: Pt[] = [];
  for (const p of unique) { while (lower.length >= 2 && cross(lower[lower.length - 2]!, lower[lower.length - 1]!, p) <= 0) lower.pop(); lower.push(p); }
  const upper: Pt[] = [];
  for (let i = unique.length - 1; i >= 0; i -= 1) { const p = unique[i]!; while (upper.length >= 2 && cross(upper[upper.length - 2]!, upper[upper.length - 1]!, p) <= 0) upper.pop(); upper.push(p); }
  lower.pop(); upper.pop();
  return [...lower, ...upper];
}

/** The arcs that round the corners of a hull: at each vertex, from the outward normal of the edge before it to that of the edge after it. */
export function hullArcs(hull: readonly Pt[]): HullArc[] {
  const n = hull.length;
  if (n === 0) return [];
  if (n === 1) return [{ x: hull[0]!.x, y: hull[0]!.y, a0: 0, a1: Math.PI * 2 }];
  const normal = (a: Pt, b: Pt) => Math.atan2(-(b.x - a.x), b.y - a.y);
  return hull.map((p, i) => {
    const a0 = normal(hull[(i + n - 1) % n]!, p);
    let a1 = normal(p, hull[(i + 1) % n]!);
    // A vertex where the outline does not turn (collinear points) must not wrap to a whole circle through rounding noise.
    if (a1 < a0 - 1e-9) a1 += Math.PI * 2;
    else if (a1 < a0) a1 = a0;
    return { x: p.x, y: p.y, a0, a1 };
  });
}

export function hullBounds(hull: readonly Pt[], pad: number): { minX: number; minY: number; maxX: number; maxY: number } {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const p of hull) { minX = Math.min(minX, p.x); minY = Math.min(minY, p.y); maxX = Math.max(maxX, p.x); maxY = Math.max(maxY, p.y); }
  return { minX: minX - pad, minY: minY - pad, maxX: maxX + pad, maxY: maxY + pad };
}

/** The four corners of the outline of a folded group: a square, which the padding then rounds. */
export const foldedCorners = (r: number): Pt[] => { const h = r * 0.62; return [{ x: -h, y: -h }, { x: h, y: -h }, { x: h, y: h }, { x: -h, y: h }]; };
export const foldedPad = (r: number): number => r * 0.28;
