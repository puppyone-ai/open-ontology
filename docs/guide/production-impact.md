# Production impact

How much the product's reads disturb a production source's own workload, as
measured by T10 (S6-H-02, decision D24). Functional correctness (T1/T2) is a
separate acceptance: a correct load can still be too heavy, and a light one can
still be wrong.

**Status (2026-11-13, E473): not signed off.** PostgreSQL 17 C1 and idle C5,
the two phases that failed most clearly before, are now within D24 with
margin, and C1 keeps up with the test's 1,900 row changes per second.
PostgreSQL changed and full-read C5 keep p95 within budget but exceed p99
through intermittent hot-row convoys on this fixture. Oracle 23's default
phases (initial load and C5, all within D24 in E472) were not remeasured; its
opt-in LogMiner C1 no longer misses its deadline but applies about 50 changes
per second, far below the test's rate. The MySQL 8.4 fixture, sized for its
write rate, was stable for its baseline and initial loads and then developed
multi-second disk stalls of its own, so its C1 and C5 verdicts measure the
host, not the product. Engine-specific side effects, sizing and the replica
option are in the [database support notes](../database-support/production-
impact.md).

## Budget and method

D24 budget, every phase against the same engine's own baseline: foreground
p95 at most +20%, p99 at most +50%, throughput at most -5%, zero foreground
errors. A product failure in a phase (a failed load, cycle or sync call) fails
the phase even when the foreground numbers are within budget.

`tools/oo-test/t10.py` runs one engine at a time. The source container is
limited to 4 CPUs / 8 GiB and holds 2,000,000 rows. The foreground is an
open-loop OLTP mix at a fixed arrival rate (60% point SELECT, 30% UPDATE of 20
hot rows, 10% INSERT) with 64 workers (32 on Oracle Free, whose listener
refuses more). The rate is half of the settled rate: burst capacity is
measured, the rate starting at 55% of it is lowered 30% at a time until a
60-second trial has no errors, and every phase offers half of that. Latency is
measured from each request's intended start, so queueing counts; a backlog
beyond 30 seconds of arrivals is an error.

Phases: a 10-minute baseline; the initial load into a fresh target at the
default 25% duty cycle, then at 100% (a loader measurement flag, not a product
setting); 10 minutes of C1 with the foreground writing; and four C5 phases,
idle (the untouched 2,000,000-row table) and with changes (all three tables,
which the foreground keeps changing), at the lite period
(`c5_full_read_every: 24`, which transfers only changed ranges) and at period
1 (full transfer every cycle). Each C5 phase repeats whole cycles of its own
fresh binding for at least five minutes (half of the baseline), so one host
IO stall in a 20-60 second cycle no longer decides a verdict.

Measurement controls, each recorded in the run summary:

- The agent DB (target) runs on tmpfs (`--target-tmpfs`): on a volume of the
  same Docker disk its write bursts stalled the source's commit fsyncs
  (PostgreSQL initial load p95 48.1 ms against 0.60 ms). No real deployment
  shares the source database's storage.
- Writer connections open before each phase clock; connecting 64 writers
  inside the first measured second biased only short phases.
- PostgreSQL slot retention is a bounded 16GB; the product refuses unlimited
  retention (`wal_retention_headroom_unproven`).
- MySQL runs with InnoDB sized for the fixture's write rate (4 GiB redo
  capacity and buffer pool, 64 MiB log buffer, `innodb_io_capacity`
  2000/4000) and default durability (`sync_binlog=1`,
  `innodb_flush_log_at_trx_commit=1`); the receipt records the applied
  values. With the 8.4 defaults the foreground stalled 3-6 s on its own.
- Every baseline reports D24 applied to sliding 50-second slices of itself
  (`self_noise`): how often the fixture fails the budget with no product.
- An fsync canary in the source container (8 KiB `O_DSYNC` write every
  100 ms on the disk the data volume shares) records host IO stalls. Each
  phase reports the canary's writes over 100 ms and the 10-second windows
  they touched, plus an informational D24 verdict with those windows removed
  from both baseline and phase. The verdicts below are the unadjusted ones.
- No other work ran on the host during a measurement. Run
  `t10-s6h02-postgres-17-r1` is kept for its C5 diagnosis only: loader
  builds on the same host during its idle C5 phase raised two windows' p95.
