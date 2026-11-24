// formal: main=event_stream inv=inv steps=40 verify-steps=10 deep-steps=12 witnesses="witCaughtUp witGone410 witDuplicateDropped witTransientSeen witReconnectReplay" vinv="invNoStrandedClient invOutboxGapFree invViewMatchesCursor invCaughtUpEqualsServer invTransientHasNoSeq"
// FM4: event outbox and client catch-up (10 8.5, append_event in migration 0138,
// store/events.rs; 20 1.7 store). Durable events get seq 1..N under the
// event_head row lock (gap-free, monotone); transient events (file.progress)
// carry no seq and never touch durable state; a client that reconnects with
// `since` replays the retained tail (duplicates allowed, dropped by seq) or,
// when the cursor is older than the retention floor, gets 410 and refetches at
// headSeq. Not modelled: the D48 freshness calculation (renderer side, V-S2-02
// follow-up). Notes: formal/reviews/FM4.md.
module event_stream {
  pure val K = 2          // keys of the durable state (e.g. file ids)
  pure val MAXD = 5       // durable events
  pure val MAXT = 2       // transient events
  pure val MAXDISC = 2    // disconnects
  pure val MAXPURGE = 1   // retention purges (-> 410)
  pure val KEYS: Set[int] = 0.to(K - 1)

  type Ev = { key: int, value: int }

  var hist: int -> Ev      // ghost: every durable event ever appended, by seq
  var headSeq: int            // event_head.seq
  var floor: int           // events with seq <= floor are purged (retention)
  var prevHead: int
  var connected: bool
  var cursor: int          // client: last applied seq
  var view: int -> int     // client: durable view
  var transients: int      // client: transient events shown (never part of `view`)
  var tUsed: int
  var disc: int
  var purges: int
  var refetches: int
  var dupDropped: int
  var lastRes: str

  // server state after the first n events
  pure def valueAt(h: int -> Ev, k: int, n: int): int = {
    val last = h.keys().filter(s => s <= n and h.get(s).key == k).fold(0, (a, s) => if (s > a) s else a)
    if (last == 0) 0 else h.get(last).value
  }
  pure def stateAt(h: int -> Ev, n: int): int -> int = KEYS.mapBy(k => valueAt(h, k, n))

  action init = all {
    hist' = Map(), headSeq' = 0, floor' = 0, prevHead' = 0,
    connected' = true, cursor' = 0, view' = KEYS.mapBy(k => 0),
    transients' = 0, tUsed' = 0, disc' = 0, purges' = 0, refetches' = 0, dupDropped' = 0, lastRes' = "init",
  }

  pure def evOf(h: int -> Ev, s: int): Ev = if (h.keys().contains(s)) h.get(s) else { key: 0, value: 0 }

  // append_event: seq = headSeq + 1 under the event_head lock
  action appendEvent(k: int, v: int): bool = all {
    headSeq < MAXD,
    hist' = hist.put(headSeq + 1, { key: k, value: v }),
    headSeq' = headSeq + 1, prevHead' = headSeq,
    floor' = floor, connected' = connected, cursor' = cursor, view' = view,
    transients' = transients, tUsed' = tUsed, disc' = disc, purges' = purges, refetches' = refetches,
    dupDropped' = dupDropped, lastRes' = "appendEvent",
  }

  // live stream, in order: the next durable event after the cursor reaches a connected client
  action deliver: bool = all {
    connected, cursor < headSeq, cursor + 1 > floor,
    view' = view.set(evOf(hist, cursor + 1).key, evOf(hist, cursor + 1).value), cursor' = cursor + 1,
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, floor' = floor, connected' = connected,
    transients' = transients, tUsed' = tUsed, disc' = disc, purges' = purges, refetches' = refetches,
    dupDropped' = dupDropped, lastRes' = "applied",
  }

  // an already applied seq arrives again (reconnect replay, `?types=` overlap): dropped by seq
  action duplicate(s: int): bool = all {
    connected, s >= 1, s <= cursor,
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, floor' = floor, connected' = connected, cursor' = cursor, view' = view,
    transients' = transients, tUsed' = tUsed, disc' = disc, purges' = purges, refetches' = refetches,
    dupDropped' = dupDropped + 1, lastRes' = "dup_dropped",
  }

