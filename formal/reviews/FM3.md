# FM3 review: incremental sync, fs backend (`formal/quint/sync_reconcile.qnt`)

Model of 10 section 4.3.4 / 4.3.5 (generation mark-sweep, targeted sync D47, watcher hints D46) against
`crates/oo-docs/src/ingest/reconcile.rs`. Card V-S3-02 (fs). Status: author mapping, checks and conformance; the
independent 80 section 5 step 2 review is open. s3 semantics (versionId, sequencer, reordered and repeated
events, KP3) are card V-S4-01 and are not in this model.

## Mapping

| Rule | Implementation | Model |
|---|---|---|
| `g = sync_generation + 1`; committed only by a complete sync | `run_sync_source`, `finish_sync` | `startSync` (`g = gen + 1`), `finishSync` (`gen' = g` only in the sweep branch) |
| listing, then apply: new rows `stored` + hash, existing rows marked `seen = g`, changed (fingerprint) rows hashed, deleted rows touched and hashed | `sync_core` first loop | `upsert` (`hp` = hash job pending, `v` = fingerprint identity, `c` = content of the current version) |
| the sweep takes live rows with `last_seen_generation < g` from the database, not from the snapshot | `load_missing` | `missing` (rows touched by a targeted sync carry `maxg` and are safe) |
| partial / failed listing records what it saw and never sweeps; the generation is not committed | `sync_core` (`full`) | `finishSync`, branch `not(ls.complete)` |
| mass delete (more than `mass_delete_min` and more than half of the live rows, or an emptied root) is held until `confirm == g` | `sync_core`, `enqueue_sync` | `mass`, `held`, `confirm` (a confirmed run is `startSync(.., confirm = true)`) |
| rename: a missing file takes the first new file of the same listed size whose hash, taken now, equals its own; the old row is deleted, the new row gets the version | `detect_renames`, `apply_rename` | `pairing` (listed size = content, hash taken from `src` at finish time) |
| the sweep deletes the file and cancels its pending jobs | `files::delete_file` | `swept` (`st: 2, hp: false`) |
| targeted sync: create or update, mark with `max(drive_sync.generation)`, never delete, never move the generation | `targeted_sync` | `targeted` |
| hash job: records the current content, revives a deleted row; an object that is gone leaves the row alone | `hashjob::run_hash` (`nothing_to_do`) | `hashAll` |

## Invariants (`vinv`)

`invOnlySweepKills` (a live row stops being live only in a complete, unheld sweep: partial, held, targeted, write
and hash never delete), `invSweepOnlyUnseen`, `invGenerationMonotone`, `invHeldAfterMassOnly`, `invRenameCarries`,
`invConverged` (after a complete, unheld sync that saw the folder as it is, with no write since it started and once
the hash jobs are done, the live rows are exactly the objects of the folder with their content).

## Finding FM3-001 (fixed in this branch)

With `RETRY_RESETS_MARKS = false` (the code as it was), `invConverged` has a 7 step counterexample
(`formal/traces/regressions/FM3-001.itf.json`): a failed or held attempt of generation g leaves `seen = g` on every
row it listed. The next attempt is again generation g (the generation moves only on a complete sync), so a file that
was deleted between the two attempts has `seen = g` and is not swept; it stays live until the sync after that. The
spec property "after quiescence one reconcile makes the index equal the folder" does not hold. Reproduced on the
real code by `ingest_fs::a_retried_generation_forgets_the_marks_of_the_failed_attempt` (fails without the fix,
passes with it). Fix: `start_sync` forgets the marks of the generation when its `drive_sync` row already exists
(`last_seen_generation = g - 1` for live rows with `>= g`), under the drive lock, before listing.

## Bounds

4 keys, 3 contents, at most 6 source operations, 1 partial listing, 2 targeted syncs, `mass_delete_min` 1. FAST
bound 8 steps; the nightly bound is 10 (`deep-steps`). Durations: `testing/super-ontology/evidence/L2/formal.md`.

## Not covered

Pages (a listing is one step), listings that change while they are read (a listing is a snapshot; writes fall between
the snapshot and the apply), symlinks, invalid or colliding names, `max_files`, sniffing and sizes, hash backlog
pause, parallel syncs (the drive lock serialises them), the scheduler, and every s3 or multi-cloud event semantic.
Liveness (the index converges) is only the safety form `invConverged`; no TLC check. Hash jobs are drained by one
action, so a write between two hash jobs of one drain is not explored.

