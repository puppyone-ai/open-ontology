import { collapsedRadius, discBounds, expandedRadius, GOLDEN_ANGLE, memberRadius, phyllotaxisAnchors, planColumns, spiralPoint } from "./layout";
import type { SimNodeSpec } from "./simulation";
import { convexHull, foldedCorners, type Pt } from "./hull";
import type { AtlasData, AtlasGroup, AtlasGroupKind, AtlasSourceKind, AtlasLink, AtlasLinkKind, AtlasMember, AtlasOp, AtlasOrigin, AtlasRef, AtlasTone } from "./model";

/**
 * The state of the atlas: cells, their sizes, how far each has grown, pulsed or faded, and the links between them. A pure
 * object stepped by `step(dt)`, so growth, pulses and fades are tested without a canvas and a clock. The renderer only reads it.
 */
export type SceneNode = {
  idx: number;
  id: string;
  kind: "group" | "member";
  /** `idx` of the group (the node's own `idx` for a group). */
  group: number;
  groupKind: AtlasGroupKind;
  /** Groups only: the kind of source, for the glyph next to the label. */
  source: AtlasSourceKind | null;
  tone: AtlasTone;
  /** Document, database, or both (a document entity tied to a table row). Drawn as a dot, a square, a dot in a ring. */
  origin: AtlasOrigin;
  label: string;
  ref: AtlasRef | null;
  weight: number;
  born: number | null;
  x: number;
  y: number;
  /** Radius the cell would have at rest, and the one it has now (a group opening or closing eases between them). */
  rBase: number;
  r: number;
  /** 0 = not there yet, 1 = fully grown. */
  appear: number;
  /** Seconds to wait before it starts to grow (stagger). */
  delay: number;
  /** Should the cell be on screen? Members of a collapsed group, and cells the growth replay has not reached, are not. */
  shown: boolean;
  /** 1 right after a change, decaying to 0. */
  pulse: number;
  /** 0 while alive; rises to 1 as the cell shrinks and fades; at 1 it is gone. */
  retract: number;
  retracting: boolean;
  alive: boolean;
  phase: number;
  ox: number;
  oy: number;
  /** Groups only: where the layout wants the cell (the simulation pulls it there). */
  ax: number;
  ay: number;
  /** Groups only. */
  expanded: boolean;
  spiralNext: number;
  /** A short group laid out in columns by link direction: the radius it needs (otherwise null, and the lattice sets it). */
  fixedR: number | null;
  members: number[];
  /** Members that are alive (not retracted). */
  count: number;
};

/** `from` is the `idx` of the source end of a directed link, -1 for an undirected one; `t` is how much of it is drawn (0 to 1, source to target). */
export type SceneLink = { a: number; b: number; kind: AtlasLinkKind; weight: number; from: number; t: number };
export type GroupLink = { a: number; b: number; weight: number; count: number };
export type SceneOptions = {
  /** Open every group at once. Default: open them all when there are at most this many members. */
  expandAllUpTo?: number;
  /** Width over height of the stage (1 to 2): the groups are laid out wider than tall so a wide stage is filled. */
  aspect?: number;
};

