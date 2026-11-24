import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fitCamera } from "../../../../src/renderer/features/atlas/camera";
import { hullPadPx } from "../../../../src/renderer/features/atlas/hull";
import { clearSpriteCache, drawAtlas, markPx, type DrawState } from "../../../../src/renderer/features/atlas/render";
import { AtlasScene } from "../../../../src/renderer/features/atlas/scene";
import { SimCore } from "../../../../src/renderer/features/atlas/simulation";
import { sceneToSvg } from "../../../../src/renderer/features/atlas/svg";
import { shopAtlas, syntheticAtlas } from "../../../../src/renderer/features/atlas/lab/fixtures";

/** A context that records what was drawn and every style it was given: enough to check the picture without a browser. */
function recorder() {
  const calls: Record<string, number> = {};
  const sets: Record<string, unknown[]> = {};
  const texts: { text: string; x: number; y: number; font: string; align: string }[] = [];
  const rects: number[][] = [];
  const target: Record<string, unknown> = {};
  const ctx = new Proxy(target, {
    get: (t, name: string) => {
      if (name in t) return t[name];
      if (name === "measureText") return (text: string) => ({ width: text.length * 6 });
      if (name === "createPattern") return () => { calls.createPattern = (calls.createPattern ?? 0) + 1; return {}; };
      return (...args: unknown[]) => {
        calls[name] = (calls[name] ?? 0) + 1;
        if (name === "fillText") texts.push({ text: String(args[0]), x: Number(args[1]), y: Number(args[2]), font: String(target.font), align: String(target.textAlign) });
        if (name === "fillRect") rects.push(args as number[]);
      };
    },
    set: (t, name: string, value) => { (sets[name] ??= []).push(value); t[name] = value; return true; },
  }) as unknown as CanvasRenderingContext2D;
  return { ctx, calls, sets, texts, rects };
}

const state = (scene: AtlasScene, over: Partial<DrawState> = {}): DrawState => ({ camera: fitCamera(scene.bounds(), 1000, 700), width: 1000, height: 700, dpr: 2, focus: -1, selected: -1, lit: null, hits: new Set(), ...over });
const settle = (scene: AtlasScene) => {
  const core = new SimCore();
  core.setNodes(scene.simSpecs(), "target");
  core.settle();
  const out = new Float32Array(scene.size * 2).fill(Number.NaN);
  core.copyPositions(out);
  scene.setPositions(out);
};
const mixed = (members = 120) => new AtlasScene(syntheticAtlas({ groups: 5, members, linkRatio: 1.2, seed: 4, dbShare: 0.4 }));

