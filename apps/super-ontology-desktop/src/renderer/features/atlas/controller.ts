import { centerOn, fitCamera, mixCamera, toWorld, zoomAt, type Camera } from "./camera";
import { drawAtlas, type DrawStats } from "./render";
import { AtlasScene, type SceneNode } from "./scene";
import { createSimHost, type SimHost } from "./simHost";
import { SimCore } from "./simulation";
import type { AtlasData, AtlasOp } from "./model";

/**
 * Wires one atlas together: the scene, the simulation (worker or inline), the camera, the pointer and keyboard, and the frame loop.
 * No React in here, so the whole thing can be driven from a test or a measuring script with a canvas and nothing else.
 */
export type ControllerOptions = {
  reducedMotion: boolean;
  worker?: "worker" | "inline";
  /** Fires when a cell is clicked: a group toggles by itself, a member asks the screen to open it. */
  onActivate?: (node: SceneNode) => void;
  onSelect?: (node: SceneNode | null) => void;
  onHover?: (node: SceneNode | null) => void;
  now?: () => number;
  requestFrame?: (callback: (time: number) => void) => number;
  cancelFrame?: (handle: number) => void;
};

export type FrameStats = { fps: number; frameMs: number; p95Ms: number; draw: DrawStats };

const CAMERA_TWEEN_MS = 260;

export class AtlasController {
  scene: AtlasScene;
  camera: Camera = { x: 0, y: 0, k: 1 };
  focus = -1;
  selected = -1;
  hits = new Set<number>();
  private lit: Set<number> | null = null;
  private host: SimHost | null = null;
  private ctx: CanvasRenderingContext2D;
  private width = 0;
  private height = 0;
  private dpr = 1;
  private raf: number | null = null;
  private last = 0;
  private dirty = true;
  private simRunning = false;
  private pushedStructure = -1;
  private tween: { from: Camera; to: Camera; start: number } | null = null;
  private drag: { id: number; x: number; y: number; moved: boolean } | null = null;
  private readonly frames: number[] = [];
  private lastDraw: DrawStats = { members: 0, labels: 0, links: 0 };
  private disposed = false;
  private fitted = false;

  constructor(private readonly canvas: HTMLCanvasElement, data: AtlasData, private readonly options: ControllerOptions) {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("Canvas 2D is not available");
    this.ctx = ctx;
    this.scene = new AtlasScene(data);
    this.layout(true);
    // The lab scripts (scripts/atlas-lab.mjs) drive the atlas through this; it does not exist in a production build.
    if (typeof window !== "undefined" && import.meta.env?.DEV) (window as unknown as { __atlas?: AtlasController }).__atlas = this;
  }

  get reduced(): boolean { return this.options.reducedMotion; }
  private now = () => (this.options.now ? this.options.now() : performance.now());
  private request = (callback: (time: number) => void) => (this.options.requestFrame ?? requestAnimationFrame)(callback);

  // ---- data -----------------------------------------------------------------------------------------------------

  /** Replaces the picture (a new mode, or a first load). */
  setData(data: AtlasData): void {
    this.scene = new AtlasScene(data);
    this.focus = -1; this.selected = -1; this.lit = null; this.hits = new Set();
    this.pushedStructure = -1;
    this.fitted = false;
    this.layout(true);
    this.fit(false);
  }

  apply(ops: readonly AtlasOp[]): void {
    this.scene.apply(ops);
    if (this.reduced) { this.scene.step(10); this.scene.finishGrowth(); }
    this.layout(false);
    this.kick();
  }

  /**
   * Lays the scene out. Reduced motion settles it here and now (no worker, no motion); otherwise one simulation host lives as long as
   * the scene does, takes every later change through `pushNodes`, and keeps the positions of the cells it already knows.
   */
  private layout(first: boolean): void {
    if (this.reduced) { this.settleNow(); return; }
    if (first || !this.host) {
      this.disposeHost();
      this.host = createSimHost((positions, settled) => {
        if (this.disposed) return;
        this.scene.setPositions(positions);
        this.simRunning = !settled;
        this.dirty = true;
        this.kick();
      }, this.options.worker ?? "worker");
      this.pushedStructure = -1;
      this.pushNodes("target", 0.9);
    }
  }

  private pushNodes(start: "bud" | "target", alpha: number): void {
    if (!this.host) return;
    this.host.setNodes(this.scene.simSpecs(), { start, reheat: alpha, size: this.scene.size });
    this.pushedStructure = this.scene.structure;
    this.simRunning = true;
  }

  /** Static layout for the snapshot and for tests: finish growth and settle the simulation in this thread. */
  settleNow(): void {
    this.scene.finishGrowth();
    const core = new SimCore();
    core.setNodes(this.scene.simSpecs(), "target");
    core.settle();
    const positions = new Float32Array(this.scene.size * 2).fill(Number.NaN);
    core.copyPositions(positions);
    this.scene.setPositions(positions);
    this.dirty = true;
  }