/** The motion of 25 section 7.3: a node scales in over 200 ms, a link is drawn over 200 ms, a change rings for 400 ms, new ones stagger by 16 ms. */
export const APPEAR_SECONDS = 0.2;
export const LINK_SECONDS = 0.2;
export const RETRACT_SECONDS = 0.2;
export const PULSE_SECONDS = 0.4;
export const STAGGER_SECONDS = 0.016;
const BREATHE_SECONDS = 0.4;
const RADIUS_RATE = 6;
const MAX_STAGGER = 0.9;
export const DEFAULT_EXPAND_ALL_UP_TO = 400;
/** cubic-bezier(0.2, 0, 0, 1), the one curve of the app, sampled once. */
const EASE_TABLE = (() => {
  const x = (t: number) => 3 * (1 - t) ** 2 * t * 0.2 + t ** 3;
  const y = (t: number) => 3 * (1 - t) * t * t * 1 + t ** 3;
  const table = new Float32Array(129);
  let t = 0;
  for (let i = 0; i <= 128; i += 1) { const target = i / 128; while (t < 1 && x(t) < target) t += 1 / 4096; table[i] = y(Math.min(1, t)); }
  return table;
})();
export const ease = (value: number): number => { if (value <= 0) return 0; if (value >= 1) return 1; const at = value * 128; const i = Math.floor(at); return EASE_TABLE[i]! + (EASE_TABLE[i + 1]! - EASE_TABLE[i]!) * (at - i); };
const hash = (text: string): number => { let h = 2166136261; for (let i = 0; i < text.length; i += 1) { h ^= text.charCodeAt(i); h = Math.imul(h, 16777619); } return (h >>> 0) / 4294967296; };

export class AtlasScene {
  readonly nodes: SceneNode[] = [];
  private readonly groupList: SceneNode[] = [];
  readonly links: SceneLink[] = [];
  readonly byId = new Map<string, number>();
  private readonly groupIdx = new Map<string, number>();
  private groupLinkMap = new Map<string, GroupLink>();
  private adjacency = new Map<number, Set<number>>();
  /** Bumped when the set of cells the simulation must know changes. */
  structure = 0;
  /** Bumped when a position changed. */
  positionsVersion = 0;
  clock = 0;
  breathe = 0;
  private growth: { steps: (() => void)[]; at: number; gap: number } | null = null;
  private stretch = 1;
  private plans = new Map<string, NonNullable<ReturnType<typeof planColumns>>>();
  private linksDrawing = 0;
  private bornInBatch = 0;
  private hulls = new Map<number, { key: string; points: Pt[] }>();
  private grid: { cell: number; cells: Map<number, number[]>; version: number } | null = null;
  private anchorScale = 60;

  constructor(data: AtlasData, options: SceneOptions = {}) {
    const limit = options.expandAllUpTo ?? DEFAULT_EXPAND_ALL_UP_TO;
    const open = data.members.length <= limit;
    const sizes = new Map<string, number>();
    for (const member of data.members) sizes.set(member.group, (sizes.get(member.group) ?? 0) + 1);
    // Short groups joined by directed links are laid out in columns by link direction (layout.ts), the rest on the lattice.
    const edges = data.links.filter((l) => l.from !== undefined).map((l) => [l.from!, l.from === l.a ? l.b : l.a] as const);
    const plans = new Map<string, NonNullable<ReturnType<typeof planColumns>>>();
    for (const group of data.groups) {
      const plan = planColumns(data.members.filter((m) => m.group === group.id).map((m) => m.id), edges);
      if (plan) plans.set(group.id, plan);
    }
    this.plans = plans;
    const radii = data.groups.map((group) => (open ? plans.get(group.id)?.radius ?? expandedRadius(sizes.get(group.id) ?? 0) : collapsedRadius(sizes.get(group.id) ?? 0)));
    const anchors = phyllotaxisAnchors(radii);
    this.stretch = Math.min(2, Math.max(1, options.aspect ?? 1));
    for (const anchor of anchors) anchor.x *= this.stretch;
    this.anchorScale = radii.length ? (radii.reduce((a, b) => a + b, 0) / radii.length) * 2.3 : 60;
    data.groups.forEach((group, index) => {
      const node = this.makeGroup(group, anchors[index]!.x, anchors[index]!.y, radii[index]!);
      node.expanded = open;
      node.fixedR = open ? plans.get(group.id)?.radius ?? null : null;
    });
    for (const member of data.members) this.addMember(member, false);
    for (const link of data.links) this.addLink(link);
    for (const node of this.nodes) { node.appear = node.shown ? 1 : 0; node.r = node.rBase; }
    for (const node of this.nodes) {
      if (node.kind !== "member") continue;
      node.x = this.nodes[node.group]!.x + node.ox;
      node.y = this.nodes[node.group]!.y + node.oy;
    }
    this.refreshRadii(true);
  }

