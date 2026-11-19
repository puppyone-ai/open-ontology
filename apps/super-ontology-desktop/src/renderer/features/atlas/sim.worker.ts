/// <reference lib="webworker" />
import { SimCore, type SimNodeSpec } from "./simulation";

/**
 * Runs the layout off the main thread: the page posts the node set, the worker ticks at about 60 Hz and posts a transferable
 * Float32Array of positions after every step until the layout settles. The same SimCore runs inline when no Worker exists.
 */
export type SimRequest =
  | { type: "nodes"; specs: SimNodeSpec[]; start: "bud" | "target"; reheat: number; size: number }
  | { type: "radius"; idx: number; r: number }
  | { type: "reheat"; alpha: number }
  | { type: "settle"; size: number };
export type SimResponse = { type: "tick"; positions: Float32Array; settled: boolean };

const core = new SimCore();
let size = 0;
let timer: ReturnType<typeof setTimeout> | null = null;

const post = (settled: boolean) => {
  const positions = new Float32Array(size * 2).fill(Number.NaN);
  core.copyPositions(positions);
  (self as unknown as Worker).postMessage({ type: "tick", positions, settled } satisfies SimResponse, [positions.buffer]);
};

function loop() {
  timer = null;
  const started = performance.now();
  // A frame budget: never hold the worker for more than one display frame, so a message (new nodes) is taken promptly.
  let settled = core.settled;
  do { settled = core.tick(); } while (!settled && performance.now() - started < 12);
  post(settled);
  if (!settled) timer = setTimeout(loop, Math.max(0, 16 - (performance.now() - started)));
}
const wake = () => { if (timer === null) timer = setTimeout(loop, 0); };

self.onmessage = (event: MessageEvent<SimRequest>) => {
  const message = event.data;
  if (message.type === "nodes") { size = message.size; core.setNodes(message.specs, message.start); core.reheat(message.reheat); wake(); }
  else if (message.type === "radius") core.setRadius(message.idx, message.r);
  else if (message.type === "reheat") { core.reheat(message.alpha); wake(); }
  else if (message.type === "settle") { size = message.size; core.settle(); post(true); }
};