beforeEach(() => {
  clearSpriteCache();
  (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = class { constructor(public width: number, public height: number) {} getContext() { return recorder().ctx; } };
});
afterEach(() => { delete (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas; });

describe("atlas drawing", () => {
  it("draws the marks in batches, the outlines of the groups and the names, and reports what it drew", () => {
    const scene = mixed();
    settle(scene);
    const { ctx, calls } = recorder();
    const stats = drawAtlas(ctx, scene, state(scene));
    expect(stats.members).toBe(scene.memberCount);
    expect(stats.labels).toBeGreaterThanOrEqual(4);
    expect(calls.arc).toBeGreaterThan(scene.memberCount / 2);
    expect(calls.roundRect).toBeGreaterThan(0);
    // One path per tone and kind of mark, never one fill per member.
    expect(calls.fill).toBeLessThan(scene.memberCount / 3);
  });

  it("has no gradient, no shadow, no blur and no glow anywhere", () => {
    const scene = mixed();
    settle(scene);
    const { ctx, calls, sets } = recorder();
    drawAtlas(ctx, scene, state(scene, { selected: scene.nodes.findIndex((n) => n.kind === "member"), focus: scene.nodes.findIndex((n) => n.kind === "member") }));
    for (const name of ["createRadialGradient", "createLinearGradient", "createConicGradient", "drawImage"]) expect(calls[name] ?? 0).toBe(0);
    for (const name of ["shadowBlur", "shadowColor", "filter"]) expect(sets[name]).toBeUndefined();
  });

  it("lays a dot grid of 8 pixels under everything and reuses it", () => {
    const scene = mixed();
    settle(scene);
    const { ctx, calls } = recorder();
    drawAtlas(ctx, scene, state(scene));
    drawAtlas(ctx, scene, state(scene, { camera: { x: 31, y: 17, k: 1 } }));
    expect(calls.createPattern).toBeLessThanOrEqual(1);
  });

  it("draws every line at a whole number of device pixels, at 1x and at 2x", () => {
    for (const dpr of [1, 2, 3]) {
      const scene = mixed();
      settle(scene);
      const { ctx, sets } = recorder();
      drawAtlas(ctx, scene, state(scene, { dpr, camera: { ...fitCamera(scene.bounds(), 1000, 700), k: 1.4 } }));
      const widths = (sets.lineWidth ?? []) as number[];
      // The 4 pixel halo of labels and the 1.2 pixel glyph outlines are text; every other line is a multiple of one CSS pixel.
      for (const width of widths.filter((w) => w !== 4 && w !== 1.2)) expect(Number.isInteger(width), `${width} at ${dpr}x`).toBe(true);
      expect(widths.some((w) => w === dpr)).toBe(true);
    }
  });

  it("culls what is outside the view", () => {
    const scene = mixed(400);
    settle(scene);
    const everything = drawAtlas(recorder().ctx, scene, state(scene));
    const zoomed = drawAtlas(recorder().ctx, scene, state(scene, { camera: { x: 0, y: 0, k: 6 } }));
    expect(zoomed.members).toBeLessThan(everything.members);
  });

  it("draws far cells as small squares of colour and no member names when zoomed out", () => {
    const scene = mixed(400);
    settle(scene);
    const { ctx, calls } = recorder();
    const stats = drawAtlas(ctx, scene, state(scene, { camera: { x: 500, y: 350, k: 0.1 } }));
    expect(calls.fillRect).toBeGreaterThan(100);
    expect(stats.labels).toBeLessThan(10);
  });

  it("names members only when zoomed in enough, with a canvas coloured halo", () => {
    const scene = mixed(60);
    settle(scene);
    const far = recorder();
    drawAtlas(far.ctx, scene, state(scene));
    const near = recorder();
    const camera = { ...fitCamera(scene.bounds(), 1000, 700), k: 3 };
    camera.x = 500 - scene.groups[0]!.x * 3; camera.y = 350 - scene.groups[0]!.y * 3;
    drawAtlas(near.ctx, scene, state(scene, { camera }));
    const names = (texts: { text: string }[]) => texts.filter((t) => scene.nodes.some((n) => n.kind === "member" && n.label === t.text)).length;
    expect(names(far.texts)).toBe(0);
    expect(names(near.texts)).toBeGreaterThan(3);
    expect(near.calls.strokeText).toBeGreaterThan(3);
    expect(near.sets.strokeStyle).toContain("#fafaf9");
    expect(near.texts.some((t) => t.font.startsWith("500 12px"))).toBe(true);
  });

  it("dims what is not near the focused cell and draws only that cell's links bright", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 4, members: 100, linkRatio: 2, seed: 9 }));
    settle(scene);
    const focus = scene.nodes.find((n) => n.kind === "member" && scene.neighbors(n.idx).size > 0)!;
    const lit = new Set<number>([focus.idx, ...scene.neighbors(focus.idx), focus.group]);
    const { ctx, sets } = recorder();
    const stats = drawAtlas(ctx, scene, state(scene, { focus: focus.idx, selected: focus.idx, lit }));
    expect((sets.globalAlpha as number[]).some((a) => a > 0 && a < 0.4)).toBe(true);
    expect((sets.strokeStyle as string[]).some((c) => c.startsWith("rgba(47, 74, 208, 0.6"))).toBe(true);
    expect(stats.labels).toBeGreaterThan(0);
  });

  it("uses the accent only for selection and focus", () => {
    const scene = mixed();
    settle(scene);
    const plain = recorder();
    drawAtlas(plain.ctx, scene, state(scene));
    const used = (sets: Record<string, unknown[]>) => [...(sets.strokeStyle ?? []), ...(sets.fillStyle ?? [])].filter((c) => typeof c === "string" && c.startsWith("rgba(47, 74, 208"));
    expect(used(plain.sets)).toHaveLength(0);
    const member = scene.nodes.find((n) => n.kind === "member")!;
    const picked = recorder();
    drawAtlas(picked.ctx, scene, state(scene, { selected: member.idx }));
    expect(used(picked.sets).length).toBeGreaterThan(0);
  });

  it("rings a changed cell once, growing from 1x to 1.6x and fading", () => {
    const scene = mixed();
    settle(scene);
    const node = scene.nodes.find((n) => n.kind === "member")!;
    scene.apply([{ type: "pulse", id: node.id }]);
    const radii: number[] = [];
    const alphas: number[] = [];
    for (const dt of [0.02, 0.15, 0.3]) {
      scene.step(dt);
      const arcs: number[] = [];
      const camera = state(scene, { dpr: 1 }).camera;
      const cx = Math.round(node.x * camera.k + camera.x), cy = Math.round(node.y * camera.k + camera.y);
      const target: Record<string, unknown> = {};
      const ctx = new Proxy(target, { get: (t, name: string) => (name in t ? t[name] : name === "measureText" ? () => ({ width: 10 }) : name === "arc" ? (...a: number[]) => { if (a[0] === cx && a[1] === cy) arcs.push(a[2]!); } : () => ({})), set: (t, name: string, v) => { if (name === "globalAlpha" && v < 1) alphas.push(v as number); t[name] = v; return true; } }) as unknown as CanvasRenderingContext2D;
      drawAtlas(ctx, scene, state(scene, { dpr: 1 }));
      radii.push(Math.max(...arcs));
    }
    expect(radii[1]!).toBeGreaterThan(radii[0]!);
    expect(alphas.length).toBeGreaterThan(0);
    scene.step(1);
    expect(node.pulse).toBe(0);
  });

  it("scales with the device pixel ratio: the canvas is cleared in device pixels", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 2, members: 10, seed: 1 }));
    const { ctx, rects } = recorder();
    drawAtlas(ctx, scene, state(scene, { dpr: 3 }));
    expect(rects[0]).toEqual([0, 0, 3000, 2100]);
  });
});