  // ---- construction -------------------------------------------------------------------------------------------

  private makeGroup(group: AtlasGroup, x: number, y: number, r: number): SceneNode {
    const node: SceneNode = {
      idx: this.nodes.length, id: group.id, kind: "group", group: this.nodes.length, groupKind: group.kind, source: group.source ?? null, tone: group.tone, origin: group.kind === "db" ? "db" : "doc", label: group.label, ref: null,
      weight: 0, born: null, x, y, rBase: r, r, appear: 1, delay: 0, shown: true, pulse: 0, retract: 0, retracting: false, alive: true, phase: hash(group.id),
      ox: 0, oy: 0, ax: x, ay: y, expanded: false, spiralNext: 0, fixedR: null, members: [], count: 0,
    };
    this.nodes.push(node);
    this.groupList.push(node);
    this.byId.set(group.id, node.idx);
    this.groupIdx.set(group.id, node.idx);
    return node;
  }

  private addMember(member: AtlasMember, grow: boolean): SceneNode | null {
    const owner = this.groupIdx.get(member.group);
    if (owner === undefined || this.byId.has(member.id)) return null;
    const parent = this.nodes[owner]!;
    const planned = this.plans.get(parent.id)?.offsets.get(member.id);
    const spot = planned ?? spiralPoint(parent.spiralNext);
    if (!planned) parent.spiralNext += 1;
    // A member that arrives later in a group laid out in columns finds the lattice, so the group goes back to it.
    if (!planned && parent.fixedR !== null) parent.fixedR = null;
    const node: SceneNode = {
      idx: this.nodes.length, id: member.id, kind: "member", group: owner, groupKind: parent.groupKind, source: null, tone: parent.tone, origin: member.origin ?? (member.ref.kind === "table" ? "db" : "doc"), label: member.label, ref: member.ref,
      weight: member.weight, born: member.born, x: parent.x, y: parent.y, rBase: memberRadius(member.weight), r: memberRadius(member.weight),
      appear: grow ? 0 : parent.expanded ? 1 : 0, delay: 0, shown: parent.expanded, pulse: 0, retract: 0, retracting: false, alive: true, phase: hash(member.id),
      ox: spot.x, oy: spot.y, ax: 0, ay: 0, expanded: false, spiralNext: 0, fixedR: null, members: [], count: 0,
    };
    this.nodes.push(node);
    this.byId.set(member.id, node.idx);
    parent.members.push(node.idx);
    parent.count += 1;
    this.structure += 1;
    return node;
  }

  private addLink(link: AtlasLink, animate = false): boolean {
    const a = this.byId.get(link.a);
    const b = this.byId.get(link.b);
    if (a === undefined || b === undefined || a === b) return false;
    const from = link.from === undefined ? -1 : this.byId.get(link.from) ?? -1;
    this.links.push({ a, b, kind: link.kind, weight: link.weight, from: from === a || from === b ? from : -1, t: animate ? 0 : 1 });
    if (animate) this.linksDrawing += 1;
    // A document tied to a table row is one thing seen from both sides.
    if (link.kind === "link") for (const idx of [a, b]) this.nodes[idx]!.origin = "both";
    this.adjacent(a).add(b);
    this.adjacent(b).add(a);
    const ga = this.nodes[a]!.group;
    const gb = this.nodes[b]!.group;
    if (ga !== gb) {
      const [lo, hi] = ga < gb ? [ga, gb] : [gb, ga];
      const key = `${lo}:${hi}`;
      const existing = this.groupLinkMap.get(key);
      if (existing) { existing.weight += link.weight; existing.count += 1; } else this.groupLinkMap.set(key, { a: lo, b: hi, weight: link.weight, count: 1 });
    }
    return true;
  }

  private adjacent(idx: number): Set<number> {
    let set = this.adjacency.get(idx);
    if (!set) { set = new Set(); this.adjacency.set(idx, set); }
    return set;
  }

