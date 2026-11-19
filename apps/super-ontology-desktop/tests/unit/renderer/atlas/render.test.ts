import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fitCamera } from "../../../../src/renderer/features/atlas/camera";
import { clearSpriteCache, drawAtlas, type DrawState } from "../../../../src/renderer/features/atlas/render";
import { AtlasScene } from "../../../../src/renderer/features/atlas/scene";
import { SimCore } from "../../../../src/renderer/features/atlas/simulation";
import { sceneToSvg } from "../../../../src/renderer/features/atlas/svg";
import { syntheticAtlas } from "../../../../src/renderer/features/atlas/lab/fixtures";

/** A context that records what was drawn: enough to check the picture without a browser. */
function recorder() {
  const calls: Record<string, number> = {};
  const target: Record<string, unknown> = {};
  const ctx = new Proxy(target, {
    get: (t, name: string) => {
      if (name in t) return t[name];
      if (name === "createRadialGradient") return () => ({ addColorStop: () => undefined });
      if (name === "measureText") return () => ({ width: 10 });
      return (..._args: unknown[]) => { calls[name] = (calls[name] ?? 0) + 1; };
    },
    set: (t, name: string, value) => { t[name] = value; return true; },
  }) as unknown as CanvasRenderingContext2D;
  return { ctx, calls };
}

const state = (scene: AtlasScene, over: Partial<DrawState> = {}): DrawState => ({ camera: fitCamera(scene.bounds(), 1000, 700), width: 1000, height: 700, dpr: 2, focus: -1, selected: -1, lit: null, hits: new Set(), time: 0, animate: false, ...over });
const settle = (scene: AtlasScene) => {
  const core = new SimCore();
  core.setNodes(scene.simSpecs(), "target");
  core.settle();
  const out = new Float32Array(scene.size * 2).fill(Number.NaN);
  core.copyPositions(out);
  scene.setPositions(out);
};

beforeEach(() => {
  clearSpriteCache();
  (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = class { constructor(public width: number, public height: number) {} getContext() { return recorder().ctx; } };
});
afterEach(() => { delete (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas; });

describe("atlas drawing", () => {
  it("draws every group, member and the labels that fit, and reports what it drew", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 5, members: 120, seed: 4 }));
    settle(scene);
    const { ctx, calls } = recorder();
    const stats = drawAtlas(ctx, scene, state(scene));
    expect(stats.members).toBe(scene.memberCount);
    expect(stats.labels).toBeGreaterThanOrEqual(5);
    expect(calls.arc).toBeGreaterThanOrEqual(5);
    expect(calls.drawImage).toBeGreaterThan(0);
  });

  it("culls what is outside the view", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 5, members: 400, seed: 4 }));
    settle(scene);
    const everything = drawAtlas(recorder().ctx, scene, state(scene));
    const zoomed = drawAtlas(recorder().ctx, scene, state(scene, { camera: { x: 0, y: 0, k: 6 } }));
    expect(zoomed.members).toBeLessThan(everything.members);
  });

  it("draws far cells as dots, not sprites, and no member labels when zoomed out", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 5, members: 400, seed: 4 }));
    settle(scene);
    const { ctx, calls } = recorder();
    const stats = drawAtlas(ctx, scene, state(scene, { camera: { x: 500, y: 350, k: 0.1 } }));
    expect(calls.drawImage ?? 0).toBe(0);
    expect(calls.fillRect).toBeGreaterThan(100);
    expect(stats.labels).toBeLessThan(10);
  });

  it("dims what is not near the focused cell and draws only that cell's links bright", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 4, members: 100, linkRatio: 2, seed: 9 }));
    settle(scene);
    const focus = scene.nodes.find((n) => n.kind === "member" && scene.neighbors(n.idx).size > 0)!;
    const lit = new Set<number>([focus.idx, ...scene.neighbors(focus.idx), focus.group]);
    const alphas = new Set<number>();
    const target: Record<string, unknown> = {};
    const ctx = new Proxy(target, {
      get: (t, name: string) => (name in t ? t[name] : name === "createRadialGradient" ? () => ({ addColorStop: () => undefined }) : () => undefined),
      set: (t, name: string, value) => { if (name === "globalAlpha") alphas.add(value as number); t[name] = value; return true; },
    }) as unknown as CanvasRenderingContext2D;
    const stats = drawAtlas(ctx, scene, state(scene, { focus: focus.idx, selected: focus.idx, lit }));
    expect([...alphas].some((a) => a > 0 && a < 0.4)).toBe(true);
    expect(stats.labels).toBeGreaterThan(0);
  });

  it("scales with the device pixel ratio: the canvas is cleared in device pixels", () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 2, members: 10, seed: 1 }));
    const rects: number[][] = [];
    const target: Record<string, unknown> = { fillRect: (...a: number[]) => rects.push(a) };
    const ctx = new Proxy(target, { get: (t, name: string) => (name in t ? t[name] : name === "createRadialGradient" ? () => ({ addColorStop: () => undefined }) : () => undefined), set: (t, n: string, v) => { t[n] = v; return true; } }) as unknown as CanvasRenderingContext2D;
    drawAtlas(ctx, scene, state(scene, { dpr: 3 }));
    expect(rects[0]).toEqual([0, 0, 3000, 2100]);
  });
});

describe("static layout snapshot", () => {
  it("draws the settled layout the same way every time", async () => {
    const scene = new AtlasScene(syntheticAtlas({ groups: 6, members: 90, linkRatio: 0.9, versions: 4, seed: 21, dbShare: 0.34 }));
    settle(scene);
    await expect(sceneToSvg(scene, 960, 600)).toMatchFileSnapshot("./__snapshots__/static-layout.svg");
  });

  it("is the same picture on a second run", () => {
    const make = () => { const s = new AtlasScene(syntheticAtlas({ groups: 6, members: 90, seed: 21 })); settle(s); return sceneToSvg(s, 960, 600); };
    expect(make()).toBe(make());
  });
});
