import type { Camera } from "./camera";
import { convexHull, hullArcs, hullBounds, hullPadPx, type Pt } from "./hull";
import { ACCENT, CANVAS, TEXT, TEXT_SECOND, TEXT_WEAK, TONE_FILL, TONE_INK, rgba } from "./palette";
import type { AtlasOrigin, AtlasTone } from "./model";
import { ease, type AtlasScene, type SceneNode } from "./scene";

/**
 * Canvas 2D drawing of the atlas (25 section 4 and 7.3, "precise growth"): a warm white canvas with a faint dot grid; members as solid
 * dots (documents), rounded squares (databases) or a dot in a square ring (both); groups as rounded hull outlines with a label above
 * their top left corner; straight one pixel links. No gradients, blur or glow, and nothing moves at rest. Everything is drawn in device
 * pixels with positions and sizes rounded to whole pixels, so lines are one crisp pixel at every zoom and on every screen.
 */
export type DrawState = {
  camera: Camera;
  width: number;
  height: number;
  dpr: number;
  /** The cell under the pointer or the keyboard, and the one that was opened. -1 for none. */
  focus: number;
  selected: number;
  /** When set, only these cells (the focused cell and what touches it) are bright; the rest recede. */
  lit: ReadonlySet<number> | null;
  hits: ReadonlySet<number>;
  /** Kept for callers that still pass them; the picture no longer depends on the clock. */
  time?: number;
  animate?: boolean;
};
export type DrawStats = { members: number; labels: number; links: number };

const TAU = Math.PI * 2;
const MAX_LABELS = 260;
const FONT = '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif';
/** Half size of a member's mark, in world units, as a share of its radius; and the padding the hull keeps around its members. */
const MARK = 0.8;
/** World zoom above which labels of members may appear (the lattice pitch on screen is then wide enough for a name). */
const LABEL_ZOOM = 2.1;
/** Below this zoom only the aggregated links between groups are drawn. */
const DETAIL_ZOOM = 0.35;
/** From this zoom on the links between single cells of different groups are drawn; before it, one summary line per pair of groups. */
const CROSS_ZOOM = 1.8;
const DIM_NODE = 0.3;
const DIM_LINK = 0.04;

export const clearSpriteCache = (): void => undefined;

// ---- the grid -----------------------------------------------------------------------------------------------------

const gridPatterns = new WeakMap<object, { dpr: number; pattern: CanvasPattern | null }>();
function gridPattern(ctx: CanvasRenderingContext2D, dpr: number): CanvasPattern | null {
  const known = gridPatterns.get(ctx);
  if (known && known.dpr === dpr) return known.pattern;
  const size = Math.round(8 * dpr);
  let pattern: CanvasPattern | null = null;
  if (typeof document !== "undefined" || typeof OffscreenCanvas !== "undefined") {
    const surface = typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(size, size) : Object.assign(document.createElement("canvas"), { width: size, height: size });
    const tile = surface.getContext("2d") as CanvasRenderingContext2D | null;
    if (tile) {
      tile.fillStyle = "rgba(28, 25, 23, 0.07)";
      tile.fillRect(0, 0, Math.max(1, Math.round(dpr)), Math.max(1, Math.round(dpr)));
      pattern = ctx.createPattern?.(surface as CanvasImageSource, "repeat") ?? null;
    }
  }
  gridPatterns.set(ctx, { dpr, pattern });
  return pattern;
}

// ---- marks --------------------------------------------------------------------------------------------------------

/** 12px kind glyphs of a source (My files, folder, bucket, database): simple outlines on a 12 unit grid, drawn with the label. */
const GLYPHS: Record<string, string> = {
  files: "M3 1.5h4.2L9.5 4v6.5H3zM7.2 1.5V4h2.3",
  folder: "M1.5 3h3l1.2 1.3h4.8v5.7h-9z",
  bucket: "M2 3.6c0 .9 1.8 1.4 4 1.4s4-.5 4-1.4-1.8-1.4-4-1.4-4 .5-4 1.4zM2 3.6l1 6.6h6l1-6.6",
  database: "M2 3c0 .9 1.8 1.6 4 1.6S10 3.9 10 3 8.2 1.4 6 1.4 2 2.1 2 3zM2 3v6c0 .9 1.8 1.6 4 1.6S10 9.9 10 9V3M2 6c0 .9 1.8 1.6 4 1.6S10 6.9 10 6",
};
const newPath = (): Path2D | null => (typeof Path2D === "undefined" ? null : new Path2D());
const glyphPaths = new Map<string, Path2D>();
const glyphPath = (kind: string): Path2D | null => {
  if (typeof Path2D === "undefined" || !GLYPHS[kind]) return null;
  let path = glyphPaths.get(kind);
  if (!path) { path = new Path2D(GLYPHS[kind]); glyphPaths.set(kind, path); }
  return path;
};
export const GLYPH_PX = 12;

