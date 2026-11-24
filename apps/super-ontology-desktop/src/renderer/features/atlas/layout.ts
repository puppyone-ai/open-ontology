/**
 * Pure layout of the atlas (25 section 7.3): groups on a phyllotaxis layout, members on a hexagonal lattice inside the group.
 * No clocks and no randomness, so the same data always gives the same picture (the static snapshot depends on it).
 */
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

/** World units. A member at weight 0 is the smallest; the lattice pitch leaves room for the largest. */
export const MEMBER_MIN_RADIUS = 3.2;
export const MEMBER_MAX_RADIUS = 6.4;
/** Distance between neighbouring members: a hexagonal lattice, so every member sits on a grid and no two are ever closer than this. */
export const LATTICE_PITCH = 18;
const GROUP_PAD = 10;

export const memberRadius = (weight: number): number => MEMBER_MIN_RADIUS + (MEMBER_MAX_RADIUS - MEMBER_MIN_RADIUS) * Math.min(1, Math.log1p(Math.max(0, weight)) / Math.log1p(50));

/** The points of a hexagonal lattice, nearest the centre first (ties by angle), grown on demand; the same index is always the same point. */
const lattice: { x: number; y: number }[] = [];
let latticeReach = 0;
function growLattice(count: number): void {
  let reach = Math.max(latticeReach * 2, LATTICE_PITCH * 3);
  while (true) {
    const rows = Math.ceil(reach / (LATTICE_PITCH * 0.866)) + 1;
    const found: { x: number; y: number; d: number; a: number }[] = [];
    for (let row = -rows; row <= rows; row += 1) {
      const y = row * LATTICE_PITCH * 0.8660254;
      const shift = (Math.abs(row) % 2) * 0.5;
      for (let col = -Math.ceil(reach / LATTICE_PITCH) - 1; col <= Math.ceil(reach / LATTICE_PITCH) + 1; col += 1) {
        const x = (col + shift) * LATTICE_PITCH;
        const d = Math.hypot(x, y);
        if (d <= reach) found.push({ x, y, d: Math.round(d * 1e6) / 1e6, a: Math.atan2(y, x) });
      }
    }
    // Ties keep opposite points together (right and left, then the next pair), so a group of three is a row and of four a tidy kite.
    const half = (angle: number) => Math.round((((angle % Math.PI) + Math.PI) % Math.PI) * 1e6) / 1e6;
    found.sort((p, q) => p.d - q.d || half(p.a) - half(q.a) || p.a - q.a);
    if (found.length >= count) { lattice.length = 0; for (const point of found) lattice.push({ x: point.x, y: point.y }); latticeReach = reach; return; }
    reach *= 1.6;
  }
}

/** The `index`-th resting place of a member inside its group (a name kept from the spiral it replaced). */
export function spiralPoint(index: number): { x: number; y: number } {
  if (index >= lattice.length) growLattice(index + 1);
  return lattice[index]!;
}

/**
 * A short group (two to eight members) joined by directed links is laid out in columns by link direction, so most arrows point the same way:
 * a member nothing points to sits in the first column, one pointed to by it in the next, and so on. Members of a column are stacked and centred.
 * Returns where each member rests relative to the group and the radius the group needs, or null when there is nothing directed to follow.
 */
export const COLUMN_PITCH = 66;
export const ROW_PITCH = 36;
export function planColumns(ids: readonly string[], edges: readonly (readonly [string, string])[]): { offsets: Map<string, { x: number; y: number }>; radius: number } | null {
  if (ids.length < 2 || ids.length > 8) return null;
  const inside = new Set(ids);
  const own = edges.filter(([from, to]) => inside.has(from) && inside.has(to) && from !== to);
  if (own.length === 0) return null;
  const rank = new Map(ids.map((id) => [id, 0]));
  for (let pass = 0; pass < ids.length; pass += 1) {
    let changed = false;
    for (const [from, to] of own) if (rank.get(to)! < rank.get(from)! + 1 && rank.get(from)! + 1 < ids.length) { rank.set(to, rank.get(from)! + 1); changed = true; }
    if (!changed) break;
  }
  const columns = new Map<number, string[]>();
  for (const id of [...ids].sort()) { const r = rank.get(id)!; const list = columns.get(r); if (list) list.push(id); else columns.set(r, [id]); }
  const last = Math.max(...columns.keys());
  const offsets = new Map<string, { x: number; y: number }>();
  for (const [r, list] of columns) list.forEach((id, i) => offsets.set(id, { x: (r - last / 2) * COLUMN_PITCH, y: (i - (list.length - 1) / 2) * ROW_PITCH }));
  const reach = Math.max(...[...offsets.values()].map((p) => Math.hypot(p.x, p.y)));
  return { offsets, radius: reach + MEMBER_MAX_RADIUS + GROUP_PAD };
}

/** Radius of a group that shows `members` cells. */
export const expandedRadius = (members: number): number => {
  if (members <= 0) return collapsedRadius(0);
  const last = spiralPoint(members - 1);
  return Math.hypot(last.x, last.y) + MEMBER_MAX_RADIUS + GROUP_PAD;
};

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