- `--diagnose` (PostgreSQL) adds `pg_stat_statements`, IO timing,
  checkpoint/autovacuum logs and a 250 ms activity sampler; diagnostic runs
  explain a delta and are never D24 measurements.

Unrelated containers on the host used up to 25% of one CPU in total; runs use
`--quiet-cpu-percent 25` and record the observed load.

## Measurement of record (E473)

Loader: `tools/oo-load` release with `mysql-binlog`, built from this branch.
PostgreSQL A (`t10-s6h02-postgres-17-r2`, loader SHA-256 `53a5da9c...`, commit
`335d4700e`) covers every phase; PostgreSQL B (`t10-s6h02-postgres-17-r3`,
`2470f2c0...`, commit `5de3bb5af`) remeasured the C5 phases with its own
baseline after the read-time growth fix below. MySQL (`t10-s6h02-mysql-84-r2`,
`2470f2c0...`, commit `5de3bb5af`) ran with the sized InnoDB settings recorded
in its receipt; the aborted first attempt (`t10-s6h02-mysql-84-r1`) and a
3-minute probe (`t10-s6h02-mysql-84-probe1`) chose them. Oracle 23
(`t10-s6h02-oracle-23-c1-r1`, `2470f2c0...`) remeasured opt-in LogMiner C1
with its own baseline; its loader was stopped after 22 minutes of one sync
call that could not drain (below). Raw evidence is in the worktree's ignored
`evidence/` directory under the run IDs.

| Engine (run) | Phase | p50 / p95 / p99 ms | Req/s | Errors | Seconds (cycles) | Δp95 / Δp99 / Δrate % | D24 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL 17 A | baseline | 0.310 / 0.524 / 1.260 | 4907.8 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| PostgreSQL 17 A | initial | 0.325 / 0.666 / 251.507 | 4907.7 | 0 | 49.6 | 27.1 / 19864.1 / -0.00 | fail |
| PostgreSQL 17 A | initial_duty100 | 0.312 / 0.569 / 1.632 | 4907.7 | 0 | 14.2 | 8.5 / 29.6 / -0.00 | pass |
| PostgreSQL 17 A | c1 | 0.324 / 0.545 / 1.251 | 4907.8 | 0 | 600.0 | 4.0 / -0.7 / -0.00 | pass |
| PostgreSQL 17 A | c5_idle | 0.320 / 0.535 / 1.190 | 4907.8 | 0 | 307.6 (15) | 2.1 / -5.5 / -0.00 | pass |
| PostgreSQL 17 A | c5_changed | 0.320 / 0.552 / 2.044 | 4907.8 | 0 | 319.2 (9) | 5.3 / 62.2 / -0.00 | fail |
| PostgreSQL 17 A | c5_full_idle | 0.313 / 0.495 / 1.049 | 4907.8 | 0 | 340.2 (6) | -5.5 / -16.7 / -0.00 | pass |
| PostgreSQL 17 A | c5_full_changed | 0.312 / 0.487 / 0.887 | 4907.8 | 0 | 191.4 | -7.2 / -29.6 / -0.00 | fail (product: RuntimeError) |
| PostgreSQL 17 B | baseline | 0.312 / 0.485 / 0.867 | 4966.9 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| PostgreSQL 17 B | c5_idle | 0.310 / 0.487 / 0.934 | 4966.9 | 0 | 317.1 (15) | 0.5 / 7.8 / -0.00 | pass |
| PostgreSQL 17 B | c5_changed | 0.311 / 0.502 / 1.593 | 4966.9 | 0 | 327.2 (10) | 3.5 / 83.7 / -0.00 | fail |
| PostgreSQL 17 B | c5_full_idle | 0.310 / 0.502 / 1.364 | 4966.9 | 0 | 348.9 (6) | 3.5 / 57.3 / -0.00 | fail |
| PostgreSQL 17 B | c5_full_changed | 0.311 / 0.563 / 7.757 | 4966.9 | 0 | 376.4 (4) | 16.1 / 794.5 / -0.00 | fail |
| MySQL 8.4 | baseline | 0.579 / 3.780 / 7.688 | 1983.5 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| MySQL 8.4 | initial | 0.487 / 3.286 / 9.661 | 1983.5 | 0 | 18.0 | -13.0 / 25.7 / -0.00 | pass |
| MySQL 8.4 | initial_duty100 | 0.486 / 3.268 / 10.424 | 1983.5 | 0 | 9.5 | -13.5 / 35.6 / -0.00 | pass |
| MySQL 8.4 | c1 | 1.446 / 3196.049 / 4739.793 | 1983.5 | 0 | 600.0 | 84462.5 / 61550.5 / -0.00 | fail |
| MySQL 8.4 | c5_idle | 1.395 / 2764.447 / 4511.172 | 1983.5 | 0 | 307.7 (16) | 73043.0 / 58576.8 / -0.00 | fail |
| MySQL 8.4 | c5_changed | 1.242 / 2429.191 / 4600.453 | 1982.5 | 0 | 300.6 (12) | 64172.6 / 59738.1 / -0.05 | fail |
| MySQL 8.4 | c5_full_idle | 1.036 / 2724.611 / 4607.681 | 1983.5 | 0 | 307.4 (8) | 71989.0 / 59832.1 / -0.00 | fail |
| MySQL 8.4 | c5_full_changed | 0.956 / 2664.626 / 4688.989 | 1983.5 | 0 | 350.0 (7) | 70401.9 / 60889.6 / -0.00 | fail |
| Oracle 23 C1 | baseline | 0.381 / 390.137 / 3402.925 | 5046.3 | 0 | 600.0 | 0.0 / 0.0 / 0.00 | pass |
| Oracle 23 C1 | c1 | 0.376 / 2.269 / 261.503 | 5046.3 | 0 | 600.0 | -99.4 / -92.3 / -0.00 | fail (product: RuntimeError) |

