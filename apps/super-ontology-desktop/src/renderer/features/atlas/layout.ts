/**
 * Pure layout of the atlas (25 section 7.3): groups on a phyllotaxis layout, members on a golden-angle spiral around the group.
 * No clocks and no randomness, so the same data always gives the same picture (the static snapshot depends on it).
 */
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

/** World units. A member at weight 0 is the smallest; the spiral spacing leaves room for the largest. */
export const MEMBER_MIN_RADIUS = 3.2;
export const MEMBER_MAX_RADIUS = 6.4;
const SPIRAL_C = (2 * MEMBER_MAX_RADIUS + 3.2) / 1.9;
const GROUP_PAD = 10;

export const memberRadius = (weight: number): number => MEMBER_MIN_RADIUS + (MEMBER_MAX_RADIUS - MEMBER_MIN_RADIUS) * Math.min(1, Math.log1p(Math.max(0, weight)) / Math.log1p(50));

/** The point of a golden-angle (Vogel) spiral: spacing between neighbours is nearly constant. */
export function spiralPoint(index: number): { x: number; y: number } {
  const radius = SPIRAL_C * Math.sqrt(index + 0.5);
  const angle = index * GOLDEN_ANGLE;
  return { x: radius * Math.cos(angle), y: radius * Math.sin(angle) };
}

/** Radius of a group that shows `members` cells. */
export const expandedRadius = (members: number): number => (members <= 0 ? collapsedRadius(0) : SPIRAL_C * Math.sqrt(members) + MEMBER_MAX_RADIUS + GROUP_PAD);

/** Radius of a collapsed group: grows with the logarithm of its size so a thousand members do not dwarf the rest. */
export const collapsedRadius = (members: number): number => Math.min(64, 16 + 7 * Math.log2(1 + Math.max(0, members)));

/** Anchors of `radii.length` groups on a phyllotaxis layout; the biggest group sits in the middle. The simulation relaxes any overlap. */
export function phyllotaxisAnchors(radii: readonly number[]): { x: number; y: number }[] {
  const count = radii.length;
  if (count === 0) return [];
  const mean = radii.reduce((sum, value) => sum + value, 0) / count;
  const scale = mean * 2.3;
  const order = radii.map((radius, index) => ({ radius, index })).sort((a, b) => b.radius - a.radius || a.index - b.index);
  const anchors = new Array<{ x: number; y: number }>(count);
  order.forEach(({ index }, rank) => {
    const radius = scale * Math.sqrt(rank);
    const angle = rank * GOLDEN_ANGLE;
    anchors[index] = { x: radius * Math.cos(angle), y: radius * Math.sin(angle) };
  });
  return anchors;
}

/** Bounds of a set of discs, for "fit". */
export function discBounds(discs: Iterable<{ x: number; y: number; r: number }>): { minX: number; minY: number; maxX: number; maxY: number } | null {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const { x, y, r } of discs) {
    if (x - r < minX) minX = x - r;
    if (y - r < minY) minY = y - r;
    if (x + r > maxX) maxX = x + r;
    if (y + r > maxY) maxY = y + r;
  }
  return Number.isFinite(minX) ? { minX, minY, maxX, maxY } : null;
}
