# Handoff — a116 snapshot owner R1 review

**Decision: APPROVE LIMITED R1.**

The coordinator owns the plan checkboxes. This review does not mark SP03-W03, R2, R3, R4, the bounded reader, supply, license, customer, owner, protected CI, runsc, or release.

The live oracle remains `oracle_result fail` with `fail_count=8`. The eight names are listed in `FINDINGS.md` and stay failed. `REPORT.md` gives the production locations and the SQL-log times.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a116` |
| Role | independent reviewer, Grok 4.7 xhigh, no subagents |
| Reviewed commit | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Tree | `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` |
| Parent | `e5a066622e0409762fe44f11c4d303d2468a3290` |
| Slot | A, released from a112 handle 50371 |
| Slot B | a114 handle 82080, unused |
| a115 handle 82672 | no builder grant |
| Archive sha256 | `1c9304da2ce6dd9524cdc19b1487fcc8ed2ccb453ed287dd77b4739162ed2bae` |
| Captured workspace tip | `a26e7d0344cd6ef2b4c40d2f33311a86f7327050` at 2026-10-02T22:01:34Z |
| Workspace tip at commit | `3a06e6042de6dda92fd1c7e6b011fef002aeca56` |

This file is inside the evidence commit, so it cannot name that commit's own SHA. The workspace tip above is the parent this evidence commit sits on.

## What the coordinator can treat as measured

- Focused smoke: lib 49 passed, exit 0, Rust 1.94.0, linux/arm64. Package tests not-run.
- Attempt 4: compile 0, Clippy 0, oracle process exit 101, outcome file complete.
- Public owner API against one private PostgreSQL 17.10, verify-full, caller CA, `application_name=oo-pg-source`.
- Backend release observed by `(pid, backend_start::text)` for finish, abort, drop, admission failure, read failure, deadline, and owned termination.
- Nonempty SQL audit. Zero classified oo-pg-source DML, DDL, and `COMMIT`.
- Author a112 counts, catalog hash, and row labels were not used as this proof.

## What stays open

- The eight oracle marks. A successor who wants those strings recorded has to use a new database. This cut was mutated by the writer and by `ADD COLUMN a116_extra`.
- `identity_drift` and `nested_transaction` were read in source and not executed.
- Package `tests/live_pg.rs`, `tests/lossless_live.rs`, and `tests/snapshot_live.rs`.
- a110 supply `request_changes`. This review did not rerun `cargo deny`.
- R2 streaming, R3 cancellation, and R4 combined apply.
- Drop observed the backend row gone. The contract still says Drop is not a confirmed rollback and is not the R3 cancel proof.

## Attempts

| Attempt | Container | Result |
| --- | --- | --- |
| 1 | postgres, bind | canary was 3 because triggers ran on the seed; reset to 0 before the audit marker |
| 2 | `rust-review` exit 1 | probe compile failed; owner not opened |
| 3 | `rust-review-2` exit 86 | probe Clippy `needless_borrow`; oracle skipped |
| 4 | `rust-review-3` exit 101 | compile 0, Clippy 0, oracle recorded `fail_count=8` |
| smoke | `rust-smoke` exit 0 | lib 49 passed |

`live/phases/lib.exit` and `live/phases/fetch.exit` are smoke files left on the reused target volume. The live console says `lib_retest=not-run`.

## Cleanup

At the time this handoff was written, the five own containers were still present: smoke exited 0, review exited 1, review-2 exited 86, review-3 exited 101, and postgres was running. `live/cleanup.sh` removes only names that contain this session UUID, then the three own volumes, the own network, and `/tmp/oo-a116-<uuid>`. Run it after this evidence commit. Foreign containers, including a064, stay.

## Stop

No push, no pull request, no release, no customer action, no plan edit.