Verdicts are the D24 rule as implemented: `fail (product: ...)` is a failed
product call with the foreground numbers shown for context. Seconds count the
phase's foreground; a C5 phase's number of whole cycles is in parentheses.

### What the deltas mean

The fixture's p95 is stable and its p99 is not. Applied to slices of each
baseline as long as the phase (no product running), D24 gives:

| Run | Phase length | Baseline slices failing D24 | Largest slice delta p95 / p99 |
| --- | --- | --- | --- |
| PostgreSQL A | 50 s | 13 of 56 (p99) | +14% / +6,721% |
| PostgreSQL A | 310 s (C5) | 15 of 30 (p99) | +4% / +166% |
| PostgreSQL B | 50 s | 5 of 56 (p99) | +8% / +9,314% |
| PostgreSQL B | 310 s (C5) | 0 of 29 | +1% / +32% |
| MySQL | 50 s | 5 of 56 (p99) | +17% / +1,235% |
| Oracle C1 | 50 s | 18 of 56 (p95) | +1,253% / +85% |

- **PostgreSQL C1** is within budget with margin (p95 +4.0%, p99 -0.7%) and
  now keeps up: 265 batches in three calls applied the foreground's 1,900 row
  changes per second and drained 5 s after the phase ended.
- **PostgreSQL initial load** (A) failed p99 in one run: the canary recorded
  two host fsync stalls (340 ms and 285 ms) 9 s into the 49.6 s load. With the
  two affected windows removed from both sides it is p95 +1.6%, p99 -8.0%; run
  r1 of this round, with the same read path, passed (p95 +4.6%, p99 -5.4%).
  The verdict of record stays the failure.
- **PostgreSQL idle C5** (the lite default cycle on an unchanged table) passes
  in both runs (p95 +2.1% / +0.5%).
- **PostgreSQL changed and full-read C5.** p95 stays within budget in every
  run (largest +16.1%), but p99 fails in run B's changed (+83.7%), full-idle
  (+57.3%) and full-changed (+794.5%) phases, beyond anything B's baseline
  produced in slices of the same length (+32%). The excess comes from a few
  10-second windows per phase with stalls of up to about a second; the canary
  saw no host fsync stall in them. In run A the same phases were within B's
  p99 range or passed, against a noisier baseline, and A's full-changed phase
  failed a cycle (fixed below). A diagnostic run (`t10-diag-postgres-17-m6`,
  not a measurement) shows the mechanism: writers queue on the 20 hot rows
  (`Lock transactionid`) behind slow WAL flushes (`LWLock WALWrite`), and once
  formed the convoy persisted for over 100 s while the reader was backing off
  (digest statements averaged 8 ms, at most 23 ms). The reader contributes the
  trigger at most; its per-operation work is now small and paced.