/** Half size, in CSS pixels, of what is drawn for a member at zoom `k` and growth `scale` (the ring of a "both" mark included). */
/** Half size of a mark in screen pixels: it follows the zoom when far out, and stops at 3 to 5 pixels (6 to 10 across) by weight, so zooming spreads the layout and never inflates the marks. */
export const markPx = (node: SceneNode, k: number): number => Math.max(1, Math.min(node.r * MARK * k, 3 + (node.r - 3.2) * 0.625));
export const markHalf = (node: SceneNode, k: number, scale = 1): number => Math.max(1, markPx(node, k) * scale) + (node.origin === "both" ? 3 : 0);

type Batch = { tone: AtlasTone; origin: AtlasOrigin; dim: boolean; values: number[]; alpha?: number };

/** Draws one kind of mark for many members at once. Positions and sizes are in device pixels, already rounded. */
function paintBatch(ctx: CanvasRenderingContext2D, batch: Batch, dpr: number): void {
  const { values, tone, origin } = batch;
  ctx.globalAlpha = batch.alpha ?? (batch.dim ? DIM_NODE : 1);
  const fill = TONE_FILL[tone];
  const hair = Math.max(1, Math.round(dpr));
  if (origin === "doc") {
    ctx.fillStyle = fill;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const r = values[i + 2]!; ctx.moveTo(values[i]! + r, values[i + 1]!); ctx.arc(values[i]!, values[i + 1]!, r, 0, TAU); }
    ctx.fill();
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = hair;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const r = values[i + 2]! + hair / 2; ctx.moveTo(values[i]! + r, values[i + 1]!); ctx.arc(values[i]!, values[i + 1]!, r, 0, TAU); }
    ctx.stroke();
  } else if (origin === "db") {
    ctx.fillStyle = fill;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const h = values[i + 2]!; box(ctx, values[i]! - h, values[i + 1]! - h, h * 2, 2 * dpr); }
    ctx.fill();
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = hair;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const h = values[i + 2]! + hair / 2; box(ctx, values[i]! - h, values[i + 1]! - h, h * 2, 2 * dpr + hair / 2); }
    ctx.stroke();
  } else {
    // Both: the dot sits inside a thin square ring, so one glance tells it is seen from the documents and the database at once.
    const ink = rgba(TONE_INK[tone], 0.9);
    ctx.fillStyle = fill;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const r = Math.max(1, Math.round(values[i + 2]! * 0.62)); ctx.moveTo(values[i]! + r, values[i + 1]!); ctx.arc(values[i]!, values[i + 1]!, r, 0, TAU); }
    ctx.fill();
    ctx.strokeStyle = ink;
    ctx.lineWidth = hair;
    ctx.beginPath();
    for (let i = 0; i < values.length; i += 3) { const h = values[i + 2]! + 3 * dpr - hair / 2; box(ctx, values[i]! - h, values[i + 1]! - h, h * 2, 2 * dpr); }
    ctx.stroke();
  }
}

function box(ctx: CanvasRenderingContext2D, x: number, y: number, size: number, radius: number): void {
  if (typeof ctx.roundRect === "function") ctx.roundRect(x, y, size, size, radius);
  else ctx.rect(x, y, size, size);
}

// ---- the picture --------------------------------------------------------------------------------------------------