  // ---- queries --------------------------------------------------------------------------------------------------

  /** Number of node slots (the simulation indexes positions densely by `idx`). */
  get size(): number { return this.nodes.length; }
  get groups(): readonly SceneNode[] { return this.groupList; }
  get groupLinks(): readonly GroupLink[] { return [...this.groupLinkMap.values()]; }
  neighbors(idx: number): ReadonlySet<number> { return this.adjacency.get(idx) ?? new Set(); }
  get memberCount(): number { return this.nodes.filter((node) => node.kind === "member" && node.alive).length; }
  node(id: string): SceneNode | null { const idx = this.byId.get(id); return idx === undefined ? null : this.nodes[idx]!; }

  /** A cell is drawn when it has grown at all and is not gone. */
  isVisible(node: SceneNode): boolean { return node.alive && node.appear > 0.001; }

  /** Group of a node; where a member of a collapsed group is drawn. */
  owner(node: SceneNode): SceneNode { return this.nodes[node.group]!; }

  bounds() {
    // A group with nothing in it draws nothing, so it is not part of what the view is fitted to.
    const visible = this.nodes.filter((node) => (node.kind === "group" ? node.count > 0 || !node.expanded : node.shown && node.appear > 0.5) && node.alive);
    return discBounds(visible.map((node) => ({ x: node.x, y: node.y, r: node.rBase })));
  }

  /** Cells matching `query` (label, case-insensitive), groups and members; used by the search box. */
  search(query: string): SceneNode[] {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return [];
    return this.nodes.filter((node) => node.alive && node.label.toLocaleLowerCase().includes(needle));
  }

  // ---- interaction ----------------------------------------------------------------------------------------------

  /** Opens a group: its members bud out of it one after another. Closing shrinks them back. */
  setExpanded(groupId: string, expanded: boolean): void {
    const idx = this.groupIdx.get(groupId);
    if (idx === undefined) return;
    const group = this.nodes[idx]!;
    if (group.expanded === expanded) return;
    group.expanded = expanded;
    group.members.forEach((memberIdx, order) => {
      const member = this.nodes[memberIdx]!;
      if (!member.alive) return;
      member.shown = expanded;
      member.delay = expanded ? Math.min(MAX_STAGGER, order * STAGGER_SECONDS) : 0;
      if (expanded && member.appear === 0) { member.x = group.x + member.ox; member.y = group.y + member.oy; }
    });
    this.structure += 1;
    this.refreshRadii(false);
  }

  toggle(groupId: string): void {
    const idx = this.groupIdx.get(groupId);
    if (idx !== undefined) this.setExpanded(groupId, !this.nodes[idx]!.expanded);
  }

  setAllExpanded(expanded: boolean): void { for (const group of this.groups) this.setExpanded(group.id, expanded); }

  /** Where a node must be drawn: the cell itself, or its group while it is folded into it. */
  private refreshRadii(snap: boolean): void {
    for (const group of this.groups) {
      group.rBase = group.expanded ? group.fixedR ?? expandedRadius(group.count) : collapsedRadius(group.count);
      if (snap) group.r = group.rBase;
    }
  }

  // ---- events and data ------------------------------------------------------------------------------------------

  apply(ops: readonly AtlasOp[]): void {
    this.bornInBatch = 0;
    for (const op of ops) {
      if (op.type === "bud") this.bud(op.member, op.group);
      else if (op.type === "pulse") this.pulseNode(op.id);
      else if (op.type === "retract") this.retractNode(op.id);
      else if (op.type === "breathe") this.breathe = 1;
      else if (op.type === "link") this.addLink(op.link, true);
    }
  }

