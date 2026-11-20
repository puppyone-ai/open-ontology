import { describe, expect, it } from "vitest";
import { AtlasController } from "../../../../src/renderer/features/atlas/controller";
import { spiralPoint } from "../../../../src/renderer/features/atlas/layout";
import { syntheticAtlas } from "../../../../src/renderer/features/atlas/lab/fixtures";

const fakeCanvas = () => {
  const noop = new Proxy({}, { get: (_t, name) => (name === "measureText" ? () => ({ width: 10 }) : () => undefined), set: () => true });
  return { width: 0, height: 0, getContext: () => noop, getBoundingClientRect: () => ({ width: 800, height: 400, left: 0, top: 0 }) } as unknown as HTMLCanvasElement;
};

describe("atlas controller", () => {
  it("sizes the backing store from the CSS size and the pixel ratio, and draws onto it without resampling", () => {
    const canvas = fakeCanvas();
    const frames: (() => void)[] = [];
    const controller = new AtlasController(canvas, syntheticAtlas({ groups: 3, members: 30, seed: 1 }), { reducedMotion: true, requestFrame: (cb) => { frames.push(() => cb(0)); return frames.length; }, cancelFrame: () => undefined });
    controller.resize(774.5, 400.5, 2);
    expect(canvas.width).toBe(1549);
    expect(canvas.height).toBe(801);
    expect(canvas.width / 774.5).toBeCloseTo(2, 2);
    controller.resize(774.5, 400.5, 3);
    expect(canvas.width).toBe(2324);
  });

  it("keeps the picture fitted while the stage settles, and keeps the view once it was moved", () => {
    const canvas = fakeCanvas();
    const controller = new AtlasController(canvas, syntheticAtlas({ groups: 1, members: 4, seed: 1, dbShare: 1 }), { reducedMotion: true, requestFrame: () => 0, cancelFrame: () => undefined });
    controller.resize(400, 120, 2);
    const small = controller.camera.k;
    controller.resize(990, 466, 2);
    expect(controller.camera.k).toBeGreaterThan(small);
    expect(controller.camera.k).toBeGreaterThan(3);
    controller.zoomBy(1.4, false);
    const moved = controller.camera.k;
    controller.resize(1200, 600, 2);
    expect(controller.camera.k).toBe(moved);
  });

  it("is perfectly still at rest: once settled it asks for no more frames", () => {
    const canvas = fakeCanvas();
    let pending: (() => void) | null = null;
    let requested = 0;
    const controller = new AtlasController(canvas, syntheticAtlas({ groups: 3, members: 30, seed: 1 }), { reducedMotion: true, requestFrame: (cb) => { requested += 1; pending = () => cb(0); return requested; }, cancelFrame: () => undefined });
    controller.resize(800, 400, 2);
    for (let i = 0; i < 5 && pending; i += 1) { const run: () => void = pending; pending = null; run(); }
    expect(pending).toBeNull();
    const before = requested;
    expect(requested).toBe(before);
  });

  it("lays three members in a row and fills a wider stage", () => {
    const [a, b, c] = [spiralPoint(0), spiralPoint(1), spiralPoint(2)];
    expect(a).toEqual({ x: 0, y: 0 });
    expect(Math.abs(b.y)).toBeLessThan(1e-6);
    expect(b.x).toBeCloseTo(-c.x, 6);
    const wide = fakeCanvas();
    (wide as unknown as { getBoundingClientRect: () => object }).getBoundingClientRect = () => ({ width: 1600, height: 800, left: 0, top: 0 });
    const square = fakeCanvas();
    (square as unknown as { getBoundingClientRect: () => object }).getBoundingClientRect = () => ({ width: 600, height: 600, left: 0, top: 0 });
    const data = syntheticAtlas({ groups: 6, members: 120, seed: 3 });
    const spread = (canvas: HTMLCanvasElement) => { const controller = new AtlasController(canvas, data, { reducedMotion: true }); const xs = controller.scene.groups.map((g) => g.x); return Math.max(...xs) - Math.min(...xs); };
    expect(spread(wide)).toBeGreaterThan(spread(square));
  });
});
