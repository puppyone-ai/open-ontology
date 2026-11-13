# T10 mitigation 2: validation in progress

S6-H-02 is **not yet signed off**. Raw evidence run: `t10-mitigate2-resume-20261109`.
Base: `6e9895917f694a5797cb8d865bcf226e74cfc1b3`. Final candidate loader SHA-256: `24a6956dbd75ac059915f2c33c3d6365316e5680fd078d6cda0647bae4ed043b`.
No customer source, public Gateway, unrelated container or running service was changed.

## Implementation

- Keyed PostgreSQL and MySQL chunks bind payload and canonical range digest to one
  read-only repeatable-read transaction. The combined read/proof has the configured
  statement deadline; cleanup is capped at one second. Row/byte limits remain intact.
- `snapshot: chunk_consistent` is the default. `snapshot: single` explicitly retains
  the long snapshot. PostgreSQL physical/no-key tables retain a per-table exporter;
  keyed tables in the same binding do not inherit that exporter. Unprovable catalog
  key shapes conservatively retain snapshot mode.
- C5 stores the payload proof, not an unrelated before/after snapshot. Consistency
  and bounds are stored in receipts, retained publication plans and C5 state.
  This is local retained-generation metadata, not a public Gateway acceptance claim.
- PostgreSQL catalog fingerprints are table-local; reader sessions disable parallel
  gather workers. C1 closes cached metadata readers before capture beside feedback.
- C5 no longer counts PG/MySQL per-operation duty sleeps and target work as source
  work a second time. Per-operation duty, active-session gates and the one-second
  metadata floor remain. Oracle retains its existing whole-cycle pacing.
- T10 MySQL C1 uses the quiescent aligned-baseline position after loading, not the
  capped handoff repair path. PostgreSQL T10-only slot retention is set to `-1` and
  verified. Oracle uses a 20,000-row load probe and records per-binding deadlines
  `max(900, ceil(3 * current_rows / measured_rows_per_second))`, before foreground work.

## Correctness evidence

| Test | Engine/version | Result | Run ID |
| --- | --- | --- | --- |
| T1/T8, 2,000,000 events + 200 small tables, all type fixtures | PG 17 | pass; 233 tables equal | `t10-mitigate2-t1-pg17-final2-20261109` |
| T1/T8, same requested scale/type fixture | MySQL 8.4 | pass; 231 tables equal | `t10-mitigate2-t1-mysql84-final2-20261109` |
| T2, original small fixture | PG 17 | C1/C5/T2 pass | `t10-mitigate2-t2-pg17-final2-20261109` |
| T2, original small fixture | MySQL 8.4 | C1/C5/T2 and handoff pass | `t10-mitigate2-t2-mysql84-fixed-20261109` |
| T2, 2,000,000 baseline rows per table | PG 17 | pass | `t10-mitigate2-t2-pg17-scale2m-20261109` |
| T2, 2,000,000 baseline rows per table | MySQL 8.4 | pass, including oversized refusal and bounded handoff | `t10-mitigate2-t2-mysql84-scale2m-r3-20261109` |
| Deterministic ABA update/delete/key move | PG 17 | 3 cases pass; 4 range proofs each | `t10-mitigate2-proof-postgres-r2-20261109` |
| Deterministic ABA update/delete/key move | MySQL 8.4 | 3 cases pass; 3 range proofs each | `t10-mitigate2-proof-mysql-20261109` |

The adversarial test independently hashes the exact transferred rows through an
admin-populated mirror, verifies every range proof and requires next-cycle
convergence after quiescence. Product accounts only read; fixture admins mutate.
This complements actual retained-generation T1 comparisons, not public publication.
Full-scale T2 covers two numeric/text tables, not T1's complete type matrix. The
100,000-key / 32 MiB handoff cap is unchanged: oversized scope must refuse atomically;
positive handoff remains a separate bounded fixture with the full-scale source intact.

Final T1 source samples: PG initial/C5 peaks 2 readers, duty 7.432/7.378 percent;
MySQL peaks 1 reader, duty 22.048/16.967 percent. All original connection/duty limits
remain. MySQL C5 wall time changed from 2506.568 to 807.872 seconds after removing
whole-table duplicate duty repayment. This is a correctness-fixture timing, not D24.

## Preserved failures and corrections

- The inherited optional loader build failed before the resume; no prior pass was assumed.
- An intermediate metadata refactor failed Rust borrowing checks; fixed before its commit.
- T2's old receipt parser raised `KeyError` on both engines; the unchanged base binary
  and archived harness reproduce it. New aggregate parsing still rejects stale tables,
  missing summaries and ambiguity, and keeps equality/zero-write checks.
- Full-scale PG T1 failed with `max_rows_per_sec` and a sampled reader peak of 3.
  Table-local catalog reads, bounded reader parallelism and session lifetime fixes
  precede the successful full rerun. No row or connection cap was raised.
- The first PG adversarial test caught a table-scope mismatch in digest metadata;
  the corrected range digest passes all three real interleavings.
- The first large MySQL T2 no-op observed 16 KiB physical target growth despite zero
  logical writes/generations. The rerun settles target maintenance with an explicit
  fixture-admin VACUUM before the measurement and logs relation fork sizes. Source
  autovacuum stays enabled; the exact zero-growth assertion is unchanged. Both C1
  and C5 then pass. Its oversized-handoff assertion used the wrong error rendering;
  the product correctly refused with `source error: budget fact=mysql_repair_staging_budget`.
- An attempted interruption selected zero loader processes and sent no signal: the
  first MySQL T1 had already passed and cleaned up. The correction receipt is retained.

## Offline validation

`cargo check --workspace --locked`, full `oo-connectors` tests with `mysql-binlog`,
full `oo-pipeline` and `oo-stack` tests, and standalone `oo-load` tests pass.
Ignored database-dependent tests are not counted as executed. All eight
`file_worker_parent` tests passed on this host; no base-failure exemption was needed.
The latest offline fixture suite has 100 passing tests. Formatting was run on all
changed Rust crates and the standalone loader. No Cargo.lock or migration changed.

## Production impact

The large T2 gate passed. Three-engine impact measurement is running serially,
starting with Oracle. Its 600-second baseline has zero errors at about 5291
requests/second; initial loading is still running. No new product-phase D24 pass
or Oracle sizing recommendation is available yet. Live-4 remains historical evidence at a different offered load.
Raw failures are never overwritten. See the two Production impact guides for the
consistency contract and provisional operator cautions.

## Resume note while measurements run

The impact process uses the frozen `oo-load-final2` artifact in the validation
evidence directory. A subsequent uncommitted replay-provenance patch in
`engine_load_pg.rs`, loader `main.rs` and `c5.rs` reads retained consistency
metadata instead of relabeling a replay from the caller's current snapshot option.
Build and live-test that target-only metadata follow-up after impact measurement;
do not introduce compiler load into the running benchmark. Documentation and the
target-maintenance fixture adjustment are also pending their final validation commit.

During the first new Oracle measurement, both initial modes failed with
`chunk_count`; C1 preparation then reached its formula-derived timeout.
Quiescent target diagnostics retained 24,401 OTHER chunk receipts at revision 12.
Code inspection found that Oracle's existing bounded keyset splitter was exposed
only as an inherent method, not the `SourceEngine::split_chunk` hook consumed by
the loader. A trait delegation and Oracle extension of the real split fixture
are now uncommitted follow-ups: validate them after the current measurements,
then remeasure Oracle with a fresh run. The failed run remains evidence, not a pass.
