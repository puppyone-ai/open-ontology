import type { SimRequest, SimResponse } from "./sim.worker";
import { SimCore, type SimNodeSpec } from "./simulation";

/**
 * Where the simulation runs. A Web Worker when the platform has one (the layout never blocks a frame), the same SimCore on a timer
 * otherwise (tests, very old shells). Both post a Float32Array of positions, NaN for a cell they do not know yet.
 */
export type SimHost = {
  setNodes(specs: readonly SimNodeSpec[], options: { start: "bud" | "target"; reheat: number; size: number }): void;
  setRadius(idx: number, r: number): void;
  reheat(alpha: number): void;
  dispose(): void;
  readonly mode: "worker" | "inline";
};
export type TickHandler = (positions: Float32Array, settled: boolean) => void;

export function createSimHost(onTick: TickHandler, prefer: "worker" | "inline" = "worker"): SimHost {
  if (prefer === "worker" && typeof Worker !== "undefined") {
    try { return createWorkerHost(onTick); } catch { /* a shell that forbids workers: run inline */ }
  }
  return createInlineHost(onTick);
}

function createWorkerHost(onTick: TickHandler): SimHost {
  const worker = new Worker(new URL("./sim.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (event: MessageEvent<SimResponse>) => onTick(event.data.positions, event.data.settled);
  const send = (message: SimRequest) => worker.postMessage(message);
  return {
    mode: "worker",
    setNodes: (specs, { start, reheat, size }) => send({ type: "nodes", specs: [...specs], start, reheat, size }),
    setRadius: (idx, r) => send({ type: "radius", idx, r }),
    reheat: (alpha) => send({ type: "reheat", alpha }),
    dispose: () => worker.terminate(),
  };
}

export function createInlineHost(onTick: TickHandler): SimHost {
  const core = new SimCore();
  let size = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const post = (settled: boolean) => {
    const positions = new Float32Array(size * 2).fill(Number.NaN);
    core.copyPositions(positions);
    onTick(positions, settled);
  };
  const loop = () => {
    timer = null;
    const settled = core.tick();
    post(settled);
    if (!settled) timer = setTimeout(loop, 16);
  };
  const wake = () => { if (timer === null) timer = setTimeout(loop, 0); };
  return {
    mode: "inline",
    setNodes: (specs, options) => { size = options.size; core.setNodes(specs, options.start); core.reheat(options.reheat); wake(); },
    setRadius: (idx, r) => core.setRadius(idx, r),
    reheat: (alpha) => { core.reheat(alpha); wake(); },
    dispose: () => { if (timer !== null) clearTimeout(timer); timer = null; },
  };
}
