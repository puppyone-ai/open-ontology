import type { Camera } from "./camera";
import { CANVAS, TEXT, TEXT_WEAK, TONE_RGB, rgba, type Rgb } from "./palette";
import type { AtlasTone } from "./model";
import type { AtlasScene, SceneNode } from "./scene";

/**
 * Canvas 2D drawing of the atlas (25 section 4 and 7.3): light canvas, translucent cells with a radial tint, a hairline membrane and
 * a short highlight arc. Members are drawn from small cached sprites (one drawImage each), so ten thousand cells cost a loop and not
 * ten thousand gradients. Everything is device-pixel aware: line widths are 1 CSS pixel at every zoom.
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
  time: number;
  /** Ambient drift and the slow pulse. False for reduced motion and for the static picture. */
  animate: boolean;
};
export type DrawStats = { members: number; labels: number; links: number };

const TAU = Math.PI * 2;
const MAX_LABELS = 260;

const STYLE = {
  group: { center: 0.045, edge: 0.13, ring: 0.5 },
  member: { center: 0.16, edge: 0.34, ring: 0.62 },
} as const;

/** One cell: radial tint (lighter towards the upper left), the membrane, and a short highlight arc on the membrane. */
export function paintCell(ctx: CanvasRenderingContext2D, cx: number, cy: number, r: number, rgb: Rgb, kind: "group" | "member", line: number): void {
  const style = STYLE[kind];
  const gradient = ctx.createRadialGradient(cx - r * 0.28, cy - r * 0.32, r * 0.08, cx, cy, r);
  gradient.addColorStop(0, rgba(rgb, style.center));
  gradient.addColorStop(1, rgba(rgb, style.edge));
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, TAU);
  ctx.fillStyle = gradient;
  ctx.fill();
  ctx.lineWidth = line;
  ctx.strokeStyle = rgba(rgb, style.ring);
  ctx.stroke();
  if (r > line * 6) {
    ctx.beginPath();
    ctx.arc(cx, cy, r - line * 1.6, -2.55, -1.95);
    ctx.lineWidth = line * 1.3;
    ctx.lineCap = "round";
    ctx.strokeStyle = "rgba(255, 255, 255, 0.95)";
    ctx.stroke();
    ctx.lineCap = "butt";
  }
}

// ---- member sprites ---------------------------------------------------------------------------------------------

type Surface = HTMLCanvasElement | OffscreenCanvas;
const sprites = new Map<string, { surface: Surface; half: number; radius: number }>();
const makeSurface = (size: number): Surface => (typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(size, size) : Object.assign(document.createElement("canvas"), { width: size, height: size }));

/** A cell of device radius ~`devRadius`, rounded to a quarter octave so a zoom does not rebuild sprites every frame. */
function spriteFor(tone: AtlasTone, devRadius: number, dpr: number) {
  const bucket = Math.max(0, Math.round(Math.log2(Math.max(1, devRadius)) * 4));
  const key = `${tone}|${bucket}|${dpr}`;
  let sprite = sprites.get(key);
  if (!sprite) {
    const radius = 2 ** (bucket / 4);
    const size = Math.ceil(radius * 2 + dpr * 4);
    const surface = makeSurface(size);
    const ctx = surface.getContext("2d") as CanvasRenderingContext2D;
    paintCell(ctx, size / 2, size / 2, radius, TONE_RGB[tone], "member", dpr);
    sprite = { surface, half: size / 2, radius };
    sprites.set(key, sprite);
    if (sprites.size > 400) { const oldest = sprites.keys().next().value; if (oldest !== undefined) sprites.delete(oldest); }
  }
  return sprite;
}

/** 12px kind glyphs of a source (My files, folder, bucket, database): simple outlines on a 12 unit grid, drawn with the label. */
const GLYPHS: Record<string, string> = {
  files: "M3 1.5h4.2L9.5 4v6.5H3zM7.2 1.5V4h2.3",
  folder: "M1.5 3h3l1.2 1.3h4.8v5.7h-9z",
  bucket: "M2 3.6c0 .9 1.8 1.4 4 1.4s4-.5 4-1.4-1.8-1.4-4-1.4-4 .5-4 1.4zM2 3.6l1 6.6h6l1-6.6",
  database: "M2 3c0 .9 1.8 1.6 4 1.6S10 3.9 10 3 8.2 1.4 6 1.4 2 2.1 2 3zM2 3v6c0 .9 1.8 1.6 4 1.6S10 9.9 10 9V3M2 6c0 .9 1.8 1.6 4 1.6S10 6.9 10 6",
};
const glyphPaths = new Map<string, Path2D>();
const glyphPath = (kind: string): Path2D | null => {
  if (typeof Path2D === "undefined" || !GLYPHS[kind]) return null;
  let path = glyphPaths.get(kind);
  if (!path) { path = new Path2D(GLYPHS[kind]); glyphPaths.set(kind, path); }
  return path;
};
export const GLYPH_PX = 12;

