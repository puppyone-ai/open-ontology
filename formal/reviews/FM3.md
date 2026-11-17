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
