# TA-01 continuation: source snapshot slice

S6-H-01 remains incomplete. This is not an accelerated-suite acceptance.
Raw evidence: `evidence/ta-01-continue/` and the per-run directories below.

## Changes

- T3/T4 background product timing from spawn through finish; failures remain
  measured and repeated waits do not double-count.
- Opt-in `oo-test snapshot`: cold source data and labelled container image,
  content-key invalidation, per-key locking, APFS COW restore, ordinary
  fixture shape/grant/gold validation, private cached credentials and owned
  cleanup. The normal full-suite commands are unchanged.
- Recognize `ta` as an owned T1-runner lane. No product code changed.

## Validation

`CARGO_BUILD_JOBS=2 cargo check --workspace --locked`: pass.

`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v`:
225 tests pass. No Rust files changed, so crate formatting is not applicable.

Live commands:

```sh
tools/oo-test/oo-test snapshot --engine postgres --versions 13,14,15,16,17,18 --run-id ta-01-snapshot-pg-build2
tools/oo-test/oo-test snapshot --engine postgres --versions 13,14,15,16,17,18 --run-id ta-01-snapshot-pg-restore2
tools/oo-test/oo-test snapshot --engine mysql --versions 5.7,8.0,8.4,9 --run-id ta-01-snapshot-mysql-build2
tools/oo-test/oo-test snapshot --engine mysql --versions 5.7,8.0,8.4,9 --run-id ta-01-snapshot-mysql-restore2
```

All ten engine/version pairs pass cold and warm checks at 2,000 rows and
three small tables. Each reruns the existing gold, shape and grant assertions.
The version-9 MySQL lock resolves to 9.5.0.

| Engine/version | Cold fixture seconds | Warm fixture seconds | COW clone seconds |
| --- | ---: | ---: | ---: |
| PG 13 | 8.109 | 7.296 | 0.230 |
| PG 14 | 8.338 | 7.271 | 0.244 |
| PG 15 | 8.217 | 7.252 | 0.243 |
| PG 16 | 8.232 | 7.247 | 0.230 |
| PG 17 | 8.395 | 7.245 | 0.239 |
| PG 18 | 7.659 | 7.282 | 0.239 |
| MySQL 5.7 | 18.726 | 9.001 | 0.074 |
| MySQL 8.0 | 13.453 | 6.895 | 0.035 |
| MySQL 8.4 | 12.728 | 6.894 | 0.037 |
| MySQL 9.5 | 16.928 | 6.859 | 0.045 |

These are fixture timer lifetimes through cleanup, not product execution
or the requested L0+L1+L2 wall time. The COW column excludes server startup.
Per-stage outputs are retained as `timing-{pg,mysql}-{build,restore}.tsv` in
`ta-01-continue`; reproduce with `oo-test timing evidence/<run-id>`.
`product_run` is correctly unmeasured for these source-only runs.

Live mutation-isolation checks changed an order amount to 999 in a restored
PG 17 and MySQL 8.4 clone, cleaned up, then restored again. Both new clones
reproduced all original gold answers. Receipts:
`ta-01-isolation-{postgres,mysql}`, `ta-01-pristine-{postgres,mysql}`;
driver output: `ta-01-continue/clone-isolation.txt`.

The first MySQL cold runs failed because the generic external-symlink
rejection encountered `mysql.sock -> /var/run/mysqld/mysqld.sock`.
Those failures are retained in `ta-01-snapshot-mysql-build` and
`ta-01-snapshot-mysql-links`. The fix removes only that exact transient
link after shutdown; other symlinks still fail. No gold values changed.

## External validation constraint

`oo-test snapshot --engine oracle --versions 11,19,21,23
--run-id ta-01-snapshot-oracle-preflight2`: all four return
`oracle_slot_occupied`. Lane T's existing Oracle XE 11g container is running.
Under the card's one-Oracle limit, starting another Oracle would exceed the
limit; the runner neither stops that container nor bypasses the cap.
The occupied slot is recorded in `ta-01-continue/oracle-slot.txt`.
Oracle 19c EE remains explicitly unvalidated along with 11g/21c/23ai.

59 historical `s6h01c-*` index receipts in the permitted sibling evidence
tree were readable. Their relative paths and hashes are retained in
`historical-inventory.json`. No fast/full comparison was performed; missing
historical coverage profiles are not treated as a blocker.

## Card exits

1. Timing remains partial: the asynchronous gap is closed, but the remaining
   independent installation/T5/T6 and inline-stage boundaries are not closed.
2. Snapshots are partially delivered: all PG/MySQL cold/warm fixture checks
   pass; Oracle's exit is blocked by the occupied slot. Linux COW is untested.
3. Suite-specific shared schemas/slots and destructive clone routing: not done.
4. Boundary fixture product branch-coverage equivalence: not done.
5. Defect corpus and replay gate: not done; zero mutations executed, not a pass.
6. L0/L1/L2, rotation, probes, impact selection and global scheduler: not done.
7. Product failpoints, toxiproxy and release-build exclusion: not done.
8. Thirty-minute acceptance, coverage comparison, same-SHA differential and
   CI wiring: not done. No production or accelerated-suite claims.

No shared-file patches or Cargo.lock changes. No other worktree was written.
Only generated source fixtures were changed through their admin accounts.
No model credentials, running services or unrelated containers were modified.
Final ownership-filtered checks found zero TA containers and zero TA volumes.
Ten current cold snapshots and labelled images are intentionally retained.
Six obsolete snapshots created earlier in this run were removed only after
checking their key, lane and originating run ID.