export const clearSpriteCache = (): void => sprites.clear();

// ---- motion -----------------------------------------------------------------------------------------------------

/** Ambient drift (a sinusoid per cell, about a pixel) and a 1.5 percent breathing of the radius. Pure of the time. */
const drift = (node: SceneNode, time: number, animate: boolean): { dx: number; dy: number; scale: number } => {
  if (!animate) return { dx: 0, dy: 0, scale: 1 };
  const p = node.phase * TAU;
  const amp = node.kind === "group" ? 0.9 : 1.4;
  return { dx: Math.sin(time * 0.37 + p) * amp, dy: Math.cos(time * 0.29 + p * 1.7) * amp, scale: 1 + 0.015 * Math.sin(time * 1.3 + p) };
};

const radiusOf = (scene: AtlasScene, node: SceneNode, scale: number): number => {
  // The common case, a settled cell: no easing, no sines.
  if (node.appear === 1 && node.pulse === 0 && !node.retracting && scene.breathe === 0) return node.r * scale;
  const grown = scene.grown(node);
  const pulse = 1 + 0.14 * Math.sin(Math.min(1, node.pulse) * Math.PI) ;
  const breathe = 1 + 0.035 * Math.sin(Math.min(1, 1 - scene.breathe) * Math.PI) * (scene.breathe > 0 ? 1 : 0);
  const retract = node.retracting ? 1 - 0.6 * node.retract : 1;
  return node.r * grown * pulse * breathe * scale * retract;
};