  // ---- sizing and camera ----------------------------------------------------------------------------------------

  resize(width: number, height: number, dpr: number): void {
    this.width = Math.max(1, Math.floor(width));
    this.height = Math.max(1, Math.floor(height));
    this.dpr = dpr || 1;
    this.canvas.width = Math.round(this.width * this.dpr);
    this.canvas.height = Math.round(this.height * this.dpr);
    if (!this.fitted) this.fit(false);
    this.dirty = true;
    this.kick();
  }

  fit(animated = true): void {
    const target = fitCamera(this.scene.bounds(), this.width, this.height);
    this.fitted = this.width > 0;
    this.moveCamera(target, animated);
  }

  /** Asks for a redraw after the camera was set directly (the measuring page). */
  invalidate(): void { this.dirty = true; this.kick(); }

  zoomBy(factor: number, animated = true): void {
    const target = zoomAt(this.camera, this.width / 2, this.height / 2, factor);
    this.moveCamera(target, animated);
  }

  panBy(dx: number, dy: number): void {
    this.camera = { ...this.camera, x: this.camera.x + dx, y: this.camera.y + dy };
    this.dirty = true;
    this.kick();
  }

  focusOn(node: SceneNode, zoom = 1.4): void {
    this.moveCamera(centerOn(node.x, node.y, Math.max(this.camera.k, zoom), this.width, this.height), true);
  }

  private moveCamera(target: Camera, animated: boolean): void {
    if (!animated || this.reduced) { this.camera = target; this.tween = null; }
    else this.tween = { from: this.camera, to: target, start: this.now() };
    this.dirty = true;
    this.kick();
  }

  // ---- selection ------------------------------------------------------------------------------------------------

  private litFor(node: SceneNode): Set<number> {
    const set = new Set<number>([node.idx, ...this.scene.neighbors(node.idx)]);
    if (node.kind === "group") for (const member of node.members) set.add(member);
    else set.add(node.group);
    return set;
  }

  setFocus(node: SceneNode | null): void {
    const idx = node ? node.idx : -1;
    if (idx === this.focus) return;
    this.focus = idx;
    this.lit = node ? this.litFor(node) : this.selected >= 0 ? this.litFor(this.scene.nodes[this.selected]!) : null;
    this.options.onHover?.(node);
    this.dirty = true;
    this.kick();
  }

  select(node: SceneNode | null): void {
    this.selected = node ? node.idx : -1;
    this.lit = node ? this.litFor(node) : null;
    this.options.onSelect?.(node);
    this.dirty = true;
    this.kick();
  }

  /** Marks the cells that match a search; returns them (the screen lists them and offers to jump). */
  search(query: string): SceneNode[] {
    const found = this.scene.search(query);
    this.hits = new Set(found.slice(0, 400).map((node) => node.idx));
    this.dirty = true;
    this.kick();
    return found;
  }

  /** Opens whatever holds `node` and brings it into view. */
  reveal(node: SceneNode): void {
    const owner = this.scene.owner(node);
    if (node.kind === "member" && !owner.expanded) this.scene.setExpanded(owner.id, true);
    this.layout(false);
    this.select(node);
    this.focusOn(node.kind === "member" ? owner : node, node.kind === "member" ? 1.2 : 1);
  }

  activate(node: SceneNode): void {
    if (node.kind === "group") {
      this.scene.toggle(node.id);
      if (this.reduced) { this.scene.step(10); }
      this.layout(false);
      this.select(node);
    } else {
      this.select(node);
      this.options.onActivate?.(node);
    }
  }

  playGrowth(): number {
    if (this.reduced) { this.scene.finishGrowth(); this.dirty = true; this.kick(); return 0; }
    const steps = this.scene.playGrowth();
    this.pushNodes("target", 0.5);
    this.fit(false);
    this.kick();
    return steps;
  }

  // ---- pointer and keyboard -------------------------------------------------------------------------------------

  private local(event: { clientX: number; clientY: number }) {
    const box = this.canvas.getBoundingClientRect();
    return { x: event.clientX - box.left, y: event.clientY - box.top };
  }

  private hitAt(x: number, y: number): SceneNode | null {
    const w = toWorld(this.camera, x, y);
    return this.scene.hit(w.x, w.y, 3 / this.camera.k);
  }

  pointerDown = (event: PointerEvent): void => {
    if (event.button !== 0) return;
    this.canvas.setPointerCapture?.(event.pointerId);
    this.drag = { id: event.pointerId, x: event.clientX, y: event.clientY, moved: false };
  };