describe("links, marks and zoom", () => {
  it("draws one summary line per pair of groups in the overview, and single links only inside open groups", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 5, members: 300, linkRatio: 1.5, seed: 4, dbShare: 0 }));
    settle(scene);
    const overview = drawAtlas(recorder().ctx, scene, state(scene));
    const pairs = scene.groupLinks.length;
    const inside = scene.links.filter((l) => scene.nodes[l.a]!.group === scene.nodes[l.b]!.group).length;
    expect(overview.links).toBeLessThanOrEqual(pairs + inside);
    expect(overview.links).toBeLessThan(scene.links.length);
    const camera = { ...fitCamera(scene.bounds(), 1000, 700), k: 2.2 };
    const zoomed = drawAtlas(recorder().ctx, scene, state(scene, { camera: { ...camera, x: 500 - scene.groups[0]!.x * 2.2, y: 350 - scene.groups[0]!.y * 2.2 } }));
    expect(zoomed.links).toBeGreaterThan(0);
  });

  it("keeps a mark between 6 and 10 pixels across at any zoom, and lets it shrink only when far out", () => {
    const scene = mixed(60);
    const node = scene.nodes.find((n) => n.kind === "member")!;
    for (const k of [1, 2, 4, 8, 12]) { const half = markPx(node, k); expect(half).toBeGreaterThanOrEqual(2.5); expect(half).toBeLessThanOrEqual(5); }
    expect(markPx(node, 0.1)).toBeLessThan(2);
    const big = scene.nodes.filter((n) => n.kind === "member").sort((a, b) => b.r - a.r)[0]!;
    expect(markPx(big, 12) * 2).toBeLessThanOrEqual(10);
  });

  it("draws arrowheads of 8 pixels at 1x", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 1, members: 4, linkRatio: 1, seed: 2, dbShare: 1 }));
    settle(scene);
    const lines: number[][] = [];
    const target: Record<string, unknown> = {};
    const ctx = new Proxy(target, { get: (t, name: string) => (name in t ? t[name] : name === "measureText" ? () => ({ width: 10 }) : () => undefined), set: (t, name: string, v) => { t[name] = v; return true; } }) as unknown as CanvasRenderingContext2D;
    (globalThis as { Path2D?: unknown }).Path2D = class { moveTo(...a: number[]) { lines.push(a); } lineTo(...a: number[]) { lines.push(a); } closePath() { /* none */ } };
    try { drawAtlas(ctx, scene, state(scene, { dpr: 1, camera: { ...fitCamera(scene.bounds(), 1000, 700) } })); } finally { delete (globalThis as { Path2D?: unknown }).Path2D; }
    const tips = lines.filter((_, i) => i % 3 === 0);
    expect(tips.length).toBeGreaterThan(0);
    const back = lines[1]!, tip = lines[0]!;
    expect(Math.hypot(back[0]! - tip[0]!, back[1]! - tip[1]!)).toBeGreaterThanOrEqual(8);
  });
});

