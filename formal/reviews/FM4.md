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

## Independent review (native sub-agent, 2026-11-17, commit e82490f57)

Verdict: APPROVE WITH CHANGES. Reviewed: `event_stream.qnt` against 10 8.5, `api/events.rs` (`parse_since`, `stream`, `run`), and `tests/conformance_events.rs` (header mapping, `deliver`/`duplicate`/`transient`/`purge` replay). Checks: `quint typecheck` passes; `quint run` (seed 7, 40 steps) finds no violation for `inv` on the unmodified model.

Blocking
1. **B1, the 410 path is not protected by any invariant.** Mutant `if (cursor < floor) all {` replaced by `if (false) all {`: `inv` still gives "No violation found" (seed 7, 3000 samples). The refetch branch is only a witness (`witGone410`). Without it, a client whose cursor is below the floor is stuck: `deliver` needs `cursor + 1 > floor`, and nothing else moves the cursor. Add a liveness-style safety property that a model run can check: `connected implies cursor >= floor` is violated by the model in 12 ms (probe, not committed), so the model shows that a purge can strand a connected client. The code handles the reconnect case (`api/events.rs` l.214 `gone`), but the live case (purge while connected, client below the new floor) is not specified: `stream.lagging` (l.31, l.313) is about slow clients, not retention. Decide and write it down: (a) the purge guard must not pass the oldest connected cursor, or (b) the server closes the live stream with `CURSOR_EXPIRED` and the model adds that transition, or (c) the spec says live clients cannot be purged past. Then the invariant follows.
2. **B2, the conformance test cannot see a stuck client.** `conformance_events.rs` replays `deliver` as "the next frame has seq cursor+1". A purged cursor never gets a frame, so the replay of a trace that reaches the stuck state will fail only if the trace contains a `deliver` action there; the model has no such action at that point (guard false). The test therefore does not check the state where the client is stuck. Add a compare of the client's cursor and `floor` (`oldest - 1`) after each step, not only the view.

Non-blocking
3. `purge` keeps the ghost `hist`, so `invOutboxGapFree` checks gap-freedom of the full history, not of the retained tail (`floor+1..headSeq`). Model the retained set (`hist` filtered by `seq > floor`) if the claim "retained tail is exactly floor+1..headSeq" in the comment is to hold.
4. `reconnect` with `cursor > headSeq` (the code's `s > head` branch, l.214 to 219, also returns `CURSOR_EXPIRED`) is not modelled. It cannot happen in the model because `cursor <= headSeq` always. Say so in "Not modelled", since a client can send any `since` value.
5. The race between `bounds()` (l.208) and the start of `run` (purge in between) is not modelled. It only matters for a client whose `since` is at the old floor; mention it as an open point.
6. `invTransientHasNoSeq` is `headSeq == hist.keys().size() and tUsed >= transients`. It does not say that transients never enter `hist` or `view`. Add `view` and `hist` unchanged by `transient` as a check, or rename the invariant to what it checks.
7. `duplicate(s)` is accepted for `s <= cursor` only. A duplicate of a purged seq (the code's replay returns 410 for it) is not modelled; fine, but say so.

Checked and fine: `appendEvent` is gap-free and monotone (`headSeq + 1`, `invOutboxGapFree` holds on the unmodified model); the code 410 condition `s + 1 < oldest` equals the model's `cursor < floor` when `floor = oldest - 1`; `deliver` always applies `cursor + 1`, so out-of-order delivery cannot be expressed in the model at all (the `seq` check of the test is therefore a real check only of the real stream).

## Response to the independent review (lane A, 2026-11-17)

- **B1:** reviewer option (b). A purge that leaves a connected client below the new floor now closes that connection in the model
  (`purge`: `connected' = connected and cursor >= n`), and the reconnect takes the 410 branch. Invariant `invNoStrandedClient`
  (`connected implies cursor >= floor`); mutation M1 (purge leaves the client connected) is caught by it, before nothing failed.
  **Code finding, no patch:** the live loop in `api/events.rs::run` reads `seq > cursor` and has no check that `oldest <= cursor + 1`, so a purge while a client is
  connected and behind would make it silently skip the purged events. Nothing in the engine deletes events today (no retention job for the `event` table), so
  the state is unreachable now; the job that is added later must close or 410 such streams. Open item for the board lead.
- **B2:** the replay already compared `floor` and `cursor` after every step; it now follows the model on purge (drops its stream when the model closes it).
  It cannot see the real stream's behaviour in that state, for the reason above.
- Not done: 3 (retained-tail ghost), 4 (`since > head` is not modelled: the model keeps `cursor <= head`), 5, 6, 7 (a duplicate of a purged seq is not modelled).