- **MySQL 8.4** passed its baseline, initial load and 100% duty load. About 20
  minutes into the run, during C1, the fixture began stalling every writer for
  2-5 s every 25-30 s and kept doing so through every later phase, including
  idle C5, which only digests the untouched table: the canary in the container
  recorded 8-16 writes over 100 ms (up to 5.6 s) per phase against none
  before, with `Threads_running` at 58-66, CPU near 1% and InnoDB at about
  1,100 fsyncs/s. With the canary's stall windows removed every later phase is
  within the p95 budget (+4% to +17%), but each stall's queue spills into the
  next windows, so p99 stays over. No product call failed. These phases
  measure the fixture's disk, not the product.
- **Oracle 23 C1 (opt-in LogMiner).** The fixture's baseline was itself
  unstable (p95 390 ms; the canary recorded 32 writes over 100 ms, up to 6 s),
  so the foreground deltas are not meaningful; with the canary's stall windows
  removed C1 is p95 +4.3%, p99 +288%. The functional result is clear: no batch
  missed its deadline (the previous failure), the persisted mining floor
  advanced twice, but 15 batches in 22 minutes applied about 50 row changes
  per second against about 2,000 offered, so the cursor fell further behind
  and the floor stopped advancing once it trailed every new observation. The
  sync call was stopped (the `RuntimeError` above); `sync: auto` (C5) remains
  the Oracle default.

## Mitigations in this round

Each change has unit tests; PostgreSQL changes passed fast L1 on PostgreSQL 17
(T0/T2/T4/T7), the `c5_growth` live test and the A-to-B-to-A chunk-proof test;
the Oracle change passed `c1_backlog` on Oracle 23 (two calls, the second
resuming from the persisted floor); MySQL 8.4 passed fast L1
(T0/T1/T2/T4/T7/T8) and `c5_growth`. Full-scale T1 on PostgreSQL 17 and fast
L1 on Oracle 23 were not rerun: the host fell below the 20 GB free-disk guard.
Across 15 idle C5 cycles per PostgreSQL run every range digest of the
2,000,000-row table loaded through the new part reads matched the source (no
range transferred).

| Mitigation (card order) | Diagnosis | Before | After |
| --- | --- | --- | --- |
| PostgreSQL C1: bound each slot peek (`upto_nchanges`) and batch whole small transactions (8,192 changes); convert row images per relation in a few statements; pace each source operation once; apply a key's final image once per batch | A peek materializes everything it decodes; each batch decoded the backlog to the current WAL position to keep one transaction: 82 peeks returned 42.2 million pgoutput rows and spilled 266,069 temp blocks (diagnostic run, two minutes of writes). The step was paced twice (about 6% effective duty). | E472: p95 +13.0%, p99 +129.5%; one transaction per batch; the 600 s phase took 1,294 s; peak retained WAL 174 MB | p95 +4.0%, p99 -0.7%; 265 batches in 3 calls kept up with 1,900 changes/s; drained 5 s after the phase; peak retained WAL 10 MB |
| PostgreSQL C5: the first cycle after a load compares against the load's own range proofs | The first cycle had no source digests and transferred every range (55.5 MB for the idle 2,000,000-row table) | E472 idle: one cycle, full transfer, p95 +269.8%, p99 +5,588.9% | idle: 15 cycles, digest-only, p95 +2.1%, p99 -5.5% |
| PostgreSQL C5 and chunk reads: key-ordered parts of about 10 ms (digest) and 25 ms (rows plus digest) of source work, each in its own short transaction, paced | One statement hashed a 50,000-row range (about 90 ms, max 138 ms) or read it with its proof in one snapshot; foreground p99 rose while they ran | E472 full idle: p95 +15.2%, p99 +204.5% | full idle: 6 cycles, p95 -5.5%, p99 -16.7% |
| Adaptive throttle on source activity (PostgreSQL): back off 100 ms to 1.6 s while four or more other sessions wait on locks or LWLocks, then proceed | The worst stalls were hot-row lock convoys behind slow WAL flushes | none | applied to every read, digest and C1 batch; never fails a read |
| C5 range that outgrew the budget during the cycle: read the tail first, retry the table on `chunk_rows_replan` | Full-read cycles at the paced rate let the foreground's append tail pass the chunk budget before it was read | run A full changed: the whole cycle failed (`chunk_rows_replan`) | run B full changed: 4 cycles fresh, no product failure |
| Oracle LogMiner C1: mine from a floor that advances with proven observations | Every batch re-mined redo from the binding's fixed floor | E472: `logminer_deadline` within ten minutes at about 2,200 changes/s | no deadline in 15 batches; the floor advanced while the cursor kept within one observation; about 50 changes/s applied, so C1 still falls behind; `c1_backlog` drains in 22 batches over two calls, every row equal |
| MySQL fixture sized for its write rate (measurement, not product) | With 8.4 defaults the foreground stalled 3-6 s on its own | E472: baseline 50 s slices failed p95 20/56 (half rate); first attempt this round: canary writes of 7-10 s every 16-20 s | baseline slices fail p95 0/56 (largest +17%); stalls returned about 20 minutes into the run (see residuals) |