describe("a small database picture", () => {
  it("names every table, the group above all of them, and keeps each name off the links", () => {
    const scene = new AtlasScene(shopAtlas());
    settle(scene);
    const camera = { k: 3.6, x: 500, y: 250 };
    const { ctx, texts } = recorder();
    drawAtlas(ctx, scene, state(scene, { camera, dpr: 2, width: 990, height: 466 }));
    for (const name of ["customers", "products", "orders", "order_items"]) expect(texts.some((t) => t.text === name), name).toBe(true);
    const group = texts.find((t) => t.text === scene.groups[0]!.label)!;
    for (const name of ["customers", "products", "orders", "order_items"]) expect(group.y).toBeLessThan(texts.find((t) => t.text === name)!.y);
    // A name's box (12px text, about 6px a letter) does not meet any link between the tables.
    const at = (label: string) => { const n = scene.nodes.find((x) => x.label === label)!; return { x: n.x * camera.k + camera.x, y: n.y * camera.k + camera.y }; };
    const links: [string, string][] = [["orders", "customers"], ["order_items", "orders"], ["order_items", "products"]];
    for (const text of texts.filter((t) => ["customers", "products", "orders", "order_items"].includes(t.text))) {
      const width = text.text.length * 6;
      for (const [from, to] of links) {
        const a = at(from), b = at(to);
        for (let i = 0; i <= 40; i += 1) {
          const x = a.x + ((b.x - a.x) * i) / 40, y = a.y + ((b.y - a.y) * i) / 40;
          const left = text.align === "right" ? text.x - width : text.align === "center" ? text.x - width / 2 : text.x;
          const inside = x > left - 1 && x < left + width + 1 && Math.abs(y - text.y) < 6;
          const own = Math.hypot(x - at(text.text).x, y - at(text.text).y) < 12;
          if (inside && !own) expect.fail(`${text.text} meets the link ${from} to ${to}`);
        }
      }
    }
  });
});

describe("group labels", () => {
  it("sit above the top left of their outline at every zoom, never on a member", () => {
    for (const zoom of [0.6, 1, 2.5]) {
      const scene = new AtlasScene(syntheticAtlas({ groups: 4, members: 160, seed: 6, dbShare: 0 }));
      settle(scene);
      const camera = { k: zoom, x: 500, y: 350 };
      const { ctx, texts } = recorder();
      drawAtlas(ctx, scene, state(scene, { camera, dpr: 1 }));
      let seen = 0;
      for (const group of scene.groups) {
        const label = texts.find((t) => t.text === group.label && t.font.startsWith("500 13px"));
        if (!label) continue;
        seen += 1;
        const hull = scene.hullOf(group);
        const pad = hullPadPx(zoom) / zoom;
        const top = (group.y + Math.min(...hull.map((p) => p.y)) - pad) * zoom + camera.y;
        const leftEdge = (group.x + Math.min(...hull.map((p) => p.x)) - pad) * zoom + camera.x;
        expect(label.y, `${group.label} at zoom ${zoom}`).toBeLessThan(top);
        expect(Math.abs(label.x - leftEdge)).toBeLessThanOrEqual(22);
      }
      expect(seen).toBeGreaterThan(0);
    }
  });

  it("show the count after the name in digits of one width", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 3, members: 40, seed: 2, dbShare: 0 }));
    settle(scene);
    const { ctx, texts } = recorder();
    drawAtlas(ctx, scene, state(scene, { dpr: 1 }));
    const group = scene.groups[0]!;
    const digits = texts.filter((t) => /^\d$/.test(t.text));
    expect(digits.length).toBeGreaterThan(0);
    const label = texts.find((t) => t.text === group.label)!;
    const own = digits.filter((t) => Math.abs(t.y - label.y) < 0.5 && t.x > label.x);
    expect(own.map((t) => t.text).join("")).toBe(String(group.count));
    for (let i = 1; i < own.length; i += 1) expect(own[i]!.x - own[i - 1]!.x).toBeCloseTo(6, 5);
  });
});

describe("static layout snapshot", () => {
  it("draws the settled layout the same way every time", async () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 6, members: 90, linkRatio: 0.9, versions: 4, seed: 21, dbShare: 0.34 }));
    settle(scene);
    await expect(sceneToSvg(scene, 960, 600)).toMatchFileSnapshot("./__snapshots__/static-layout.svg");
  });

  it("draws the not-connected group beside the picture with a dashed outline", async () => {
    const data = syntheticAtlas({ groups: 5, members: 60, linkRatio: 0.9, versions: 3, seed: 21 });
    const lone = Array.from({ length: 7 }, (_, i) => ({ id: `e:lone${i}`, group: "g:unlinked", label: `lone${i}`, ref: { kind: "entity" as const, key: `lone${i}` }, weight: 0, born: null }));
    const scene = new AtlasScene({ ...data, groups: [...data.groups, { id: "g:unlinked", kind: "unlinked", label: "Not connected", tone: "stone" }], members: [...data.members, ...lone] });
    settle(scene);
    const svg = sceneToSvg(scene, 960, 600);
    expect(svg).toContain('stroke-dasharray="4 4"');
    await expect(svg).toMatchFileSnapshot("./__snapshots__/static-layout-unlinked.svg");
  });

  it("is the same picture on a second run", () => {
    const make = () => { const s = new AtlasScene(syntheticAtlas({ groups: 6, members: 90, seed: 21 })); settle(s); return sceneToSvg(s, 960, 600); };
    expect(make()).toBe(make());
  });
});
