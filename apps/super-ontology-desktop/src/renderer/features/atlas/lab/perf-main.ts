import { AtlasController } from "../controller";
import { syntheticAtlas } from "./fixtures";

/**
 * Measures the atlas on a synthetic corpus of `n` cells in the real runtime (`atlas-perf.html?n=1000`): how long the layout takes to
 * settle (the worker, off the main thread), and the frame cost while the picture idles with ambient motion, while it is panned and
 * zoomed continuously, and while a group opens. Result: `window.__perf` (JSON). Not part of a production build.
 */
declare global { interface Window { __perf?: unknown; __perfRun?: () => Promise<unknown> } }

const q = new URLSearchParams(window.location.search);
const n = Number(q.get("n") ?? 1000);
const groups = Number(q.get("groups") ?? Math.max(8, Math.round(Math.sqrt(n) / 2)));
const canvas = document.getElementById("c") as HTMLCanvasElement;
const dpr = window.devicePixelRatio || 1;

const frames = (ms: number, each?: (t: number) => void): Promise<number[]> => new Promise((resolve) => {
  const deltas: number[] = [];
  let last = performance.now();
  const start = last;
  const tick = (now: number) => {
    deltas.push(now - last);
    last = now;
    each?.(now - start);
    if (now - start < ms) requestAnimationFrame(tick); else resolve(deltas.slice(2));
  };
  requestAnimationFrame(tick);
});
const pct = (values: number[], p: number) => { const s = [...values].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(s.length * p))] ?? 0; };
const summarize = (deltas: number[]) => {
  const mean = deltas.reduce((a, b) => a + b, 0) / Math.max(1, deltas.length);
  return { frames: deltas.length, fps: Math.round((1000 / mean) * 10) / 10, p50Ms: Math.round(pct(deltas, 0.5) * 10) / 10, p95Ms: Math.round(pct(deltas, 0.95) * 10) / 10, p99Ms: Math.round(pct(deltas, 0.99) * 10) / 10, over20ms: deltas.filter((d) => d > 20).length };
};

window.__perfRun = async () => {
  const data = syntheticAtlas({ groups, members: n, linkRatio: 0.8, versions: 8, seed: 7, dbShare: 0.2 });
  const started = performance.now();
  const controller = new AtlasController(canvas, data, { reducedMotion: false, worker: "worker" });
  controller.resize(window.innerWidth, window.innerHeight, dpr);
  // Open every group so all n cells are on screen (the heaviest case).
  controller.scene.setAllExpanded(true);
  controller.scene.step(10);
  controller.start();
  controller.fit(false);
  const out: Record<string, unknown> = { cells: controller.scene.memberCount, groups: data.groups.length, links: data.links.length, dpr, viewport: [window.innerWidth, window.innerHeight], worker: true };
  // Settle: wait until the simulation stops moving (frames keep coming; the main thread only draws).
  const settleDeltas = await frames(Number(q.get("settle") ?? 6000));
  out.settleWindow = summarize(settleDeltas);
  out.setupMs = Math.round(performance.now() - started);
  controller.fit(false);
  await frames(300);
  out.idle = summarize(await frames(3000));
  // Pan and zoom continuously: a slow sweep out and in while dragging sideways.
  out.panZoom = summarize(await frames(4000, (t) => {
    const k = 0.6 + 0.5 * Math.sin(t / 700);
    controller.camera = { k: Math.max(0.05, k), x: window.innerWidth / 2 + Math.sin(t / 500) * 160, y: window.innerHeight / 2 + Math.cos(t / 600) * 90 };
    controller.invalidate();
  }));
  // Zoomed in on the middle, where labels and sprites are largest.
  controller.camera = { k: 2, x: window.innerWidth / 2, y: window.innerHeight / 2 };
  out.zoomedIn = summarize(await frames(2000));
  out.draw = controller.stats();
  window.__perf = out;
  controller.dispose();
  return out;
};
if (q.get("auto") !== "0") void window.__perfRun().then((result) => { document.title = `done ${JSON.stringify(result).length}`; });