  private bud(member: AtlasMember, group?: AtlasGroup): void {
    if (group && !this.groupIdx.has(group.id)) {
      const rank = this.groupList.length;
      const radius = Math.sqrt(rank + 0.5) * this.anchorScale;
      const angle = rank * GOLDEN_ANGLE;
      const node = this.makeGroup(group, radius * Math.cos(angle) * this.stretch, radius * Math.sin(angle), collapsedRadius(0));
      node.appear = 0;
      node.expanded = true;
      this.structure += 1;
    }
    const known = this.byId.get(member.id);
    if (known !== undefined) {
      const node = this.nodes[known]!;
      if (!node.alive || node.retracting) {
        node.alive = true; node.retracting = false; node.retract = 0; node.shown = this.nodes[node.group]!.expanded;
        if (node.kind === "member") this.nodes[node.group]!.count += 1;
        this.structure += 1;
        this.refreshRadii(false);
      }
      return;
    }
    const added = this.addMember(member, true);
    if (!added) return;
    const owner = this.nodes[added.group]!;
    added.x = owner.x + added.ox; added.y = owner.y + added.oy;
    added.delay = Math.min(MAX_STAGGER, this.bornInBatch * STAGGER_SECONDS);
    this.bornInBatch += 1;
    this.refreshRadii(false);
    owner.pulse = Math.max(owner.pulse, 0.6);
    if (!owner.expanded) added.shown = false;
  }

  private pulseNode(id: string): void {
    const idx = this.byId.get(id);
    if (idx === undefined) return;
    const node = this.nodes[idx]!;
    node.pulse = 1;
    if (node.kind === "member" && !node.shown) this.nodes[node.group]!.pulse = 1;
  }

  private retractNode(id: string): void {
    const idx = this.byId.get(id);
    if (idx === undefined) return;
    const node = this.nodes[idx]!;
    if (!node.alive || node.retracting) return;
    node.retracting = true;
    if (node.kind === "group") for (const memberIdx of node.members) { const m = this.nodes[memberIdx]!; if (m.alive) m.retracting = true; }
    if (node.kind === "member") { const owner = this.nodes[node.group]!; owner.count = Math.max(0, owner.count - 1); this.refreshRadii(false); }
  }

  // ---- growth replay --------------------------------------------------------------------------------------------

  /**
   * "Play growth": everything folds away, then cells appear in order. With ontology versions known (`born`), one version at a time;
   * otherwise one group at a time, each opening into its members. Returns the number of steps.
   */
  playGrowth(stepSeconds = 0.9): number {
    const versions = [...new Set(this.nodes.filter((n) => n.kind === "member" && n.alive && n.born !== null).map((n) => n.born as number))].sort((a, b) => a - b);
    const bornOf = (member: SceneNode) => member.born ?? versions[0] ?? 0;
    for (const node of this.nodes) {
      if (!node.alive) continue;
      node.shown = false; node.appear = 0; node.delay = 0; node.pulse = 0;
      if (node.kind === "group") node.expanded = false;
    }
    // Links are drawn again as their two ends arrive.
    for (const link of this.links) link.t = 0;
    this.linksDrawing = this.links.length;
    /** Brings `fresh` members of `group` into view; the group appears with its first member and stays open. */
    const reveal = (group: SceneNode, fresh: SceneNode[], stagger: number) => {
      if (fresh.length === 0) return;
      group.shown = true;
      group.expanded = true;
      fresh.forEach((member, order) => { member.shown = true; member.delay = Math.min(MAX_STAGGER, order * stagger); member.x = group.x + member.ox; member.y = group.y + member.oy; });
      group.pulse = Math.max(group.pulse, 0.7);
      this.structure += 1;
      this.refreshRadii(false);
    };
    const members = (group: SceneNode) => group.members.map((i) => this.nodes[i]!).filter((m) => m.alive);
    const steps: (() => void)[] = [];
    if (versions.length > 0) {
      for (const version of versions) steps.push(() => { for (const group of this.groupList) reveal(group, members(group).filter((m) => bornOf(m) === version), STAGGER_SECONDS); });
    } else {
      for (const group of this.groupList) steps.push(() => { group.shown = true; group.expanded = true; reveal(group, members(group), STAGGER_SECONDS); });
    }
    this.structure += 1;
    this.growth = { steps, at: 0, gap: stepSeconds };
    this.growthClock = 0;
    return steps.length;
  }