  pointerMove = (event: PointerEvent): void => {
    if (this.drag && this.drag.id === event.pointerId) {
      const dx = event.clientX - this.drag.x, dy = event.clientY - this.drag.y;
      if (!this.drag.moved && Math.hypot(dx, dy) < 4) return;
      this.drag.moved = true;
      this.drag.x = event.clientX; this.drag.y = event.clientY;
      this.tween = null;
      this.panBy(dx, dy);
      return;
    }
    const p = this.local(event);
    this.setFocus(this.hitAt(p.x, p.y));
  };

  pointerUp = (event: PointerEvent): void => {
    const drag = this.drag;
    this.drag = null;
    this.canvas.releasePointerCapture?.(event.pointerId);
    if (!drag || drag.moved) return;
    const p = this.local(event);
    const node = this.hitAt(p.x, p.y);
    if (node) this.activate(node); else this.select(null);
  };

  pointerLeave = (): void => { if (!this.drag) this.setFocus(null); };

  wheel = (event: WheelEvent): void => {
    event.preventDefault();
    const p = this.local(event);
    // A pinch arrives as a wheel with ctrlKey and small deltas; a mouse wheel as larger ones. Both feel the same this way.
    const factor = Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.0016));
    this.tween = null;
    this.camera = zoomAt(this.camera, p.x, p.y, factor);
    this.dirty = true;
    this.kick();
  };

  key = (event: KeyboardEvent): boolean => {
    const step = 64;
    switch (event.key) {
      case "ArrowLeft": this.panBy(step, 0); return true;
      case "ArrowRight": this.panBy(-step, 0); return true;
      case "ArrowUp": this.panBy(0, step); return true;
      case "ArrowDown": this.panBy(0, -step); return true;
      case "+": case "=": this.zoomBy(1.3); return true;
      case "-": case "_": this.zoomBy(1 / 1.3); return true;
      case "0": this.fit(); return true;
      case "Escape": this.select(null); this.setFocus(null); return true;
      default: return false;
    }
  };

  // ---- loop -----------------------------------------------------------------------------------------------------

  start(): void { this.disposed = false; this.kick(); }

  /** Schedules a frame if none is pending. The loop keeps itself going while anything moves and goes quiet otherwise. */
  private kick(): void {
    if (this.raf !== null || this.disposed) return;
    this.last = this.now();
    this.raf = this.request((time) => this.frame(time));
  }

  private frame(_time: number): void {
    this.raf = null;
    if (this.disposed) return;
    const now = this.now();
    const dt = Math.min(0.1, Math.max(0, (now - this.last) / 1000));
    this.last = now;
    let moving = false;
    if (this.reduced) { this.scene.step(10); }
    else moving = this.scene.step(dt);
    if (this.tween) {
      const t = Math.min(1, (now - this.tween.start) / CAMERA_TWEEN_MS);
      this.camera = mixCamera(this.tween.from, this.tween.to, t);
      if (t >= 1) this.tween = null; else moving = true;
    }
    if (this.host && this.scene.structure !== this.pushedStructure) this.pushNodes("target", 0.5);
    if (this.host) for (const group of this.scene.groups) if (Math.abs(group.r - group.rBase) > 0.02 || group.r !== group.rBase) this.host.setRadius(group.idx, group.r);
    if (moving || this.simRunning || this.dirty) {
      const started = this.now();
      this.lastDraw = drawAtlas(this.ctx, this.scene, { camera: this.camera, width: this.width, height: this.height, dpr: this.dpr, focus: this.focus, selected: this.selected, lit: this.lit, hits: this.hits });
      this.frames.push(this.now() - started);
      if (this.frames.length > 240) this.frames.shift();
      this.dirty = false;
    }
    if (moving || this.simRunning || this.scene.growing) this.kick();
  }

  /** Draws one frame now, bypassing the loop (snapshots and measurements). */
  drawNow(): DrawStats {
    this.lastDraw = drawAtlas(this.ctx, this.scene, { camera: this.camera, width: this.width, height: this.height, dpr: this.dpr, focus: this.focus, selected: this.selected, lit: this.lit, hits: this.hits });
    return this.lastDraw;
  }

  /** Draw cost over the last frames; the interval between frames comes from the browser, so `fps` is measured by the caller. */
  stats(): FrameStats {
    const sorted = [...this.frames].sort((a, b) => a - b);
    const mean = sorted.length ? sorted.reduce((s, v) => s + v, 0) / sorted.length : 0;
    return { fps: mean > 0 ? Math.min(60, 1000 / mean) : 60, frameMs: mean, p95Ms: sorted.length ? sorted[Math.floor(sorted.length * 0.95)]! : 0, draw: this.lastDraw };
  }

  private disposeHost(): void { this.host?.dispose(); this.host = null; }

  dispose(): void {
    this.disposed = true;
    if (this.raf !== null) (this.options.cancelFrame ?? cancelAnimationFrame)(this.raf);
    this.raf = null;
    this.disposeHost();
  }
}