  // file.progress: no seq, not persisted, shown only
  action transient: bool = all {
    connected, tUsed < MAXT,
    transients' = transients + 1, tUsed' = tUsed + 1,
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, floor' = floor, connected' = connected, cursor' = cursor, view' = view,
    disc' = disc, purges' = purges, refetches' = refetches, dupDropped' = dupDropped, lastRes' = "transient",
  }

  action disconnect: bool = all {
    connected, disc < MAXDISC,
    connected' = false, disc' = disc + 1,
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, floor' = floor, cursor' = cursor, view' = view,
    transients' = transients, tUsed' = tUsed, purges' = purges, refetches' = refetches, dupDropped' = dupDropped,
    lastRes' = "disconnected",
  }

  // retention: the oldest events are dropped (7 days / 1M rows); the newest event always stays, because
  // the runtime role cannot read event_head and `bounds()` takes head = max(seq)
  action purge(n: int): bool = all {
    purges < MAXPURGE, n > floor, n < headSeq,  // retention keeps the head event (see FM4.md section on bounds())
    floor' = n, purges' = purges + 1,
    // a connected client that the purge leaves below the floor can never be served again (`deliver` needs cursor + 1 > floor):
    // the server closes its stream, and the reconnect gets the 410 (FM4.md B1, option b)
    connected' = if (connected and cursor < n) false else connected,
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, cursor' = cursor, view' = view,
    transients' = transients, tUsed' = tUsed, disc' = disc, refetches' = refetches, dupDropped' = dupDropped,
    lastRes' = "purged",
  }

  // reconnect with since = cursor: replay when the tail is retained, 410 + refetch at headSeq otherwise
  action reconnect: bool = all {
    not(connected),
    connected' = true,
    if (cursor < floor) all {
      // 410 DOCS_ENGINE_CURSOR_EXPIRED: list refetched, subscribe from as_of_seq = headSeq
      view' = stateAt(hist, headSeq), cursor' = headSeq, refetches' = refetches + 1, lastRes' = "gone_410_refetch",
    } else all {
      view' = view, cursor' = cursor, refetches' = refetches, lastRes' = "resumed",
    },
    hist' = hist, headSeq' = headSeq, prevHead' = headSeq, floor' = floor,
    transients' = transients, tUsed' = tUsed, disc' = disc, purges' = purges, dupDropped' = dupDropped,
  }

  action step = any {
    { nondet k = KEYS.oneOf(); nondet v = 1.to(3).oneOf(); appendEvent(k, v) },
    deliver,
    { nondet s = 1.to(MAXD).oneOf(); duplicate(s) },
    transient,
    disconnect,
    { nondet n = 1.to(MAXD).oneOf(); purge(n) },
    reconnect,
  }

  // ---- invariants ----
  // outbox seq: gap-free and monotone (retained tail is exactly floor+1..headSeq; headSeq grows by 1)
  val invOutboxGapFree = hist.keys().forall(s => s >= 1 and s <= headSeq) and hist.keys().size() == headSeq and floor <= headSeq and (headSeq == prevHead or headSeq == prevHead + 1)
  // client view = server state at the client's cursor: nothing lost, nothing applied twice
  val invViewMatchesCursor = cursor <= headSeq and view == stateAt(hist, cursor)
  // caught up at headSeq => equal to the server's current state; transients never change durable state
  val invCaughtUpEqualsServer = (cursor == headSeq) implies (view == stateAt(hist, headSeq))
  // seq are only ever assigned to durable events
  val invTransientHasNoSeq = headSeq == hist.keys().size() and tUsed >= transients
  // the 410 path is the only way out of a position below the floor: no connected client is ever stranded there (review B1)
  val invNoStrandedClient = connected implies cursor >= floor
  val inv = invNoStrandedClient and invOutboxGapFree and invViewMatchesCursor and invCaughtUpEqualsServer and invTransientHasNoSeq

  val witCaughtUp = headSeq >= 3 and cursor == headSeq and connected
  val witGone410 = lastRes == "gone_410_refetch"
  val witDuplicateDropped = dupDropped > 0
  val witTransientSeen = transients > 0 and headSeq > 0
  val witReconnectReplay = lastRes == "resumed" and cursor < headSeq
}