  private growthClock = 0;
  get growing(): boolean { return this.growth !== null; }

  private endGrowth(): void {
    this.growth = null;
    // Groups the replay never reached (no member of that version) are still part of the picture.
    for (const group of this.groupList) if (group.alive && !group.shown) { group.shown = true; group.expanded = true; this.structure += 1; }
  }

  /** Jump to the finished picture (reduced motion, snapshots, or "skip"). */
  finishGrowth(): void {
    if (this.growth) { while (this.growth.at < this.growth.steps.length) this.growth.steps[this.growth.at++]!(); this.endGrowth(); }
    for (const node of this.nodes) { if (node.alive && node.shown) { node.appear = 1; node.delay = 0; } }
    for (const link of this.links) link.t = 1;
    this.linksDrawing = 0;
    this.refreshRadii(true);
  }

  // ---- time -----------------------------------------------------------------------------------------------------

  /** Advances every animation by `dt` seconds. Returns true while anything still moves (the loop may idle when false). */
  step(dt: number): boolean {
    this.clock += dt;
    let busy = false;
    if (this.growth) {
      this.growthClock += dt;
      while (this.growth && this.growth.at < this.growth.steps.length && this.growthClock >= this.growth.at * this.growth.gap) this.growth.steps[this.growth.at++]!();
      if (this.growth && this.growth.at >= this.growth.steps.length && this.growthClock >= this.growth.steps.length * this.growth.gap) this.endGrowth();
      busy = true;
    }
    for (const node of this.nodes) {
      if (!node.alive) continue;
      if (node.retracting) {
        node.retract = Math.min(1, node.retract + dt / RETRACT_SECONDS);
        busy = true;
        if (node.retract >= 1) { node.alive = false; node.shown = false; this.structure += 1; }
        continue;
      }
      const goal = node.shown ? 1 : 0;
      if (node.appear !== goal) {
        if (node.delay > 0) node.delay = Math.max(0, node.delay - dt);
        else node.appear = goal === 1 ? Math.min(1, node.appear + dt / APPEAR_SECONDS) : Math.max(0, node.appear - dt / (APPEAR_SECONDS * 0.6));
        busy = true;
        if (goal === 0 && node.appear === 0) this.structure += 1;
      }
      if (node.pulse > 0) { node.pulse = Math.max(0, node.pulse - dt / PULSE_SECONDS); busy = true; }
      if (node.kind === "group" && node.r !== node.rBase) {
        const gap = node.rBase - node.r;
        node.r = Math.abs(gap) < 0.05 ? node.rBase : node.r + gap * Math.min(1, dt * RADIUS_RATE);
        busy = true;
      }
    }
    if (this.linksDrawing > 0) {
      for (const link of this.links) {
        if (link.t >= 1) continue;
        const a = this.nodes[link.a]!, b = this.nodes[link.b]!;
        // A link is drawn once both ends are mostly there.
        if (!a.alive || !b.alive || a.appear < 0.6 || b.appear < 0.6) { busy = true; continue; }
        link.t = Math.min(1, link.t + dt / LINK_SECONDS);
        if (link.t >= 1) this.linksDrawing -= 1;
        busy = true;
      }
    }
    if (this.breathe > 0) { this.breathe = Math.max(0, this.breathe - dt / BREATHE_SECONDS); busy = true; }
    return busy;
  }

  /** Eased 0..1 growth of a node, for drawing. */
  grown(node: SceneNode): number { return ease(node.appear) * (1 - ease(node.retract)); }

