# HANDOFF — current catalog guard

Independent rereview starts at this commit. Do not treat this session as approval.

| | |
| --- | --- |
| Commit | `2254eab39c14a1902e65e72eb3674debc1c54f03` |
| Tree | `69fad83bf3c5b030f3a6eda9a05be04239de812b` |
| Parent | `9e515695d3c7e8b94936365a5f97fbc98124c345` |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a125` |

`git show --stat 2254eab39c14a1902e65e72eb3674debc1c54f03` is the six owned files only. Evidence under this directory is not part of that commit.

## Read first

1. `FINDINGS.md` — the live rate failure and the oracle measurements.
2. `REPORT.md` — commands, exits, attnums, and the SQL audit counts.
3. `catalog-rate-proposal.md` — the unwritten `catalog.rs` extension. It is not a waiver and it was not implemented.
4. `oracle/out/outcome.txt` and `oracle/out/sql-by-actor.txt` — the passing public-API run.
5. `live/out/rate-measurement.txt` — `snapshot_live` exit 101.

## Failed attempts kept

| Attempt | Container | Exit | What it shows |
| --- | --- | --- | --- |
| `lib/attempt-1` | `…-rust-lib` | 101 | `ConnPermit` is not `Debug`; `unwrap_err` did not compile |
| `lib/attempt-2` | `…-rust-lib-2` | 101 | 52 lib tests passed; clippy `collapsible_if` |
| `lib/console.log` | `…-rust-lib-3` | 0 | 52 passed, clippy 0 |
| `live/attempt-1` | none | launcher 1 | `pg_rotate_log` does not exist; `pg_rotate_logfile` does |
| `live/out` | `…-rust-live-1` | 101 | fresh open `max_rows_per_sec` |
| `oracle/attempt-1` | `…-rust-oracle-1` | 101 | missing `mut`; `sqlx::query` rejected dynamic SQL |
| `oracle/attempt-2` | `…-rust-oracle-2` | 101 | assertion asked for `transaction_timestamp` before keywords |
| `oracle/out` | `…-rust-oracle-3` | 0 | public API, 42.71 s |

Prerequisite outputs `prereq/out/outcome.txt` and `prereq/out-locks/outcome.txt` are both `result=pass`.

## Rereview bounds

The product under test is the committed tree, built with Rust 1.94.0, `--locked --offline`, from the pinned arm64 image. PostgreSQL is 17.10. The oracle binary was an overlay copied into the container test directory. It is not a crate test in the commit. `a125_oracle.rs` lives in this evidence directory.

a121 attempt 6 remains a failing record. Do not replace it with this oracle.

Still open: all 87 goals, supply, customer release, R2, R3, R4, and any R1 checkbox. This handoff does not close them.

Official web fetch of PostgreSQL documentation was blocked in this session. Version and lock behavior above come from the server that ran here (`server_version` and the prerequisite lock observations), not from a fetched manual page.

## Runtime left behind

After the evidence files were on disk, the UUID-scoped containers and the network `eef831aa-dba4-4af2-a78b-541bc9e0a125-net` were removed. `docker stop` of the Postgres container returned exit 1 (`404 page not found` from the daemon) and the container was then removed with the others. No container with this UUID remained.

These volumes were left in place and were not pruned:

- `eef831aa-dba4-4af2-a78b-541bc9e0a125-cargo`
- `eef831aa-dba4-4af2-a78b-541bc9e0a125-target`
- `eef831aa-dba4-4af2-a78b-541bc9e0a125-pglog`

Password files under `/tmp/oo-a125-eef831aa-dba4-4af2-a78b-541bc9e0a125` are removed. The evidence logs were scanned against those files before removal (0 hits) and do not contain the passwords.
