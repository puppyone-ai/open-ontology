# FM4 review: event outbox and client catch-up (`formal/quint/event_stream.qnt`)

Model of 10 section 8.5 and `_oo_docs.append_event` (migration 0138, `store/events.rs`). Card V-S2-02 model
part. Status: author checks only. The renderer-side replay (`tests/unit/super-drive/conformance/`) and the D48
freshness comparison are not part of this change.

| Rule | Implementation | Model |
|---|---|---|
| seq assigned under the `event_head` row lock, gap-free, monotone | `append_event` (`UPDATE event_head SET seq = seq + 1`, insert) | `appendEvent`, `invOutboxGapFree` |
| client applies in order, drops seq <= cursor | spec 8.5 (dedupe by seq) | `deliver`, `duplicate`, `invViewMatchesCursor` |
| `since` replay of the retained tail | `read_since` | `reconnect` (resumed) |
| cursor older than retention floor: 410, refetch list, subscribe from `as_of_seq` | spec 8.5 | `purge`, `reconnect` (gone_410_refetch) |
| transient events carry no seq and do not change durable state | spec 8.5 | `transient`, `invTransientHasNoSeq` |
| caught up at head => same state as the server | spec 80 FM4 | `invCaughtUpEqualsServer` |

Bounds: 5 durable events, 2 keys, 2 transient events, 2 disconnects, 1 purge. Not modelled: `?types=` filtering,
`stream.lagging`, the 15 s ping, multiple clients, freshness (D48), real transaction ordering across
sessions (the `event_head` lock makes seq order commit order; `jobs_live.rs` tests concurrent appenders).
Liveness (client eventually reaches head) is shown only by witnesses, not model checked.

## CI bounds (card V-S1-01, time budget of 80 section 6)

Per invariant, 10 steps, same conditions as in FM1.md (loaded 16 core machine): invOutboxGapFree 14 s,
invTransientHasNoSeq 12 s, invCaughtUpEqualsServer 176 s, invViewMatchesCursor 561 s. All inside 12 minutes, so the
FAST bound stays `verify-steps=10`; the nightly bound is `deep-steps=12` (not measured).