  /**
   * The outline of a group, in the group's own coordinates: the convex hull of its members' resting places (those that have arrived), or the
   * corners of a square for a folded group. Cached until the set of arrived members changes.
   */
  hullOf(group: SceneNode): Pt[] {
    if (!group.expanded) return foldedCorners(group.r);
    let arrived = 0;
    for (const idx of group.members) { const m = this.nodes[idx]!; if (m.alive && m.appear > 0.001) arrived += 1; }
    const key = `${group.members.length}:${arrived}`;
    const cached = this.hulls.get(group.idx);
    if (cached && cached.key === key) return cached.points;
    const points = convexHull(group.members.map((idx) => this.nodes[idx]!).filter((m) => m.alive && m.appear > 0.001).map((m) => ({ x: m.ox, y: m.oy })));
    this.hulls.set(group.idx, { key, points });
    return points;
  }

  // ---- simulation wiring ----------------------------------------------------------------------------------------

  /** The cells the simulation must place right now: every group, and every member that is on its way in or fully there. */
  simSpecs(): SimNodeSpec[] {
    const specs: SimNodeSpec[] = [];
    for (const node of this.nodes) {
      if (!node.alive) continue;
      if (node.kind === "group") specs.push({ idx: node.idx, kind: "group", r: node.r, ax: node.ax, ay: node.ay });
      else if (node.shown || node.appear > 0) specs.push({ idx: node.idx, kind: "member", r: node.rBase, group: node.group, ox: node.ox, oy: node.oy });
    }
    return specs;
  }

  /** Takes positions posted by the simulation (`2 * idx`; NaN marks a cell the simulation does not know yet). */
  setPositions(positions: Float32Array): void {
    for (const node of this.nodes) {
      const at = node.idx * 2;
      if (at + 1 >= positions.length) break;
      if (!node.alive) continue;
      const x = positions[at]!;
      const y = positions[at + 1]!;
      if (Number.isNaN(x) || Number.isNaN(y)) continue;
      node.x = x; node.y = y;
    }
    this.positionsVersion += 1;
  }

  // ---- hit testing ----------------------------------------------------------------------------------------------

  /** The topmost cell under a world point: members before the group that holds them, smaller before larger. */
  hit(wx: number, wy: number, slack = 0): SceneNode | null {
    const grid = this.ensureGrid();
    const key = (cx: number, cy: number) => (cx + 32768) * 65536 + (cy + 32768);
    const cx = Math.floor(wx / grid.cell);
    const cy = Math.floor(wy / grid.cell);
    let best: SceneNode | null = null;
    let bestScore = Infinity;
    for (let dx = -1; dx <= 1; dx += 1) {
      for (let dy = -1; dy <= 1; dy += 1) {
        for (const idx of grid.cells.get(key(cx + dx, cy + dy)) ?? []) {
          const node = this.nodes[idx]!;
          const reach = node.r * this.grown(node) + slack;
          const d = Math.hypot(node.x - wx, node.y - wy);
          if (d > reach) continue;
          const score = (node.kind === "member" ? 0 : 1e6) + node.r + d * 0.001;
          if (score < bestScore) { best = node; bestScore = score; }
        }
      }
    }
    if (best) return best;
    // Large groups reach across many cells: test the groups directly (they are few).
    for (const group of this.groups) {
      if (!group.alive || group.appear < 0.2) continue;
      if (Math.hypot(group.x - wx, group.y - wy) <= group.r + slack) return group;
    }
    return null;
  }

  private ensureGrid() {
    if (this.grid && this.grid.version === this.positionsVersion + this.structure * 1e-6) return this.grid;
    const cell = 24;
    const cells = new Map<number, number[]>();
    for (const node of this.nodes) {
      if (!this.isVisible(node) || node.kind === "group") continue;
      const k = (Math.floor(node.x / cell) + 32768) * 65536 + (Math.floor(node.y / cell) + 32768);
      const bucket = cells.get(k);
      if (bucket) bucket.push(node.idx); else cells.set(k, [node.idx]);
    }
    this.grid = { cell, cells, version: this.positionsVersion + this.structure * 1e-6 };
    return this.grid;
  }
}
