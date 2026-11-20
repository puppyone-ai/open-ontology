import { fitCamera } from "./camera";
import { hullArcs, hullPadPx, type Pt } from "./hull";
import { CANVAS, TEXT, TEXT_WEAK, TONE_FILL, TONE_INK, rgba } from "./palette";
import type { AtlasScene } from "./scene";

/**
 * The static atlas as SVG: the same marks, outlines, links and fit as the canvas, with no animation. It is what the layout snapshot test
 * compares (text, so a change of layout shows up in a diff) and what "export" would hand out. Numbers are rounded to a tenth of a pixel
 * so the output does not depend on floating point noise.
 */
const n = (value: number): string => String(Math.round(value * 10) / 10);
const MARK = 0.8;

export function sceneToSvg(scene: AtlasScene, width: number, height: number): string {
  const camera = fitCamera(scene.bounds(), width, height);
  const k = camera.k;
  const px = (x: number) => x * k + camera.x;
  const py = (y: number) => y * k + camera.y;
  const out: string[] = [`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`];
  out.push(`<defs><pattern id="grid" width="8" height="8" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="rgba(28, 25, 23, 0.07)"/></pattern></defs>`);
  out.push(`<rect width="${width}" height="${height}" fill="${CANVAS}"/>`, `<rect width="${width}" height="${height}" fill="url(#grid)"/>`);
  const visible = (node: { appear: number; alive: boolean }) => node.alive && node.appear > 0.001;

  // Outlines.
  for (const group of scene.groups) {
    if (!visible(group)) continue;
    const local = scene.hullOf(group);
    if (local.length === 0) continue;
    const radius = (group.expanded ? hullPadPx(k) : Math.max(4, group.r * 0.28) * k);
    const pts: Pt[] = local.map((p) => ({ x: px(group.x + p.x), y: py(group.y + p.y) }));
    const arcs = hullArcs(pts);
    const at = (a: { x: number; y: number }, angle: number) => `${n(a.x + radius * Math.cos(angle))} ${n(a.y + radius * Math.sin(angle))}`;
    let d: string;
    if (arcs.length === 1) d = `M${n(pts[0]!.x + radius)} ${n(pts[0]!.y)} a${n(radius)} ${n(radius)} 0 1 1 ${n(-2 * radius)} 0 a${n(radius)} ${n(radius)} 0 1 1 ${n(2 * radius)} 0Z`;
    else d = `${arcs.map((a, i) => `${i === 0 ? "M" : "L"}${at(a, a.a0)} A${n(radius)} ${n(radius)} 0 ${a.a1 - a.a0 > Math.PI ? 1 : 0} 1 ${at(a, a.a1)}`).join(" ")}Z`;
    out.push(`<path d="${d}" fill="${rgba(TONE_INK[group.tone], group.expanded ? 0.03 : 0.06)}" stroke="${rgba(TONE_INK[group.tone], 0.4)}" stroke-width="1"/>`);
  }

  // Links: straight, one pixel, from the edge of one mark to the edge of the other.
  out.push(`<g fill="none" stroke="${rgba(TONE_INK.stone, 0.12)}" stroke-width="1">`);
  const reach = (node: { r: number }) => Math.max(1, Math.min(node.r * MARK * k, 3 + (node.r - 3.2) * 0.625)) + 2;
  for (const link of scene.links) {
    const a = scene.nodes[link.a]!, b = scene.nodes[link.b]!;
    if (!visible(a) || !visible(b) || a.appear < 0.5 || b.appear < 0.5) continue;
    const dx = px(b.x) - px(a.x), dy = py(b.y) - py(a.y), len = Math.hypot(dx, dy);
    const ta = reach(a), tb = reach(b);
    if (len <= ta + tb + 1) continue;
    const ux = dx / len, uy = dy / len;
    out.push(`<path d="M${n(px(a.x) + ux * ta)} ${n(py(a.y) + uy * ta)} L${n(px(b.x) - ux * tb)} ${n(py(b.y) - uy * tb)}"/>`);
  }
  out.push("</g>");

  // Marks: a dot for a document, a rounded square for a database, a dot inside a square ring for both.
  for (const node of scene.nodes) {
    if (node.kind !== "member" || !visible(node)) continue;
    const x = px(node.x), y = py(node.y);
    const half = Math.max(1, Math.min(node.r * MARK * k, 3 + (node.r - 3.2) * 0.625) * scene.grown(node));
    const fill = TONE_FILL[node.tone];
    if (node.origin === "doc") out.push(`<circle cx="${n(x)}" cy="${n(y)}" r="${n(half)}" fill="${fill}" stroke="#ffffff" stroke-width="1"/>`);
    else if (node.origin === "db") out.push(`<rect x="${n(x - half)}" y="${n(y - half)}" width="${n(half * 2)}" height="${n(half * 2)}" rx="2" fill="${fill}" stroke="#ffffff" stroke-width="1"/>`);
    else out.push(`<rect x="${n(x - half - 3)}" y="${n(y - half - 3)}" width="${n(half * 2 + 6)}" height="${n(half * 2 + 6)}" rx="2" fill="none" stroke="${rgba(TONE_INK[node.tone], 0.9)}" stroke-width="1"/><circle cx="${n(x)}" cy="${n(y)}" r="${n(half * 0.62)}" fill="${fill}"/>`);
  }

  // Group names above the top left of their outlines, the count after them in a quieter colour.
  for (const group of scene.groups) {
    if (!visible(group)) continue;
    const local = scene.hullOf(group);
    if (local.length === 0) continue;
    const pad = group.expanded ? hullPadPx(k) / k : Math.max(4, group.r * 0.28);
    const x = px(group.x + Math.min(...local.map((p) => p.x)) - pad), y = py(group.y + Math.min(...local.map((p) => p.y)) - pad);
    out.push(`<text x="${n(x)}" y="${n(y - 8)}" font-size="13" font-weight="500" fill="${TEXT}" style="font-variant-numeric: tabular-nums">${group.label.replace(/&/g, "&amp;").replace(/</g, "&lt;")}<tspan dx="6" fill="${TEXT_WEAK}">${group.count}</tspan></text>`);
  }
  out.push("</svg>");
  return `${out.join("\n")}\n`;
}
