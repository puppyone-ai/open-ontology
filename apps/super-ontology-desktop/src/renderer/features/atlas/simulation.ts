import { forceCollide, forceManyBody, forceSimulation, type Simulation, type SimulationNodeDatum } from "d3-force";

/**
 * The layout simulation (25 section 7.3): groups are pulled to their phyllotaxis anchors, push each other apart and collide;
 * members follow their group to a golden-angle offset and collide with each other. Two small simulations instead of one big one:
 * the charge force only ever sees the groups, so ten thousand members cost one collision pass and nothing else.
 *
 * It has no clock and no DOM, so the same class runs in a Web Worker (sim.worker.ts) and inline (tests, snapshot).
 */
export type SimNodeSpec =
  | { idx: number; kind: "group"; r: number; ax: number; ay: number }
  /** `group` is the `idx` of the group node; `ox` and `oy` is where the member rests relative to it. */
  | { idx: number; kind: "member"; r: number; group: number; ox: number; oy: number };

type Body = SimulationNodeDatum & { idx: number; r: number; ax: number; ay: number; ox: number; oy: number; group: number };

const PULL = 0.16;
const MEMBER_PAD = 0.8;
const GROUP_PAD = 54;

export class SimCore {
  private readonly bodies = new Map<number, Body>();
  private groups: Body[] = [];
  private members: Body[] = [];
  private readonly groupSim: Simulation<Body, undefined>;
  private readonly memberSim: Simulation<Body, undefined>;

  constructor() {
    const pullToAnchor = () => { for (const body of this.groups) { body.vx = (body.vx ?? 0) + (body.ax - (body.x ?? 0)) * PULL; body.vy = (body.vy ?? 0) + (body.ay - (body.y ?? 0)) * PULL; } };
    const followGroup = () => {
      for (const body of this.members) {
        const owner = this.bodies.get(body.group);
        if (!owner) continue;
        body.vx = (body.vx ?? 0) + (owner.x! + body.ox - (body.x ?? 0)) * PULL;
        body.vy = (body.vy ?? 0) + (owner.y! + body.oy - (body.y ?? 0)) * PULL;
      }
    };
    this.groupSim = forceSimulation<Body>([]).stop().alphaDecay(0.045).velocityDecay(0.5)
      .force("charge", forceManyBody<Body>().strength((body) => -(24 + body.r * 1.6)).distanceMax(900))
      .force("collide", forceCollide<Body>((body) => body.r + GROUP_PAD).iterations(2))
      .force("pull", pullToAnchor);
    this.memberSim = forceSimulation<Body>([]).stop().alphaDecay(0.045).velocityDecay(0.5)
      .force("collide", forceCollide<Body>((body) => body.r + MEMBER_PAD).iterations(1))
      .force("follow", followGroup);
  }

  /**
   * Replaces the node set. A node that was there keeps its position; a new group starts at its anchor and a new member at its
   * group (`start: "bud"`, it flows out to its place) or already at its place (`start: "target"`, the first layout).
   */
  setNodes(specs: readonly SimNodeSpec[], start: "bud" | "target" = "bud"): void {
    const next = new Map<number, Body>();
    const groups: Body[] = [];
    const members: Body[] = [];
    for (const spec of specs) {
      const known = this.bodies.get(spec.idx);
      if (spec.kind === "group") {
        const body: Body = known ?? { idx: spec.idx, r: spec.r, ax: spec.ax, ay: spec.ay, ox: 0, oy: 0, group: -1, x: spec.ax, y: spec.ay, vx: 0, vy: 0 };
        body.r = spec.r; body.ax = spec.ax; body.ay = spec.ay;
        next.set(spec.idx, body);
        groups.push(body);
      } else {
        const body: Body = known ?? { idx: spec.idx, r: spec.r, ax: 0, ay: 0, ox: spec.ox, oy: spec.oy, group: spec.group, vx: 0, vy: 0 };
        body.r = spec.r; body.ox = spec.ox; body.oy = spec.oy; body.group = spec.group;
        members.push(body);
        next.set(spec.idx, body);
      }
    }
    // Members get their start once every group is known.
    for (const body of members) {
      if (body.x === undefined) {
        const owner = next.get(body.group);
        const ownerX = owner?.x ?? 0;
        const ownerY = owner?.y ?? 0;
        body.x = start === "target" ? ownerX + body.ox : ownerX;
        body.y = start === "target" ? ownerY + body.oy : ownerY;
      }
    }
    this.bodies.clear();
    for (const [idx, body] of next) this.bodies.set(idx, body);
    this.groups = groups;
    this.members = members;
    this.groupSim.nodes(groups);
    this.memberSim.nodes(members);
  }

  /** Radius changes (a group opening, a member shrinking) without replacing the set. */
  setRadius(idx: number, r: number): void {
    const body = this.bodies.get(idx);
    if (body) body.r = r;
  }

  reheat(alpha = 0.7): void {
    this.groupSim.alpha(alpha);
    this.memberSim.alpha(alpha);
  }

  get alpha(): number { return Math.max(this.groupSim.alpha(), this.memberSim.alpha()); }
  get settled(): boolean { return this.alpha < this.groupSim.alphaMin(); }

  /** One step of both simulations; the groups move first so the members follow where they now are. */
  tick(): boolean {
    this.groupSim.tick();
    this.memberSim.tick();
    return this.settled;
  }

  /** Runs until the layout stops moving (bounded), for static output. Returns the number of steps. */
  settle(maxTicks = 600): number {
    this.reheat(1);
    let steps = 0;
    while (steps < maxTicks && !this.tick()) steps += 1;
    return steps;
  }

  /** Writes x,y of every node into `out` at `2 * idx` (idx is dense from 0). */
  copyPositions(out: Float32Array): void {
    for (const body of this.bodies.values()) {
      const at = body.idx * 2;
      if (at + 1 < out.length) { out[at] = body.x ?? 0; out[at + 1] = body.y ?? 0; }
    }
  }

  position(idx: number): { x: number; y: number } | null {
    const body = this.bodies.get(idx);
    return body ? { x: body.x ?? 0, y: body.y ?? 0 } : null;
  }
}