export function drawAtlas(ctx: CanvasRenderingContext2D, scene: AtlasScene, state: DrawState): DrawStats {
  const { camera, width, height, dpr, lit, hits } = state;
  const stats: DrawStats = { members: 0, labels: 0, links: 0 };
  const k = camera.k;
  const hair = Math.max(1, Math.round(dpr));
  /** A line of one device pixel width lies on a pixel centre when the width is odd, on a pixel edge when it is even. */
  const snap = (v: number): number => Math.round(v - hair / 2) + hair / 2;
  const toX = (x: number): number => (x * k + camera.x) * dpr;
  const toY = (y: number): number => (y * k + camera.y) * dpr;

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  ctx.fillStyle = CANVAS;
  ctx.fillRect(0, 0, width * dpr, height * dpr);
  const pattern = gridPattern(ctx, dpr);
  if (pattern) {
    const step = Math.round(8 * dpr);
    const ox = ((Math.round(camera.x * dpr) % step) + step) % step;
    const oy = ((Math.round(camera.y * dpr) % step) + step) % step;
    ctx.setTransform(1, 0, 0, 1, ox, oy);
    ctx.fillStyle = pattern;
    ctx.fillRect(-ox, -oy, width * dpr, height * dpr);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  const left = -camera.x / k, top = -camera.y / k, right = (width - camera.x) / k, bottom = (height - camera.y) / k;
  const inView = (x: number, y: number, r: number) => x + r >= left && x - r <= right && y + r >= top && y - r <= bottom;
  const dimmed = (node: SceneNode) => lit !== null && !lit.has(node.idx);
  const nodes = scene.nodes;
  const shown = (node: SceneNode) => scene.isVisible(node);

  // 0. A small picture (a database of a few tables, say) is laid out on the screen first: where each name goes, clear of the marks, of the links and of
  //    the other names, and the outline of each group around its marks and its names with 16 pixels to spare.
  const small = k >= 0.8 && scene.memberCount <= 24;
  type Spot = { x: number; y: number; align: CanvasTextAlign; box: [number, number, number, number] };
  const spots = new Map<number, Spot>();
  const smallHulls = new Map<number, Pt[]>();
  if (small) {
    ctx.font = `500 12px ${FONT}`;
    const shownMembers = nodes.filter((n) => n.kind === "member" && shown(n) && n.appear >= 0.6);
    const at = (n: SceneNode) => ({ x: n.x * k + camera.x, y: n.y * k + camera.y, h: markHalf(n, k, 1) });
    const markBoxes = shownMembers.map((n) => { const c = at(n); return [c.x - c.h - 1, c.y - c.h - 1, c.x + c.h + 1, c.y + c.h + 1] as [number, number, number, number]; });
    const segments: [number, number, number, number][] = [];
    const live2 = new Set(shownMembers.map((n) => n.idx));
    for (const link of scene.links) {
      if (!live2.has(link.a) || !live2.has(link.b)) continue;
      const a = at(nodes[link.a]!), b = at(nodes[link.b]!);
      const len = Math.hypot(b.x - a.x, b.y - a.y);
      if (len <= a.h + b.h + 5) continue;
      const ux = (b.x - a.x) / len, uy = (b.y - a.y) / len;
      segments.push([a.x + ux * (a.h + 2), a.y + uy * (a.h + 2), b.x - ux * (b.h + 2), b.y - uy * (b.h + 2)]);
    }
    const crosses = (box: [number, number, number, number]) => segments.some(([x0, y0, x1, y1]) => {
      const steps = Math.max(2, Math.ceil(Math.hypot(x1 - x0, y1 - y0) / 3));
      for (let i = 0; i <= steps; i += 1) { const x = x0 + ((x1 - x0) * i) / steps, y = y0 + ((y1 - y0) * i) / steps; if (x >= box[0] && x <= box[2] && y >= box[1] && y <= box[3]) return true; }
      return false;
    });
    const placed: [number, number, number, number][] = [];
    const clash = (box: [number, number, number, number], list: [number, number, number, number][]) => list.some((m) => box[0] < m[2] && box[2] > m[0] && box[1] < m[3] && box[3] > m[1]);
    shownMembers.forEach((node, index) => {
      const c = at(node);
      const w = ctx.measureText(node.label).width;
      const options: [number, number, CanvasTextAlign][] = [[c.x + c.h + 7, c.y, "left"], [c.x, c.y + c.h + 13, "center"], [c.x, c.y - c.h - 13, "center"], [c.x - c.h - 7, c.y, "right"]];
      let chosen: Spot | null = null;
      for (const [x, y, align] of options) {
        const x0 = align === "left" ? x : align === "right" ? x - w : x - w / 2;
        const box: [number, number, number, number] = [x0 - 3, y - 9, x0 + w + 3, y + 9];
        if (clash(box, placed) || clash(box, markBoxes.filter((_, i) => i !== index)) || crosses(box)) continue;
        chosen = { x, y, align, box };
        break;
      }
      // Nothing is free: beside the mark, where the name is easiest to attach to its mark.
      if (!chosen) { const [x, y, align] = options[0]!; chosen = { x, y, align, box: [x - 3, y - 9, x + w + 3, y + 9] }; }
      placed.push(chosen.box);
      spots.set(node.idx, chosen);
    });
    for (const group of scene.groups) {
      const own = shownMembers.filter((n) => n.group === group.idx);
      if (own.length === 0) continue;
      const points: Pt[] = [];
      for (const node of own) {
        const m = markBoxes[shownMembers.indexOf(node)]!;
        const b = spots.get(node.idx)!.box;
        for (const [x, y] of [[m[0], m[1]], [m[2], m[1]], [m[0], m[3]], [m[2], m[3]], [b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]] as const) points.push({ x, y });
      }
      smallHulls.set(group.idx, convexHull(points));
    }
  }

  // 1. Group outlines: the hull of the members grown by a padding, one pixel line, a two to four percent tint inside.
  const labelsToDraw: { node: SceneNode; x: number; y: number; w: number; strong: boolean }[] = [];
  for (const group of scene.groups) {
    if (!shown(group) || (group.count === 0 && group.expanded)) continue;
    const own = smallHulls.get(group.idx);
    const local = own ? [] : scene.hullOf(group);
    if (local.length === 0 && !own) continue;
    const pad = group.expanded ? hullPadPx(k) / k : 0;
    const folded = !group.expanded;
    if (own) {
      const edge = 16;
      const arcsOwn = hullArcs(own);
      ctx.beginPath();
      arcsOwn.forEach((arc, i) => {
        const cx = snap(arc.x * dpr), cy = snap(arc.y * dpr), r = Math.round(edge * dpr);
        if (arcsOwn.length === 1) { ctx.moveTo(cx + r, cy); ctx.arc(cx, cy, r, 0, TAU); return; }
        ctx.arc(cx, cy, r, arc.a0, arc.a1);
        if (i === arcsOwn.length - 1) ctx.closePath();
      });
      const activeOwn = state.selected === group.idx || state.focus === group.idx;
      ctx.globalAlpha = (1 - group.retract) * (dimmed(group) ? 0.35 : 1);
      ctx.fillStyle = rgba(TONE_INK[group.tone], 0.02);
      ctx.fill();
      ctx.lineWidth = hair;
      ctx.strokeStyle = activeOwn ? rgba(ACCENT, 0.9) : rgba(TONE_INK[group.tone], 0.4);
      if (group.groupKind === "unlinked") ctx.setLineDash([Math.round(4 * dpr), Math.round(4 * dpr)]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
      const bounds = hullBounds(own, edge);
      labelsToDraw.push({ node: group, x: Math.round(bounds.minX), y: Math.round(bounds.minY), w: bounds.maxX - bounds.minX, strong: activeOwn || hits.has(group.idx) });
      continue;
    }
    const radius = folded ? Math.max(4, group.r * 0.28) : pad;
    const world = local.map((p) => ({ x: group.x + p.x, y: group.y + p.y }));
    const box = hullBounds(world, radius);
    if (!inView((box.minX + box.maxX) / 2, (box.minY + box.maxY) / 2, Math.max(box.maxX - box.minX, box.maxY - box.minY) / 2)) continue;
    const dim = dimmed(group);
    const active = state.selected === group.idx || state.focus === group.idx;
    const fade = (1 - group.retract) * (dim ? 0.35 : 1);
    const padPx = Math.round(radius * k * dpr);
    ctx.beginPath();
    // The turns are worked out in world space, where they are exact; only the centres are then put on the pixel grid.
    const arcs = hullArcs(world);
    arcs.forEach((arc, i) => {
      const cx = snap(toX(arc.x)), cy = snap(toY(arc.y));
      if (arcs.length === 1) { ctx.moveTo(cx + padPx, cy); ctx.arc(cx, cy, padPx, 0, TAU); return; }
      ctx.arc(cx, cy, padPx, arc.a0, arc.a1);
      if (i === arcs.length - 1) ctx.closePath();
    });
    ctx.globalAlpha = fade;
    ctx.fillStyle = rgba(TONE_INK[group.tone], folded ? 0.05 : 0.02);
    ctx.fill();
    ctx.lineWidth = hair;
    ctx.strokeStyle = active ? rgba(ACCENT, 0.9) : rgba(TONE_INK[group.tone], 0.4);
    if (group.groupKind === "unlinked") ctx.setLineDash([Math.round(4 * dpr), Math.round(4 * dpr)]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    const sx = Math.round(toX(box.minX) / dpr), sy = Math.round(toY(box.minY) / dpr);
    const wpx = (box.maxX - box.minX) * k;
    labelsToDraw.push({ node: group, x: sx, y: sy, w: wpx, strong: active || hits.has(group.idx) });
  }

  // 2. Links, straight one pixel lines under the marks. They stop at the edge of the mark; a directed one ends in a small arrowhead once
  //    the picture is large enough to read it. Far out only the aggregated links between groups are drawn.
  const arrowsOn = k >= 0.8;
  const density = scene.links.length <= 40 ? 0.42 : scene.links.length <= 120 ? 0.2 : 0.12;
  const tip = (node: SceneNode) => markHalf(node, k, scene.grown(node)) + 2;
  /** Appends the segment between two cells, trimmed at both marks, to the current path; true when it was long enough to draw. */
  const segment = (a: SceneNode, b: SceneNode, from: number, t: number, arrows: Path2D | null): boolean => {
    const ax = toX(a.x) / dpr, ay = toY(a.y) / dpr, bx = toX(b.x) / dpr, by = toY(b.y) / dpr;
    const len = Math.hypot(bx - ax, by - ay);
    const ta = tip(a), tb = tip(b);
    if (len <= ta + tb + 1) return false;
    // Drawn from the source towards the target, trimmed at the edge of each mark.
    const source = from === b.idx ? b : a;
    const [sx, sy, tx, ty, ts, tt] = source === a ? [ax, ay, bx, by, ta, tb] : [bx, by, ax, ay, tb, ta];
    const ux = (tx - sx) / len, uy = (ty - sy) / len;
    const x0 = sx + ux * ts, y0 = sy + uy * ts, x1 = tx - ux * tt, y1 = ty - uy * tt;
    const reach = t < 1 ? t : 1;
    ctx.moveTo(snap(x0 * dpr), snap(y0 * dpr));
    ctx.lineTo(snap((x0 + (x1 - x0) * reach) * dpr), snap((y0 + (y1 - y0) * reach) * dpr));
    if (arrows && from >= 0 && t >= 1) {
      const s = 8 * dpr, w = 3.5 * dpr;
      const px = x1 * dpr, py = y1 * dpr;
      arrows.moveTo(px, py);
      arrows.lineTo(px - ux * s - uy * w, py - uy * s + ux * w);
      arrows.lineTo(px - ux * s + uy * w, py - uy * s - ux * w);
      arrows.closePath();
    }
    return true;
  };
  ctx.lineCap = "butt";
  ctx.lineWidth = hair;
  const live = (node: SceneNode) => node.alive && node.appear > 0.5 && node.retract === 0;
  const linkVisible = (a: SceneNode, b: SceneNode) => live(a) && live(b);
  if (k >= DETAIL_ZOOM) {
    const arrows = arrowsOn ? newPath() : null;
    const drawing: [SceneNode, SceneNode, number, number][] = [];
    ctx.beginPath();
    for (const link of scene.links) {
      const a = nodes[link.a]!, b = nodes[link.b]!;
      if (!linkVisible(a, b) || !(a.shown && b.shown)) continue;
      // Between single cells only inside an open group, or once zoomed in far enough; otherwise the summary lines stand for them.
      if (!(a.group === b.group && nodes[a.group]!.expanded) && k < CROSS_ZOOM) continue;
      if (!inView(a.x, a.y, 40) && !inView(b.x, b.y, 40)) continue;
      if (link.t < 1) { drawing.push([a, b, link.from, link.t]); continue; }
      if (segment(a, b, link.from, 1, arrows)) stats.links += 1;
    }
    ctx.strokeStyle = rgba(TONE_INK.stone, lit !== null ? DIM_LINK : density);
    ctx.stroke();
    if (arrows) { ctx.fillStyle = rgba(TONE_INK.stone, lit !== null ? DIM_LINK * 2 : Math.min(0.75, density + 0.3)); ctx.fill(arrows); }
    if (drawing.length) {
      ctx.beginPath();
      for (const [a, b, from, t] of drawing) if (segment(a, b, from, ease(t), null)) stats.links += 1;
      ctx.strokeStyle = rgba(ACCENT, 0.5);
      ctx.stroke();
    }
  }
  // Between groups: one line per pair, one to three pixels by how many links it stands for. Between two open groups they are the member links.
  for (const link of scene.groupLinks) {
    const a = nodes[link.a]!, b = nodes[link.b]!;
    if (!shown(a) || !shown(b)) continue;
    if (k >= CROSS_ZOOM) continue;
    if (!inView(a.x, a.y, a.r) && !inView(b.x, b.y, b.r)) continue;
    const near = lit === null || (lit.has(a.idx) && lit.has(b.idx));
    const px = Math.min(3, Math.max(1, Math.round(1 + Math.log2(link.count) / 2)));
    ctx.lineWidth = px * hair;
    ctx.strokeStyle = rgba(TONE_INK.stone, near ? (lit !== null ? 0.5 : 0.2) : DIM_LINK);
    const dx = b.x - a.x, dy = b.y - a.y, len = Math.hypot(dx, dy) || 1;
    const ra = (a.expanded ? a.r : a.r * 0.9) * k, rb = (b.expanded ? b.r : b.r * 0.9) * k;
    if (len * k <= ra + rb) continue;
    ctx.beginPath();
    ctx.moveTo(snap(toX(a.x + (dx / len) * (ra / k))), snap(toY(a.y + (dy / len) * (ra / k))));
    ctx.lineTo(snap(toX(b.x - (dx / len) * (rb / k))), snap(toY(b.y - (dy / len) * (rb / k))));
    ctx.stroke();
    stats.links += 1;
  }
  ctx.lineWidth = hair;
  // Focus: the links of the focused cell in the accent, over the quiet ones.
  if (lit !== null && k >= DETAIL_ZOOM) {
    const arrows = arrowsOn ? newPath() : null;
    ctx.beginPath();
    for (const link of scene.links) {
      const a = nodes[link.a]!, b = nodes[link.b]!;
      if (!(lit.has(a.idx) && lit.has(b.idx) && linkVisible(a, b) && link.t >= 1)) continue;
      if (segment(a, b, link.from, 1, arrows)) stats.links += 1;
    }
    ctx.strokeStyle = rgba(ACCENT, 0.6);
    ctx.stroke();
    if (arrows) { ctx.fillStyle = rgba(ACCENT, 0.75); ctx.fill(arrows); }
  }
  ctx.globalAlpha = 1;

  // 3. Members. Settled ones are drawn in batches (one path per tone and kind of mark); the few that are arriving or leaving one by one.
  const batches = new Map<string, Batch>();
  const tiny = new Map<string, { fill: string; dim: boolean; values: number[] }>();
  const single: { node: SceneNode; scale: number }[] = [];
  const minMark = 1.1;
  for (const node of nodes) {
    if (node.kind !== "member" || !shown(node)) continue;
    const scale = scene.grown(node);
    const r = markPx(node, k) * scale;
    if (scale <= 0 || !inView(node.x, node.y, node.r)) continue;
    stats.members += 1;
    const dim = dimmed(node);
    const x = toX(node.x), y = toY(node.y);
    if (node.appear === 1 && !node.retracting && node.pulse === 0) {
      if (r < minMark) {
        const key = `${node.tone}|${dim ? 1 : 0}`;
        let bucket = tiny.get(key);
        if (!bucket) { bucket = { fill: TONE_FILL[node.tone], dim, values: [] }; tiny.set(key, bucket); }
        bucket.values.push(Math.round(x), Math.round(y));
        continue;
      }
      const key = `${node.tone}|${node.origin}|${dim ? 1 : 0}`;
      let batch = batches.get(key);
      if (!batch) { batch = { tone: node.tone, origin: node.origin, dim, values: [] }; batches.set(key, batch); }
      // A dot is centred on a device pixel and a square is cornered on one; both get a whole number of pixels.
      const half = Math.max(1, Math.round(r * dpr));
      batch.values.push(node.origin === "doc" ? Math.round(x) : Math.round(x - half) + half, node.origin === "doc" ? Math.round(y) : Math.round(y - half) + half, half);
    } else single.push({ node, scale });
  }
  for (const batch of batches.values()) paintBatch(ctx, batch, dpr);
  const speck = Math.max(2, Math.round(dpr * 1.6));
  for (const bucket of tiny.values()) {
    ctx.globalAlpha = bucket.dim ? DIM_NODE : 1;
    ctx.fillStyle = bucket.fill;
    for (let i = 0; i < bucket.values.length; i += 2) ctx.fillRect(bucket.values[i]! - speck / 2, bucket.values[i + 1]! - speck / 2, speck, speck);
  }
  for (const { node, scale } of single) {
    const half = Math.max(1, Math.round(markPx(node, k) * scale * dpr));
    const x = toX(node.x), y = toY(node.y);
    const batch: Batch = { tone: node.tone, origin: node.origin, dim: dimmed(node), values: [node.origin === "doc" ? Math.round(x) : Math.round(x - half) + half, node.origin === "doc" ? Math.round(y) : Math.round(y - half) + half, half] };
    batch.alpha = (1 - node.retract) * (batch.dim ? DIM_NODE : 1);
    paintBatch(ctx, batch, dpr);
  }
  ctx.globalAlpha = 1;

  // 4. Rings: a change rings once (1px accent, 1x to 1.6x, fading over 400 ms); search hits, focus and selection get a still ring.
  ctx.lineWidth = hair;
  for (const node of nodes) {
    if (node.pulse <= 0 || node.kind !== "member" || !shown(node) || !inView(node.x, node.y, node.r * 3)) continue;
    const t = 1 - node.pulse;
    const half = markHalf(node, k, 1);
    const grow = 1 + 0.6 * ease(t);
    ctx.globalAlpha = 1 - ease(t);
    ctx.strokeStyle = rgba(ACCENT, 1);
    ctx.beginPath();
    ctx.arc(Math.round(toX(node.x)), Math.round(toY(node.y)), (half + 2) * grow * dpr, 0, TAU);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  const ring = (idx: number, alpha: number) => {
    const node = nodes[idx];
    if (!node || !shown(node) || node.kind !== "member") return;
    const half = markHalf(node, k, scene.grown(node)) + 3;
    ctx.strokeStyle = rgba(ACCENT, alpha);
    ctx.beginPath();
    const x = Math.round(toX(node.x)), y = Math.round(toY(node.y));
    const r = Math.round(half * dpr);
    ctx.arc(x, y, r, 0, TAU);
    ctx.stroke();
  };
  for (const idx of hits) ring(idx, 0.5);
  if (state.focus >= 0 && state.focus !== state.selected) ring(state.focus, 0.7);
  if (state.selected >= 0) ring(state.selected, 1);

  // 5. Labels, in screen space and on the pixel grid: a group's name above the top left of its outline (the count after it in tabular
  //    digits), a member's name beside its mark once the picture is large enough, 12px with a two pixel halo of the canvas colour.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.textBaseline = "alphabetic";
  ctx.lineJoin = "round";
  const taken: [number, number, number, number][] = [];
  const overlaps = (b: [number, number, number, number]) => taken.some((t) => b[0] < t[2] && b[2] > t[0] && b[1] < t[3] && b[3] > t[1]);
  const marks: [number, number, number, number][] = [];
  const labelAll = k >= LABEL_ZOOM || (k >= 0.8 && scene.memberCount <= 24);
  if (labelAll) for (const node of nodes) { if (node.kind === "member" && shown(node) && inView(node.x, node.y, node.r)) { const h = markHalf(node, k, 1) + 1; const x = node.x * k + camera.x, y = node.y * k + camera.y; marks.push([x - h, y - h, x + h, y + h]); } }
  const hitsMark = (b: [number, number, number, number]) => marks.some((m) => b[0] < m[2] && b[2] > m[0] && b[1] < m[3] && b[3] > m[1]);

  ctx.font = `500 13px ${FONT}`;
  const digit = ctx.measureText("0").width;
  labelsToDraw.sort((p, q) => Number(q.strong) - Number(p.strong) || q.w - p.w);
  for (const entry of labelsToDraw) {
    if (stats.labels >= MAX_LABELS) break;
    const { node, x, y } = entry;
    const text = node.label;
    const count = String(node.count);
    const glyph = node.source ? glyphPath(node.source) : null;
    const gw = glyph ? GLYPH_PX + 5 : 0;
    ctx.font = `500 13px ${FONT}`;
    const tw = ctx.measureText(text).width;
    const total = gw + tw + 6 + count.length * digit;
    // Too small a group has no room for its name; the rest are left out rather than printed over each other.
    if (!entry.strong && entry.w < Math.min(total, 28)) continue;
    const box: [number, number, number, number] = [x, y - 22, x + total, y - 4];
    if (!entry.strong && overlaps(box)) continue;
    if (x + total < 0 || x > width || y < 0 || y - 4 > height + 20) continue;
    taken.push(box);
    const base = y - 8;
    ctx.textAlign = "left";
    ctx.lineWidth = 4;
    ctx.strokeStyle = CANVAS;
    ctx.strokeText(text, x + gw, base);
    ctx.fillStyle = TEXT;
    ctx.fillText(text, x + gw, base);
    ctx.fillStyle = TEXT_WEAK;
    ctx.font = `500 13px ${FONT}`;
    ctx.textAlign = "center";
    for (let i = 0; i < count.length; i += 1) ctx.fillText(count[i]!, x + gw + tw + 6 + digit * i + digit / 2, base);
    ctx.textAlign = "left";
    const path = glyph;
    if (path) {
      ctx.save();
      ctx.translate(x, base - GLYPH_PX + 2);
      ctx.lineWidth = 1.2;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.strokeStyle = rgba(TONE_INK[node.tone], 0.9);
      ctx.stroke(path);
      ctx.restore();
    }
    stats.labels += 1;
  }

  ctx.font = `500 12px ${FONT}`;
  ctx.textAlign = "left";
  ctx.lineWidth = 4;
  ctx.strokeStyle = CANVAS;
  const wanted = (node: SceneNode) => node.idx === state.focus || node.idx === state.selected || hits.has(node.idx) || (lit !== null && lit.has(node.idx) && k > 1);
  const candidates: { node: SceneNode; strong: boolean }[] = [];
  if (labelAll || state.focus >= 0 || state.selected >= 0 || hits.size > 0 || lit !== null) {
    for (const node of nodes) {
      if (node.kind !== "member" || !shown(node) || node.appear < 0.6) continue;
      const strong = wanted(node);
      if (strong || labelAll) candidates.push({ node, strong });
    }
  }
  if (small) {
    // The names were placed in step 0.
    for (const [idx, spot] of spots) {
      const node = nodes[idx]!;
      const strong = wanted(node);
      ctx.globalAlpha = dimmed(node) && !strong ? 0.35 : 1;
      ctx.textAlign = spot.align;
      ctx.textBaseline = "middle";
      const tx = Math.round(spot.x * dpr) / dpr, ty = Math.round(spot.y * dpr) / dpr;
      ctx.lineWidth = 4;
      ctx.strokeStyle = CANVAS;
      ctx.strokeText(node.label, tx, ty);
      ctx.fillStyle = strong ? TEXT : TEXT_SECOND;
      ctx.fillText(node.label, tx, ty);
      stats.labels += 1;
    }
    ctx.globalAlpha = 1;
    candidates.length = 0;
  }
  candidates.sort((a, b) => Number(b.strong) - Number(a.strong) || b.node.r - a.node.r);
  for (const { node, strong } of candidates) {
    if (stats.labels >= MAX_LABELS) break;
    const sx = node.x * k + camera.x, sy = node.y * k + camera.y;
    if (sx < -80 || sx > width + 80 || sy < -20 || sy > height + 20) continue;
    if (dimmed(node) && !strong) continue;
    const half = markHalf(node, k, 1);
    const tw = ctx.measureText(node.label).width;
    // Beside the mark first, then below, above, and to its left: the first place where the name touches no other name and no other mark.
    const spots: [number, number, CanvasTextAlign][] = [[sx + half + 6, sy, "left"], [sx, sy + half + 12, "center"], [sx, sy - half - 12, "center"], [sx - half - 6, sy, "right"]];
    for (const [px, py, align] of spots) {
      const x0 = align === "left" ? px : align === "right" ? px - tw : px - tw / 2;
      const box: [number, number, number, number] = [x0 - 2, py - 8, x0 + tw + 2, py + 8];
      if (overlaps(box) || hitsMark(box)) continue;
      taken.push(box);
      ctx.textAlign = align;
      ctx.textBaseline = "middle";
      const tx = Math.round(px * dpr) / dpr, ty = Math.round(py * dpr) / dpr;
      ctx.strokeText(node.label, tx, ty);
      ctx.fillStyle = strong ? TEXT : TEXT_SECOND;
      ctx.fillText(node.label, tx, ty);
      stats.labels += 1;
      break;
    }
  }
  ctx.textBaseline = "alphabetic";
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  return stats;
}