## CI bounds (card V-S1-01, time budget of 80 section 6)

Per invariant at 8 steps on a loaded 16 core machine (see FM1.md): invOnlySweepKills 48 s, invSweepOnlyUnseen 52 s,
invHeldAfterMassOnly 57 s, invGenerationMonotone 58 s, invRenameCarries 92 s, invConverged 499 s. FAST bound
`verify-steps=8`; nightly `deep-steps=10` (not measured).

## Independent review (native sub-agent, 2026-11-17, commit e82490f57)

Verdict: APPROVE WITH CHANGES. Reviewed: `sync_reconcile.qnt` against 10 4.3.4 to 4.3.5, `ingest/reconcile.rs` (`start_sync` marks reset, `sync_core` sweep, `load_missing`, `detect_renames`, `targeted_sync`), and `tests/conformance_sync.rs` (`compare`, `replay_steps`). Checks: `quint typecheck` passes. `quint run` (seed 7, 40 steps) on the unmodified model finds no violation. The FM3-001 mutant (`RETRY_RESETS_MARKS = false`) is caught by `inv`.

Blocking
1. **B1, the header claim "a listing overtaken by writes does not delete what was written" is not checked, and the model violates it.** `finishSync` sweeps `missing` = live rows with `seen < g`, with no test that the folder still lacks the key. A key written after the snapshot, with no targeted hint, is swept. Probe (copy in /tmp, not committed): `not(lastKind == "sweep" and KEYS.exists(k => rows.get(k).st == 2 and src.get(k) != 0))` gives "Invariant violated" in 40 ms. The code does the same (`load_missing` is the DB set, `reconcile.rs` ~l.822 to l.833), so the row is deleted while the file exists and is only revived by the next sync. Either (a) the claim is narrowed to "the sweep does not delete a key written with a targeted hint (D46/D47)" and the invariant is written with that precondition (track a ghost `hinted` per key), or (b) the code gains a current-existence check before deleting. Choose one and state it in the header; as written, the header promises something the model contradicts.
2. **B2, the mass-delete hold has no invariant.** The branch `mass and not(ls.confirm)` is the only protection for "mass delete is held until confirmed", but mutating it to `else if (false)` leaves `inv` at "No violation found" (seed 7, 3000 samples). Add a ghost set in the sweep branch (for example `sweptMass = mass and not(ls.confirm)`) and the invariant `not(sweptMass)`; or `lastKind == "sweep" implies not(massUnconfirmed)`.
3. **B3, `invSweepOnlyUnseen` (= `sweptSeenOk`) is tautological against the code.** `missing` is defined as `seen < g` after `upsert`, so `missing.forall(seen < g)` holds by construction. It fails only for a mutant that changes `missing` itself, which is not what the header calls "only unseen". Replace it with the real claim: a key is swept only if its row was not marked by this generation and (per B1) not written in the listing window.

Non-blocking
4. `v` (fingerprint version) is not compared in `conformance_sync.rs::compare` (only `st`, `seen`, content, pending hash, generation, held). Change detection (`upsert` `r.v != snapV`) is therefore not conformance-tested. Add `v` or document why the content hash suffices.
5. `invConverged` is only checked when `quiet`; `quiet` is false after any write during the listing, so the convergence claim is weak exactly in the window where B1 happens. Acceptable if B1 is resolved as (a).
6. The `held` flag is compared only for generation `gen + 1` (`conformance_sync.rs` ~l.380). That matches the model; note it.
7. Bounds: K=4 keys, C=3 contents, MAXW=6 writes, MAXP=1 partial listing, MAXT=2 targeted syncs, MASSMIN=1 (the test configuration, not the production 100). The mass rule is exercised only with a threshold of 1; the production threshold (100) is not reached by any model run. State this in the bounds.
8. The fingerprint and `rows.v` change model is abstracted; `ver` is per-key, not per-object identity. Say so, since rename detection (`detect_renames`) depends on the hash of a file taken "now", which the model reads from `cur`.

Checked and fine: the FM3-001 fix (`start_sync` marks reset in `reconcile.rs` l.323 to l.350) matches `RETRY_RESETS_MARKS` (model resets only live rows with `seen >= g`, code does the same); `partial` never sweeps (`invOnlySweepKills` holds on a mutant that sweeps on partial would fail it, not run); `targeted` never changes `gen` (matches code: no generation move on targeted).
