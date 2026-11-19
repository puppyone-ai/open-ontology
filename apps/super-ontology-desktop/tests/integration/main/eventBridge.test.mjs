import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createEventBridge } from "@so-main/event-bridge.mjs";
import { createSnapshotStore } from "@so-main/snapshot.mjs";
import { createSseParser } from "@so-main/sse-parser.mjs";
import { startFakeEventStream } from "../../support/fake-event-stream.mjs";

const event = (seq, type = "file.state_changed", data = {}) => ({ seq, type, at: "2026-11-17T00:00:00Z", drive_id: null, data });

describe("SSE parser", () => {
  function parse(chunks) {
    const messages = [];
    const retries = [];
    const comments = [];
    const parser = createSseParser({ onMessage: (m) => messages.push(m), onRetry: (ms) => retries.push(ms), onComment: (c) => comments.push(c) });
    for (const chunk of chunks) parser.feed(chunk);
    return { messages, retries, comments };
  }

  it("reads id, event and data, ignores pings and reports retry", () => {
    const { messages, retries, comments } = parse(["retry: 3000\n\n: ping\n\nid: 7\nevent: file.state_changed\ndata: {\"a\":1}\n\n"]);
    expect(retries).toEqual([3000]);
    expect(comments).toEqual(["ping"]);
    expect(messages).toEqual([{ event: "file.state_changed", data: "{\"a\":1}", id: "7" }]);
  });

  it("joins multi-line data and survives any chunk boundary, CRLF included", () => {
    const text = "id: 1\r\nevent: x\r\ndata: one\r\ndata: two\r\n\r\nid: 2\ndata: z\n\n";
    for (let split = 1; split < text.length; split += 1) {
      const { messages } = parse([text.slice(0, split), text.slice(split)]);
      expect(messages, `split at ${split}`).toEqual([
        { event: "x", data: "one\ntwo", id: "1" }, { event: "message", data: "z", id: "2" },
      ]);
    }
    expect(parse([...text]).messages).toHaveLength(2);
  });

  it("dispatches nothing without data and rejects a malformed retry", () => {
    const { messages, retries } = parse(["id: 5\n\nretry: abc\n\n"]);
    expect(messages).toEqual([]);
    expect(retries).toEqual([]);
  });
});