export function drawAtlas(ctx: CanvasRenderingContext2D, scene: AtlasScene, state: DrawState): DrawStats {
  const { camera, width, height, dpr, lit, hits, time, animate } = state;
  const stats: DrawStats = { members: 0, labels: 0, links: 0 };
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  ctx.fillStyle = CANVAS;
  ctx.fillRect(0, 0, width * dpr, height * dpr);
  ctx.setTransform(dpr * camera.k, 0, 0, dpr * camera.k, dpr * camera.x, dpr * camera.y);

  const k = camera.k;
  const hairline = 1 / k;
  const left = -camera.x / k, top = -camera.y / k, right = (width - camera.x) / k, bottom = (height - camera.y) / k;
  const inView = (x: number, y: number, r: number) => x + r >= left && x - r <= right && y + r >= top && y - r <= bottom;
  const dimmed = (node: SceneNode) => lit !== null && !lit.has(node.idx);
  const nodes = scene.nodes;
  const shown = (node: SceneNode) => scene.isVisible(node);
  const place = (node: SceneNode) => {
    if (!animate) return { x: node.x, y: node.y, scale: 1 };
    const d = drift(node, time, animate);
    return { x: node.x + d.dx, y: node.y + d.dy, scale: d.scale };
  };

  // 1. Links, behind everything: gentle curves, very light. Between groups they are aggregated (width by how many links they stand for);
  //    links between single cells only appear around the focused or selected cell.
  ctx.lineCap = "round";
  const stone = TONE_RGB.stone;
  for (const link of scene.groupLinks) {
    const a = nodes[link.a]!, b = nodes[link.b]!;
    if (!shown(a) || !shown(b)) continue;
    const pa = place(a), pb = place(b);
    if (!inView(pa.x, pa.y, a.r) && !inView(pb.x, pb.y, b.r)) continue;
    const near = lit === null || (lit.has(a.idx) && lit.has(b.idx));
    ctx.globalAlpha = near ? 1 : 0.5;
    ctx.lineWidth = Math.min(5, 0.9 + Math.log2(1 + link.count) * 0.55) / k;
    ctx.strokeStyle = rgba(near && lit !== null ? TONE_RGB.ink : stone, near && lit !== null ? 0.3 : 0.09);
    const mx = (pa.x + pb.x) / 2, my = (pa.y + pb.y) / 2;
    const bend = 0.16;
    ctx.beginPath();
    ctx.moveTo(pa.x, pa.y);
    ctx.quadraticCurveTo(mx - (pb.y - pa.y) * bend, my + (pb.x - pa.x) * bend, pb.x, pb.y);
    ctx.stroke();
    stats.links += 1;
  }
  if (lit !== null) {
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1.2 / k;
    ctx.strokeStyle = rgba(TONE_RGB.ink, 0.42);
    ctx.beginPath();
    for (const link of scene.links) {
      const a = nodes[link.a]!, b = nodes[link.b]!;
      if (!(lit.has(a.idx) && lit.has(b.idx) && a.appear > 0.5 && b.appear > 0.5 && a.alive && b.alive)) continue;
      const pa = place(a), pb = place(b);
      const mx = (pa.x + pb.x) / 2, my = (pa.y + pb.y) / 2;
      ctx.moveTo(pa.x, pa.y);
      ctx.quadraticCurveTo(mx - (pb.y - pa.y) * 0.12, my + (pb.x - pa.x) * 0.12, pb.x, pb.y);
      stats.links += 1;
    }
    ctx.stroke();
  }
  ctx.lineCap = "butt";
  ctx.globalAlpha = 1;

  // 2. Group cells (behind their members).
  for (const node of scene.groups) {
    if (!shown(node)) continue;
    const p = place(node);
    const r = radiusOf(scene, node, p.scale);
    if (r <= 0 || !inView(p.x, p.y, r)) continue;
    ctx.globalAlpha = (dimmed(node) ? 0.3 : 1) * (1 - 0.6 * node.retract);
    paintCell(ctx, p.x, p.y, r, TONE_RGB[node.tone], "group", hairline);
    if (node.pulse > 0) {
      ctx.beginPath();
      ctx.arc(p.x, p.y, r * (1 + 0.1 * (1 - node.pulse)), 0, TAU);
      ctx.lineWidth = 1.5 / k;
      ctx.strokeStyle = rgba(TONE_RGB[node.tone], 0.5 * node.pulse);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // 3. Member cells: sprites when they are big enough to see, a dot otherwise. Drawn in device pixels so a sprite is never resampled twice.
  const dpk = dpr * k;
  const dotBuckets = new Map<AtlasTone, number[]>();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  for (const node of nodes) {
    if (node.kind !== "member" || !shown(node)) continue;
    const p = place(node);
    const r = radiusOf(scene, node, p.scale);
    if (r <= 0 || !inView(p.x, p.y, r)) continue;
    stats.members += 1;
    const alpha = (dimmed(node) ? 0.25 : 1) * (1 - 0.85 * node.retract);
    const sx = (p.x * k + camera.x) * dpr, sy = (p.y * k + camera.y) * dpr;
    if (r * k < 1.2) {
      let bucket = dotBuckets.get(node.tone);
      if (!bucket) { bucket = []; dotBuckets.set(node.tone, bucket); }
      bucket.push(sx, sy, Math.max(r * dpk, 0.7 * dpr), alpha);
      continue;
    }
    const sprite = spriteFor(node.tone, r * dpk, dpr);
    const scale = (r * dpk) / sprite.radius;
    ctx.globalAlpha = alpha;
    ctx.drawImage(sprite.surface as CanvasImageSource, sx - sprite.half * scale, sy - sprite.half * scale, sprite.half * 2 * scale, sprite.half * 2 * scale);
  }
  for (const [tone, values] of dotBuckets) {
    ctx.fillStyle = rgba(TONE_RGB[tone], 0.55);
    for (let i = 0; i < values.length; i += 4) {
      const r = values[i + 2]!;
      ctx.globalAlpha = values[i + 3]!;
      ctx.fillRect(values[i]! - r, values[i + 1]! - r, r * 2, r * 2);
    }
  }
  ctx.globalAlpha = 1;
  ctx.setTransform(dpk, 0, 0, dpk, dpr * camera.x, dpr * camera.y);

  // 4. Focus, selection and search hits: an accent ring around the cell.
  const ring = (idx: number, width: number, color: string) => {
    const node = nodes[idx];
    if (!node || !shown(node)) return;
    const p = place(node);
    const r = radiusOf(scene, node, p.scale) + 2.5 / k;
    ctx.beginPath();
    ctx.arc(p.x, p.y, r, 0, TAU);
    ctx.lineWidth = width / k;
    ctx.strokeStyle = color;
    ctx.stroke();
  };
  for (const idx of hits) ring(idx, 1.5, rgba(TONE_RGB.ink, 0.55));
  if (state.selected >= 0) ring(state.selected, 2, rgba(TONE_RGB.ink, 0.9));
  if (state.focus >= 0 && state.focus !== state.selected) ring(state.focus, 1.5, rgba(TONE_RGB.ink, 0.7));

  // 5. Labels, in screen space: only where there is room, or for what is focused, found or selected.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.font = '500 12px -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif';
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  const taken: [number, number, number, number][] = [];
  const label = (text: string, sx: number, sy0: number, strong: boolean, color: string, glyph?: { kind: string; rgb: Rgb }) => {
    const tw = ctx.measureText(text).width;
    const gw = glyph ? GLYPH_PX + 5 : 0;
    const w = tw + gw + 8, h = 16;
    const box = (cy: number): [number, number, number, number] => [sx - w / 2, cy - h / 2, sx + w / 2, cy + h / 2];
    const overlaps = (b: [number, number, number, number]) => taken.some((t) => b[0] < t[2] && b[2] > t[0] && b[1] < t[3] && b[3] > t[1]);
    let sy = sy0;
    let b = box(sy);
    // A label that matters steps aside (further down, never onto its own cell) for another one; an ordinary one is simply left out.
    for (let attempt = 0; overlaps(b); attempt += 1) {
      if (!strong || attempt >= 4) return;
      sy = sy0 + h * (attempt + 1);
      b = box(sy);
    }
    taken.push(b);
    const left = Math.round(sx - (tw + gw) / 2);
    const y = Math.round(sy);
    ctx.textAlign = "left";
    ctx.lineWidth = 4;
    ctx.strokeStyle = "rgba(250, 250, 249, 0.94)";
    ctx.strokeText(text, left + gw, y);
    ctx.fillStyle = color;
    ctx.fillText(text, left + gw, y);
    ctx.textAlign = "center";
    const path = glyph ? glyphPath(glyph.kind) : null;
    if (glyph && path) {
      ctx.save();
      ctx.translate(left, y - GLYPH_PX / 2);
      ctx.lineWidth = 1.2;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.strokeStyle = rgba(glyph.rgb, 0.9);
      ctx.stroke(path);
      ctx.restore();
    }
    stats.labels += 1;
  };
  const wanted = (node: SceneNode) => node.idx === state.focus || node.idx === state.selected || hits.has(node.idx) || (lit !== null && lit.has(node.idx) && node.kind === "member" && k > 0.5);
  const candidates: { node: SceneNode; strong: boolean }[] = [];
  for (const node of nodes) {
    if (!shown(node) || (node.kind === "member" && node.appear < 0.6)) continue;
    const strong = wanted(node);
    const screenR = node.r * k;
    if (strong || (node.kind === "group" && screenR > 20) || (node.kind === "member" && screenR > 11)) candidates.push({ node, strong });
  }
  candidates.sort((a, b) => Number(b.strong) - Number(a.strong) || b.node.r - a.node.r);
  for (const { node, strong } of candidates) {
    if (stats.labels >= MAX_LABELS) break;
    const p = place(node);
    const sx = Math.round(p.x * k + camera.x), sy = Math.round(p.y * k + camera.y);
    if (sx < -60 || sx > width + 60 || sy < -20 || sy > height + 20) continue;
    const FONT = '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif';
    ctx.font = node.kind === "group" ? `600 13px ${FONT}` : `500 12px ${FONT}`;
    if (node.kind === "group") {
      {
        // Always just below the cell, so no glyph ever lands on a member; strong so it steps aside for a neighbour instead of vanishing.
        label(node.label, sx, sy + node.r * k + 12, true, TEXT, node.source ? { kind: node.source, rgb: TONE_RGB[node.tone] } : undefined);
        if (!node.expanded && node.r * k > 18) { ctx.fillStyle = rgba(TONE_RGB[node.tone], 0.95); ctx.font = '600 13px -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif'; ctx.fillText(String(node.count), sx, sy); ctx.font = '500 12px -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif'; }
      }
    } else label(node.label, sx, sy + node.r * k + 10, strong, strong ? TEXT : TEXT_WEAK);
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  return stats;
}
