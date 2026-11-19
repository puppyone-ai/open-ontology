import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";
import { createSseParser } from "./sse-parser.mjs";

export const BRIDGE_DEFAULTS = Object.freeze({
  ringSize: 5_000,
  coalesceMs: 50,
  batchMax: 1_000,
  idleDisconnectMs: 60_000,
  backoffMs: Object.freeze([1_000, 2_000, 5_000, 10_000, 30_000]),
  jitter: 0.2,
});

const LIVE_STATUSES = new Set(["healthy", "degraded"]);

/**
 * SSE -> IPC event bridge (20-frontend §1.4.3): one connection per engine
 * instance, `since` cursor, de-duplication by `seq`, a ring buffer for
 * `events-since`, 50 ms coalescing, and connection-state pushes. The control
 * token never leaves `lifecycle.credentials()`.
 *
 * @param {object} options
 * @param {{ credentials: Function, reloadCredentials: Function, instanceId: Function, pokeHealth?: Function }} options.lifecycle
 * @param {{ peek: Function, update: Function, subscribe: Function }} options.snapshots
 * @param {(channel: string, payload: unknown) => void} options.broadcast
 */
export function createEventBridge({
  lifecycle,
  snapshots,
  broadcast,
  fetchImpl = globalThis.fetch,
  logger = console,
  timers = { setTimeout, clearTimeout },
  random = Math.random,
  tuning = {},
} = {}) {
  const t = { ...BRIDGE_DEFAULTS, ...tuning };
  const subscribers = new Set();
  const watched = new WeakSet();
  let ring = [];
  let lastSeq = 0;
  let instanceId = null;
  let connection = { state: "offline", attempt: 0, nextRetryAt: null, lastSeq: 0 };
  let controller = null;
  let reconnectTimer = null;
  let flushTimer = null;
  let idleTimer = null;
  let pending = [];
  let serverRetryMs = 0;
  let generation = 0;
  let needResync = null;
  let disposed = false;

  const liveNow = () => LIVE_STATUSES.has(snapshots.peek().status);

  function setConnection(patch) {
    const next = { ...connection, ...patch, lastSeq };
    const changed = ["state", "attempt", "nextRetryAt"].some((key) => next[key] !== connection[key]);
    connection = next;
    if (!changed) return;
    snapshots.update({ connection: { ...connection } });
    broadcast("connection-changed", { ...connection });
  }

  // ---- delivery -----------------------------------------------------------

  function flush() {
    flushTimer = null;
    while (pending.length > 0) {
      const events = pending.splice(0, t.batchMax);
      broadcast("events", { instanceId, events, lastSeq });
    }
  }

  function enqueue(event) {
    pending.push(event);
    flushTimer ??= timers.setTimeout(flush, t.coalesceMs);
  }

  function handleEvent(event) {
    if (event.type === "stream.lagging") return; // The server closes the stream next; we reconnect with `since`.
    const persistent = Number.isSafeInteger(event.seq);
    if (!persistent) {
      // Transient (file.progress): forwarded, never buffered, never advances the cursor.
      enqueue(event);
      return;
    }
    if (event.seq <= lastSeq) return; // Duplicate or replay overlap.
    lastSeq = event.seq;
    ring.push(event);
    if (ring.length > t.ringSize) ring = ring.slice(ring.length - t.ringSize);
    if (event.type === "engine.health_changed") lifecycle.pokeHealth?.();
    enqueue(event);
  }

  function onMessage({ event: name, data, id }) {
    let event;
    try { event = JSON.parse(data); } catch { return; }
    if (!event || typeof event !== "object" || typeof (event.type ?? name) !== "string") return;
    const type = event.type ?? name;
    const seq = id !== undefined && /^\d+$/u.test(id) ? Number(id) : event.seq;
    handleEvent({ ...event, type, ...(Number.isSafeInteger(seq) ? { seq } : {}) });
  }

  // ---- connection -----------------------------------------------------------

  function clearTimersOnly() {
    if (reconnectTimer) timers.clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function disconnect(state = "offline") {
    generation += 1;
    clearTimersOnly();
    controller?.abort();
    controller = null;
    setConnection({ state, attempt: 0, nextRetryAt: null });
  }

  function resync(reason, headSeq) {
    ring = [];
    pending = [];
    lastSeq = Number.isSafeInteger(headSeq) ? headSeq : 0;
    needResync = reason;
    setConnection({ state: "resyncing", attempt: 0, nextRetryAt: null });
    broadcast("resync-required", { instanceId, reason, ...(Number.isSafeInteger(headSeq) ? { headSeq } : {}) });
  }

  function scheduleReconnect(gen) {
    if (disposed || gen !== generation || subscribers.size === 0 || !liveNow()) {
      if (gen === generation) setConnection({ state: "offline", attempt: 0, nextRetryAt: null });
      return;
    }
    const attempt = connection.attempt + 1;
    const base = t.backoffMs[Math.min(attempt - 1, t.backoffMs.length - 1)];
    const jittered = Math.round(base * (1 + (random() * 2 - 1) * t.jitter));
    const delay = Math.max(jittered, serverRetryMs);
    setConnection({ state: needResync ? "resyncing" : "reconnecting", attempt, nextRetryAt: new Date(Date.now() + delay).toISOString() });
    reconnectTimer = timers.setTimeout(() => { reconnectTimer = null; void connect(gen); }, delay);
    reconnectTimer.unref?.();
  }

  async function readStream(response, gen) {
    const parser = createSseParser({ onMessage, onRetry: (ms) => { serverRetryMs = ms; } });
    const decoder = new TextDecoder();
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done || gen !== generation) return;
      parser.feed(decoder.decode(value, { stream: true }));
    }
  }

  function endConnection(gen) {
    if (gen === generation) controller = null;
  }

  async function connect(gen) {
    if (disposed || gen !== generation) return;
    const live = lifecycle.credentials();
    if (!live || !liveNow() || subscribers.size === 0) { scheduleReconnect(gen); return; }
    const currentInstance = lifecycle.instanceId();
    if (instanceId !== null && currentInstance !== instanceId) {
      instanceId = currentInstance;
      resync("engine_restarted", snapshots.peek().health?.events_head);
    }
    instanceId = currentInstance;
    const cursor = lastSeq > 0 ? lastSeq : (Number(snapshots.peek().health?.events_head) || 0);
    if (lastSeq === 0) lastSeq = cursor;
    controller = new AbortController();
    const { signal } = controller;
    let response;
    try {
      response = await fetchImpl(`${live.baseUrl}/v1/docs/events?since=${cursor}`, {
        headers: { Authorization: `Bearer ${live.token}`, Accept: "text/event-stream", "OO-Docs-Api-Min": "1.0" },
        signal, redirect: "error",
      });
    } catch {
      endConnection(gen);
      if (gen === generation) scheduleReconnect(gen);
      return;
    }
    if (gen !== generation) return;
    if (response.status === 410) {
      endConnection(gen);
      let headSeq;
      try { headSeq = (await response.json())?.details?.head_seq; } catch { /* Use 0. */ }
      logger.debug?.("Event cursor expired.");
      resync("cursor_expired", headSeq);
      scheduleReconnect(gen);
      return;
    }
    if (response.status === 401) {
      endConnection(gen);
      await response.arrayBuffer().catch(() => {});
      await lifecycle.reloadCredentials?.();
      scheduleReconnect(gen);
      return;
    }
    if (!response.ok || !response.body) {
      endConnection(gen);
      await response.arrayBuffer().catch(() => {});
      scheduleReconnect(gen);
      return;
    }
    setConnection({ state: needResync ? "resyncing" : "connected", attempt: 0, nextRetryAt: null });
    if (!needResync) needResync = null;
    try {
      await readStream(response, gen);
    } catch {
      /* A broken stream is an ordinary disconnect. */
    }
    endConnection(gen);
    if (gen === generation) scheduleReconnect(gen);
  }

  function ensureConnected() {
    if (disposed || subscribers.size === 0 || !liveNow() || controller || reconnectTimer) return;
    generation += 1;
    void connect(generation);
  }

  function armIdleTimer() {
    if (idleTimer) timers.clearTimeout(idleTimer);
    idleTimer = null;
    if (subscribers.size > 0 || connection.state === "offline") return;
    idleTimer = timers.setTimeout(() => {
      idleTimer = null;
      if (subscribers.size === 0) disconnect("offline");
    }, t.idleDisconnectMs);
    idleTimer.unref?.();
  }

  let observedStatus = snapshots.peek().status;
  const stopWatching = snapshots.subscribe((snapshot) => {
    // Our own connection updates also pass through here: react to engine status changes only.
    if (snapshot.status === observedStatus) return;
    observedStatus = snapshot.status;
    if (LIVE_STATUSES.has(snapshot.status)) ensureConnected();
    else if (connection.state !== "offline") disconnect("offline");
  });

  return Object.freeze({
    /** `events-subscribe`: registers the window and connects when the engine is live. */
    subscribe(sender, { sinceSeq } = {}) {
      const hadSubscribers = subscribers.size > 0;
      if (sender && typeof sender === "object") {
        // A window re-subscribes whenever it reloads its state: watch each window for destruction once.
        if (!watched.has(sender)) {
          watched.add(sender);
          sender.once?.("destroyed", () => { subscribers.delete(sender); armIdleTimer(); });
        }
        subscribers.add(sender);
      }
      if (idleTimer) timers.clearTimeout(idleTimer);
      idleTimer = null;
      if (Number.isSafeInteger(sinceSeq) && needResync) {
        // The renderer reloaded its lists: continue from their newest `as_of_seq`.
        lastSeq = sinceSeq;
        needResync = null;
        if (connection.state === "resyncing") setConnection({ state: controller ? "connected" : "reconnecting" });
      }
      if (!hadSubscribers && needResync && !Number.isSafeInteger(sinceSeq)) {
        // A new window loads everything itself: nothing is left to resynchronize.
        needResync = null;
        if (connection.state === "resyncing") setConnection({ state: controller ? "connected" : "reconnecting" });
      }
      instanceId ??= lifecycle.instanceId();
      ensureConnected();
      return { instanceId, lastSeq, connection: { ...connection } };
    },
    unsubscribe(sender) {
      subscribers.delete(sender);
      armIdleTimer();
      return { unsubscribed: true };
    },
    /** `events-since`: events after `seq` from the ring, or SO_EVENTS_GAP when it cannot be complete. */
    since({ instanceId: requested, seq }) {
      if (requested !== instanceId) throw new SuperOntologyError("SO_EVENTS_GAP");
      if (seq >= lastSeq) return { events: [], lastSeq };
      const oldest = ring[0]?.seq;
      if (oldest === undefined || oldest > seq + 1) {
        // A gap is real only when the ring starts later than the next expected seq. Events dropped by
        // `?types=` filtering would also skip numbers; we never filter, so every skip is a gap.
        throw new SuperOntologyError("SO_EVENTS_GAP");
      }
      return { events: ring.filter((event) => event.seq > seq), lastSeq };
    },
    connection: () => ({ ...connection }),
    state: () => ({ instanceId, lastSeq, ringSize: ring.length }),
    dispose() {
      disposed = true;
      stopWatching();
      disconnect("offline");
      if (flushTimer) timers.clearTimeout(flushTimer);
      if (idleTimer) timers.clearTimeout(idleTimer);
    },
  });
}