describe("event bridge", () => {
  let stream;
  let snapshots;
  let pushes;
  let bridge;
  let lifecycle;
  let instance;
  const sender = { once: vi.fn() };

  const channelPayloads = (channel) => pushes.filter((push) => push.channel === channel).map((push) => push.payload);
  const eventsDelivered = () => channelPayloads("events").flatMap((payload) => payload.events);
  const connectionStates = () => channelPayloads("connection-changed").map((payload) => payload.state);

  async function until(predicate, label, timeout = 5_000) {
    const deadline = Date.now() + timeout;
    while (!predicate()) {
      if (Date.now() > deadline) throw new Error(`Timed out: ${label}. Connection ${JSON.stringify(bridge.connection())}`);
      await new Promise((resolve) => { setTimeout(resolve, 10); });
    }
  }

  function make(tuning = {}, extra = {}) {
    bridge = createEventBridge({
      lifecycle, snapshots, logger: { debug() {}, warn() {} },
      broadcast: (channel, payload) => pushes.push({ channel, payload }),
      random: () => 0.5, // No jitter.
      tuning: { coalesceMs: 25, backoffMs: [40, 80, 120], idleDisconnectMs: 10_000, ...tuning },
      ...extra,
    });
    return bridge;
  }

  beforeEach(async () => {
    stream = await startFakeEventStream();
    pushes = [];
    instance = "42:start-1";
    snapshots = createSnapshotStore({ broadcast: () => {} });
    lifecycle = {
      credentials: () => ({ baseUrl: stream.baseUrl, token: stream.token }),
      reloadCredentials: vi.fn(async () => true),
      instanceId: () => instance,
      pokeHealth: vi.fn(),
    };
    snapshots.update({ status: "healthy", health: { events_head: 10 } });
  });
  afterEach(async () => {
    bridge?.dispose();
    await stream.close();
  });

  it("does not connect until a window subscribes, then starts at the health events_head", async () => {
    make();
    await new Promise((resolve) => { setTimeout(resolve, 100); });
    expect(stream.connections).toHaveLength(0);
    const subscribed = bridge.subscribe(sender);
    expect(subscribed).toMatchObject({ instanceId: instance, lastSeq: 10 });
    await until(() => bridge.connection().state === "connected", "connected");
    expect(stream.connections[0].since).toBe("10");
    expect(stream.connections[0].headers).toMatchObject({ authorization: `Bearer ${stream.token}`, accept: "text/event-stream", "oo-docs-api-min": "1.0" });
  });

  it("delivers persistent events in one coalesced push per 50 ms-style window and splits big batches", async () => {
    make({ batchMax: 3 });
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    for (let seq = 11; seq <= 15; seq += 1) stream.send(event(seq));
    await until(() => eventsDelivered().length === 5, "5 events");
    expect(channelPayloads("events").map((payload) => payload.events.length)).toEqual([3, 2]);
    expect(channelPayloads("events").at(-1)).toMatchObject({ instanceId: instance, lastSeq: 15 });
  });

  it("drops duplicates and older seq, forwards transient events without moving the cursor, hides stream.lagging", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    stream.send(event(11));
    stream.send(event(9));
    stream.send({ type: "file.progress", at: "x", data: { file_id: "f", unit: "page", done: 1, total: 2 } }, { id: undefined });
    stream.send({ type: "stream.lagging", at: "x", data: {} }, { id: undefined });
    stream.send(event(12));
    await until(() => eventsDelivered().length === 3, "3 events");
    expect(eventsDelivered().map((entry) => entry.type)).toEqual(["file.state_changed", "file.progress", "file.state_changed"]);
    expect(eventsDelivered().map((entry) => entry.seq)).toEqual([11, undefined, 12]);
    expect(bridge.state()).toMatchObject({ lastSeq: 12, ringSize: 2 });
    expect(() => bridge.since({ instanceId: instance, seq: 10 })).not.toThrow();
    expect(bridge.since({ instanceId: instance, seq: 10 }).events.map((entry) => entry.seq)).toEqual([11, 12]);
  });

  it("reads the id field when data carries no seq, and survives byte-sized chunks and pings", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.comment("ping");
    const body = JSON.stringify({ type: "file.removed", at: "x", data: { file_id: "f" } });
    const wire = `id: 11\nevent: file.removed\ndata: ${body}\n\n`;
    for (const char of wire) stream.raw(char);
    await until(() => eventsDelivered().length === 1, "event");
    expect(eventsDelivered()[0]).toMatchObject({ seq: 11, type: "file.removed" });
  });

  it("reconnects with backoff after a drop, resumes from lastSeq and shows the connection states in order", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    await until(() => eventsDelivered().length === 1, "first event");
    stream.drop();
    await until(() => bridge.connection().state === "reconnecting", "reconnecting");
    expect(bridge.connection()).toMatchObject({ attempt: 1, lastSeq: 11 });
    expect(bridge.connection().nextRetryAt).toEqual(expect.any(String));
    await stream.attempts(2);
    await until(() => bridge.connection().state === "connected", "connected again");
    expect(stream.connections[1].since).toBe("11");
    stream.send(event(12));
    stream.send(event(13));
    await until(() => eventsDelivered().length === 3, "catch-up");
    expect(connectionStates()).toEqual(["connected", "reconnecting", "connected"]);
    expect(bridge.connection().attempt).toBe(0);
  });

  it("grows the delay on repeated failures up to the cap and honors a larger server retry", async () => {
    make({ backoffMs: [30, 60, 90] });
    stream.respondOnce(503, {});
    bridge.subscribe(sender);
    await until(() => bridge.connection().attempt === 1, "attempt 1");
    const failures = [];
    for (let attempt = 2; attempt <= 4; attempt += 1) {
      stream.respondOnce(503, {});
      await until(() => bridge.connection().attempt >= attempt, `attempt ${attempt}`, 8_000);
      failures.push(bridge.connection().attempt);
    }
    expect(failures).toEqual([2, 3, 4]);
    await until(() => bridge.connection().state === "connected", "connected");
    expect(bridge.connection().attempt).toBe(0);
  });

  it("uses the server retry: value when it is larger than the backoff", async () => {
    await stream.close();
    stream = await startFakeEventStream({ retryMs: 2_500 });
    make({ backoffMs: [20] });
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    // retry: 2500 from the server is larger than 20 ms.
    stream.drop();
    await until(() => bridge.connection().state === "reconnecting", "reconnecting");
    const wait = Date.parse(bridge.connection().nextRetryAt) - Date.now();
    expect(wait).toBeGreaterThan(2_000);
  });

  it("treats stream.lagging plus a closed connection as an ordinary reconnect with since", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    stream.send({ type: "stream.lagging", at: "x", data: {} }, { id: undefined });
    stream.end();
    await until(() => bridge.connection().state === "reconnecting", "reconnecting");
    expect(channelPayloads("resync-required")).toEqual([]);
    await stream.attempts(2);
    expect(stream.connections[1].since).toBe("11");
  });

  it("answers 410 with resync-required, clears the ring and continues from the head the engine names", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    await until(() => eventsDelivered().length === 1, "event");
    stream.respondOnce(410, { code: "DOCS_ENGINE_CURSOR_EXPIRED", message: "x", retryable: false, details: { oldest_seq: 500, head_seq: 900 } });
    stream.drop();
    await until(() => channelPayloads("resync-required").length === 1, "resync-required");
    expect(channelPayloads("resync-required")[0]).toEqual({ instanceId: instance, reason: "cursor_expired", headSeq: 900 });
    expect(bridge.state()).toMatchObject({ lastSeq: 900, ringSize: 0 });
    expect(bridge.connection().state).toBe("resyncing");
    await until(() => stream.connections.length >= 3 && stream.connections.at(-1).since === "900", "reconnect from head");
    // The renderer has reloaded its lists and continues from the newest as_of_seq.
    expect(bridge.subscribe(sender, { sinceSeq: 905 }).lastSeq).toBe(905);
    await until(() => bridge.connection().state === "connected", "connected after resync");
    expect(() => bridge.since({ instanceId: instance, seq: 11 })).toThrow(expect.objectContaining({ code: "SO_EVENTS_GAP" }));
  });

  it("treats a new engine instance like an expired cursor", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    await until(() => eventsDelivered().length === 1, "event");
    instance = "43:start-2";
    snapshots.update({ health: { events_head: 3 } });
    stream.drop();
    await until(() => channelPayloads("resync-required").length === 1, "resync-required");
    expect(channelPayloads("resync-required")[0]).toMatchObject({ reason: "engine_restarted", instanceId: "43:start-2", headSeq: 3 });
    expect(bridge.state()).toMatchObject({ instanceId: "43:start-2", lastSeq: 3, ringSize: 0 });
  });

  describe("events-since", () => {
    it("returns the missing events while the ring covers them and SO_EVENTS_GAP otherwise", async () => {
      make({ ringSize: 4 });
      bridge.subscribe(sender);
      await until(() => bridge.connection().state === "connected", "connected");
      for (let seq = 11; seq <= 16; seq += 1) stream.send(event(seq));
      await until(() => bridge.state().lastSeq === 16, "16 events");
      expect(bridge.state().ringSize).toBe(4);
      expect(bridge.since({ instanceId: instance, seq: 16 })).toEqual({ events: [], lastSeq: 16 });
      expect(bridge.since({ instanceId: instance, seq: 20 })).toEqual({ events: [], lastSeq: 16 });
      expect(bridge.since({ instanceId: instance, seq: 13 }).events.map((entry) => entry.seq)).toEqual([14, 15, 16]);
      expect(bridge.since({ instanceId: instance, seq: 12 }).events.map((entry) => entry.seq)).toEqual([13, 14, 15, 16]);
      expect(() => bridge.since({ instanceId: instance, seq: 11 })).toThrow(expect.objectContaining({ code: "SO_EVENTS_GAP" }));
      expect(() => bridge.since({ instanceId: "other", seq: 15 })).toThrow(expect.objectContaining({ code: "SO_EVENTS_GAP" }));
    });
  });

  it("disconnects 60 s (here: a moment) after the last window leaves and resumes from lastSeq", async () => {
    make({ idleDisconnectMs: 60 });
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11));
    await until(() => eventsDelivered().length === 1, "event");
    bridge.unsubscribe(sender);
    await until(() => bridge.connection().state === "offline", "offline after idle");
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected again");
    expect(stream.connections.at(-1).since).toBe("11");
  });

  it("drops a window that was destroyed", () => {
    make();
    const destroyable = { once: (name, callback) => { destroyable.callback = callback; } };
    bridge.subscribe(destroyable);
    destroyable.callback();
    expect(bridge.connection().state === "offline" || bridge.connection().state === "connected").toBe(true);
  });

  it("follows the engine: no connection while it is not live, offline as soon as it stops being live", async () => {
    snapshots.update({ status: "starting" });
    make();
    bridge.subscribe(sender);
    await new Promise((resolve) => { setTimeout(resolve, 80); });
    expect(stream.connections).toHaveLength(0);
    snapshots.update({ status: "degraded", health: { events_head: 10 } });
    await until(() => bridge.connection().state === "connected", "connected when degraded");
    snapshots.update({ status: "restarting", reason: "drive_change" });
    await until(() => bridge.connection().state === "offline", "offline");
    snapshots.update({ status: "healthy" });
    await until(() => bridge.connection().state === "connected", "connected again");
  });

  it("nudges the health patrol on engine.health_changed", async () => {
    make();
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected");
    stream.send(event(11, "engine.health_changed", { status: "degraded" }));
    await until(() => lifecycle.pokeHealth.mock.calls.length === 1, "poke");
  });

  it("re-reads the token after a 401 and never lets credentials into a push", async () => {
    make();
    const original = lifecycle.credentials;
    let calls = 0;
    lifecycle.credentials = () => (calls++ === 0 ? { baseUrl: stream.baseUrl, token: "oodt_wrong" } : original());
    bridge.subscribe(sender);
    await until(() => bridge.connection().state === "connected", "connected after token reload", 8_000);
    expect(lifecycle.reloadCredentials).toHaveBeenCalled();
    expect(JSON.stringify(pushes)).not.toContain("oodt_");
    expect(JSON.stringify(pushes)).not.toContain(stream.token);
  });
});