### Defects fixed in the previous round (E472)

Each fix has unit tests, a live test that fails on the previous loader and
passes on the fixed one, and fast L1 (T0/T2/T4/T7, plus T1/T8 on MySQL 8.4) on
the affected engines.

| Defect | Before | After |
| --- | --- | --- |
| Keyed C5 never moves its durable ranges, so an append tail accumulated every row inserted after the baseline until reading it exceeded the chunk budget; every later cycle failed (PostgreSQL with an unclassified `chunk_rows_replan` that failed the whole cycle, Oracle with `table_budget`). | `c5_growth` PG 17: the cycle after 6x-budget growth exits 1 and stays failed. Oracle T10 `c5_changed` / `c5_full_changed`: `sync_partial` (OTHER `table_budget`). | C5 splits a range whose discovery digest exceeds the budget through the engine's bounded splitter, gives the children fresh ids and a newer revision, reads them first, and publishes only a plan that keeps or exactly refines the parent ranges with every child staged. `c5_growth` passes on PG 17, MySQL 8.4 and Oracle 23; Oracle `c5_changed` / `c5_full_changed` fresh and within D24. |
| Oracle C5 proved a range by comparing digests taken before and after the payload, so a hot range under continuous updates was `source_changed` on every attempt. | Oracle T10 `c5_changed`: ITEMS stale `source_changed`. | C5 asks each keyset chunk for a range digest `AS OF` the payload's own read SCN, the proof PostgreSQL and MySQL bind in one transaction. ITEMS fresh every cycle. |
| Oracle LogMiner mined every commit from the cursor to the current SCN in one batch; a busy foreground made the first batch larger than the budget and no later call could progress. | `c1_backlog` (Oracle 23): `query_row_limit`, 0 batches. T10 C1: `logminer_batch_over_budget` on the first call. | The reader bisects the commit-SCN span to the first one that fits (an empty shorter span moves the lower bound), remembers it for the next batches and grows it after light batches; a single oversized commit still refuses. `c1_backlog` drains in 22 batches, every row equal. |
| Under the foreground write rate Oracle switched online redo logs between the reader's inventory and LogMiner's header check (ORA-01368). | T10 C1: first sync call failed. | The batch is mined again from the same cursor after a fresh inventory (ORA-01368/ORA-00310, up to three times); nothing of a failed batch was delivered. ORA-01291 still fails. |

Unknown Oracle server codes now keep their ORA number (no driver text), and C1
step failures print it as a separate `{"diagnostic":"source_error",...}` line,
which is how ORA-01368 was identified.

Blockers named by the previous remeasurement card: PostgreSQL changed-C5
latency and backlog rejections are gone (changed C5 at both periods passes; no
queue rejections in any run); PostgreSQL C1 no longer loses its slot (bounded
16GB retention, peak retained WAL 174 MB); MySQL C1 completes (T10 binds it at
an aligned position on the quiescent fixture; the bounded handoff repair keeps
its 100,000-key cap) and changed C5 publishes fresh; Oracle initial loads
complete without replanning and C1/C5 preparation no longer times out.


## Residuals and operator guidance

