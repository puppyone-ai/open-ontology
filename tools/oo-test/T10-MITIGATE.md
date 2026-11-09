# T10 mitigation handoff: partial, not accepted

S6-H-02 remains **not done**. Evidence run: `t10-mitigate-20261109`.
Base commit: `8ba3e4685`. No product snapshot or throttle defaults changed.

## Delivered

Automatic calibration still measures burst capacity at the same concurrency
levels, starts settling at 55% of capacity, and uses the same 60-second trials
and 30% decrements. All measured phases now offer **50% of the settled rate**.
The receipt preserves `capacity`, the original calibration `rate`, and all
`settle.trials`; it adds `settled_rate`, `load_fraction`, and `used_rate`.
Explicit `--rate` remains an override, with capacity and settled rate recorded
as unmeasured (`null`). It is not evidence of moderate production utilization.
The D24 thresholds, request mix, queue, scale and phase durations are unchanged.

## Validation and live status

- `PYTHONPATH=tools/oo-test python3 -m unittest discover -s tools/oo-test -p test_t10.py`:
  51 passed, including half-settled-versus-half-capacity and idempotence checks.
- `cargo check --workspace --locked`: passed.
- `git diff --check`: passed.
- `PYTHONPATH=tools/oo-test python3 -m unittest test_t1 test_t2 test_t3t4 test_oracle_t1`:
  39 offline fixture tests passed; these are not live correctness runs.
- `python3 specs/scripts/check_spec.py`, `check_plan.py` and
  `check_v02_plan.py`: all passed (document checks, not product acceptance).
- The first `quiet_host()` preflight refused the host. A later invocation passed
  (eight pre-existing containers, maximum individual peak CPU 0.02%).
  The refusal was transient, **not an ongoing environment blocker**.
- Only Docker info/stats were read. No containers were created or modified.
- `cargo test -p oo-connectors --lib --locked protect` selected zero tests.
  This is **not** evidence of protection coverage; optional engine features
  were not enabled in that invocation.

No new live T1, T2 or T10 phase ran. There are no new latency, throughput,
side-effect or mitigation before/after measurements:

| Engine requested | baseline | initial | initial_duty100 | C1 | C5 idle/changed, periods 24 and 1 |
| --- | --- | --- | --- | --- | --- |
| PostgreSQL 17 | not run | not run | not run | not run | not run |
| MySQL 8.4 | not run | not run | not run | not run | not run |
| Oracle 23 | not run | not run | not run | not run | not run |

The live-4 numbers remain historical evidence, not results of the new load
policy. In particular, halving foreground rate is not a product mitigation and
cannot be compared causally with live-4 to claim a snapshot fix.

## Unresolved product and fixture work

1. PostgreSQL `tools/oo-load/src/main.rs` exports a snapshot before invoking
   either initial loading or C5 and closes the owner only after completion.
   The connector opens per-operation transactions, but importing that snapshot
   still retains its original xmin. Removing the owner alone is insufficient:
   no-key physical plans require it, and publication correctness needs proof.
2. In `tools/oo-load/src/c5.rs`, keyed C5 reads source digests, reads a chunk,
   and checks a later digest plus row count. Code inspection identifies an
   A-to-B-to-A risk if these are independent snapshots: matching before/after
   digests do not establish that the middle payload was A. This is a review
   finding, **not a reproduced live failure**. A safe implementation must bind
   the payload and its digest proof to one bounded chunk snapshot (or provide
   an equivalent verification), then test intervening updates, deletes and key
   moves, including correction on the next cycle. Do not enable a new lite
   default on the strength of the existing comparison alone.
3. MySQL `read_snapshot` already opens and closes a transaction per chunk.
   That does not prove the full initial/C5 publication contract or an elapsed
   transaction bound. Those tests and full-scale reruns remain required.
4. MySQL T10 C1 preparation inherits a 100,000-key / 32 MiB handoff budget
   from `tools/oo-test/t3t4.py`. `tools/oo-load/src/handoff.rs::scope` explicitly
   rejects larger values; its regression test rejects 100,001 keys. The T10
   bulk table alone has 2,000,000 keys. This cannot be fixed by increasing a
   harness parameter while keeping the current handoff contract. A bounded,
   durable repair path is still needed; the cap and fixture were not weakened.
5. Existing activity gates are enabled by default: PG limit 16 with a
   30-second pause bound, MySQL running-thread limit 16 with 120 seconds,
   Oracle active-session limit 16 with 60 seconds. No new tuning, complete
   gate test suite or before/after measurement was delivered.
6. PG T10-only slot-retention sizing and Oracle measured-rate timeout/undo
   sizing remain unresolved. No source setting or timeout was changed without
   a measurement. Oracle's live-4 interrupted loads are not completion rates.

No shared-file code patch or Cargo.lock change is proposed. No untested
snapshot-removal patch is presented as integration-ready. Finish the product
proof and bounded repair first, then run full-scale PG/MySQL T1/T2 and the
three-engine phase matrix, one mitigation at a time. The existing replica,
low-write-window and Oracle LogMiner caveats still apply.
