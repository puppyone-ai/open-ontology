import { fitCamera } from "./camera";
import { CANVAS, TONE_RGB, rgba } from "./palette";
import type { AtlasTone } from "./model";
import type { AtlasScene } from "./scene";

/**
 * The static atlas as SVG: the same cells, tints, hairline membranes and fit as the canvas, with no animation and no drift. It is what the
 * layout snapshot test compares (text, so a change of layout shows up in a diff) and what "export" would hand out. Numbers are rounded
 * to a tenth of a pixel so the output does not depend on floating point noise.
 */
const n = (value: number): string => String(Math.round(value * 10) / 10);

export function sceneToSvg(scene: AtlasScene, width: number, height: number): string {
  const camera = fitCamera(scene.bounds(), width, height);
  const px = (x: number) => n(x * camera.k + camera.x);
  const py = (y: number) => n(y * camera.k + camera.y);
  const tones = [...new Set(scene.nodes.map((node) => node.tone))].sort() as AtlasTone[];
  const defs = tones.flatMap((tone) => (["group", "member"] as const).map((kind) => {
    const [c, e] = kind === "group" ? [0.045, 0.13] : [0.16, 0.34];
    return `<radialGradient id="${kind}-${tone}" cx="36%" cy="32%" r="70%"><stop offset="0" stop-color="${rgba(TONE_RGB[tone], c)}"/><stop offset="1" stop-color="${rgba(TONE_RGB[tone], e)}"/></radialGradient>`;
  }));
  const out: string[] = [`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`, `<defs>${defs.join("")}</defs>`, `<rect width="${width}" height="${height}" fill="${CANVAS}"/>`];
  const visible = (node: { appear: number; alive: boolean }) => node.alive && node.appear > 0.001;
  out.push('<g fill="none" stroke="rgba(87, 83, 78, 0.3)" stroke-width="1" stroke-linecap="round">');
  for (const link of scene.groupLinks) {
    const a = scene.nodes[link.a]!, b = scene.nodes[link.b]!;
    if (!visible(a) || !visible(b) || (a.expanded && b.expanded)) continue;
    out.push(`<path d="M${px(a.x)} ${py(a.y)} L${px(b.x)} ${py(b.y)}"/>`);
  }
  for (const link of scene.links) {
    const a = scene.nodes[link.a]!, b = scene.nodes[link.b]!;
    if (!visible(a) || !visible(b) || a.appear < 0.5 || b.appear < 0.5) continue;
    out.push(`<path d="M${px(a.x)} ${py(a.y)} L${px(b.x)} ${py(b.y)}" stroke-width="0.6"/>`);
  }
  out.push("</g>");
  for (const node of scene.nodes) {
    if (!visible(node)) continue;
    const kind = node.kind;
    const [r, g, b] = TONE_RGB[node.tone];
    out.push(`<circle cx="${px(node.x)}" cy="${py(node.y)}" r="${n(node.r * scene.grown(node) * camera.k)}" fill="url(#${kind}-${node.tone})" stroke="rgba(${r}, ${g}, ${b}, ${kind === "group" ? 0.5 : 0.62})" stroke-width="1"/>`);
  }
  for (const node of scene.groups) {
    if (!visible(node)) continue;
    const above = node.expanded ? -node.r * camera.k - 11 : node.r * camera.k + 11;
    out.push(`<text x="${px(node.x)}" y="${n(Number(py(node.y)) + above)}" text-anchor="middle" font-size="12" font-weight="500" fill="#1c1917">${node.label.replace(/&/g, "&amp;").replace(/</g, "&lt;")}</text>`);
  }
  out.push("</svg>");
  return `${out.join("\n")}\n`;
}