- **PostgreSQL C5, changed and full-read cycles.** p95 is within budget in
  every run, but run B's p99 exceeds +50% in changed (+83.7%), full-idle
  (+57.3%) and full-changed (+794.5%) phases through a few second-long hot-row
  convoys per phase. The product's part of them is bounded (each digest or
  read part is about 10-25 ms of source work at 25% duty, and the reader backs
  off while four or more sessions wait on locks); the convoys themselves are a
  property of a workload that updates 20 rows 1,400 times per second with
  synchronous commit on this Docker disk. This is not signed off. On a
  latency-sensitive primary with hot rows: keep the period-24 default (a full
  read only every 24th cycle), schedule C5 for low-write windows, or read from
  a replica, and repeat T10 with the actual workload; `oo doctor` cannot
  detect hot-row convoys in advance.
- **PostgreSQL initial load.** One failure of record (p99, two host fsync
  stalls recorded by the canary) against a pass with the same loader; repeat
  on the target host before relying on either.
- **PostgreSQL C1** passes with margin and keeps up with 1,900 changes/s at
  the default 25% duty cycle. Still size a bounded `max_slot_wal_keep_size`.
- **MySQL 8.4: not signed off, fixture-limited.** With the receipt's sizing
  (4 GiB redo capacity and buffer pool, 1 ms binlog group-commit delay,
  default durability) the baseline was stable for its 10 minutes (p95
  slices within +17%, no canary write over 100 ms), and the initial load at
  both duty cycles passed. About 20 minutes into the run, during C1, the
  fixture began stalling all writers for 2-5 s every 25-30 s: the canary in
  the container recorded 18 such writes in C1 and 8 in the idle C5 phase,
  which only digests the untouched table, with `Threads_running` at 58-66 and
  CPU near 1% during each stall and InnoDB at about 1,100 fsyncs/s. Those
  phases are therefore not attributable to the product. Repeat MySQL on a
  host whose storage sustains the fixture's fsync rate (not a desktop
  container VM) before relying on C1 or C5 there; keep `sync: auto` (C5) and
  measure C1 with the actual workload first.
- **Oracle C1 (opt-in).** Keep `sync: auto` (C5). `oo doctor` reports the
  `logminer_reader` requirement for every Oracle source with the measured
  limit: the mining floor now advances while the cursor keeps up, but at about
  2,000 row changes per second C1 applied about 50 per second and fell
  behind. Opt in only for a change rate measured on the actual workload well
  below that, size online redo logs so they do not switch every few seconds,
  and keep the logs from the persisted mining floor onward.
- **100% duty cycle.** Exists only as the loader's `--duty-cycle-percent`
  measurement flag; no stack or lite setting exposes it. Keep 25%.
- **Storage.** Never put the agent DB's data on the source database's disk.
  The fixture host itself (a desktop container VM whose disk file sits on a
  nearly full volume) produced most of the remaining stalls; repeat T10 on
  production-class storage before treating any p99 verdict here as final.
- Keep the other defaults: source-activity gates (PostgreSQL 16 active
  sessions / 30 s pause plus the lock-wait back-off, MySQL 16 running threads
  / 120 s, Oracle 16 active sessions / 60 s) and `snapshot: chunk_consistent`
  for keyed PostgreSQL and MySQL tables. C5 idle cycles still compute source
  digests over the whole scope: change-only transfer is not change-only source
  IO.
- These are fixture measurements, not capacity certifications. Before enabling
  a binding on a busy primary, repeat the measurement with the actual workload
  and an absolute latency objective as well as the relative D24 budget.

## History

The previous round's measurement (E472, runs `t10-final-*`) is superseded:
PostgreSQL 17 C1 p95 +13.0% / p99 +129.5% with a 1,294 s drain, idle C5 p95
+269.8% (a full transfer on the first cycle), full-idle C5 p99 +204.5%;
Oracle 23 every default phase within D24 but opt-in LogMiner C1 failing
`logminer_deadline`; MySQL 8.4 inconclusive on a fixture that stalled 3-6 s on
its own. Earlier attempts are kept as evidence reports:
[T10-live-4](../../tools/oo-test/T10-LIVE4.md) (no engine passed every phase),
[mitigation 1](../../tools/oo-test/T10-MITIGATE.md) (C5 period 24, half the
settled rate) and [mitigation 2](../../tools/oo-test/T10-MITIGATE2.md)
(chunk-consistent reads and their correctness evidence).
