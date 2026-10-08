# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a387`. Card SP11-W01 / B08. Crate `tools/session-control-api`. This directory is the proof. The author does not accept SP11, the parent goal, or an independent review. Codex updates the ledger.

Parent at report time: `3b35c82795a0ac9a4d9c0838d93e99703a938266`. Precode smoke fixed commit: `e74ae63e8cf6991bb34f613ab88122bb21d1944d`. Precode tree: `118f19e575340b345e81ff1d7790568d2a82066a`. Product sources under `tools/session-control-api` are older than `linux/impl/test.exit` (2026-10-08 12:54:27). No product file was edited after that green run.

## What closed

`tests/live.rs` `localprocess_s1_through_s11_closes_and_restarts` passed in 28.95s inside one linux/arm64 Rust 1.94 builder. The HTTP cancel body that the test accepted has:

- `phase` `closed`
- `disposition` `closed` on the first cancel, `replay` on the second
- `deployment` `localprocess`
- `production_signed` false
- `retryable` false
- `reducer_physical_release_certified` false
- 11 steps
- `s8` confirmed with `actual_absence` true
- `s5` skipped with `actual_absence` false
- `s1` `s2` `s3` `s4` `s6` `s7` `s9` `s10` `s11` confirmed with `actual_absence` false

The same process checked, after that 200:

- the session token file was gone
- the pool directory set changed and the previous slot was absent
- `pg_stat_activity` for `application_name` `q{session}` was 0, queried as `oo_fixture_query`
- `/proc/{harness_pid}` was `NotFound`
- a second cancel of the closed conversation returned 200 `replay`
- the transcript bytes were still readable by the bound principal

A second conversation was turned, then `fixture_stage` in the test process left phase `releasing`, disposition `fixture_persisted`, retryable, and `acceptable()` false. A present `snapshot.bin` made the next HTTP cancel return 409 `snapshot_present_uncollected`. After the file was removed, HTTP cancel returned 200 `closed` with the same 11-step shape. A third conversation was closed by an in-project operator holding `session.terminate`.

Cancel before a turn returned 409 `backend_absent`, retryable, phase not `closed`.

## Binding and negatives

Current session is an operator binding plus a one-time grant stored as the SHA-256 of the secret. The cookie `oo-entry` holds the hash. Mutations require `x-oo-csrf`. Client body keys `principal_id`, `project_id`, `role`, `claims`, `claims_hash`, and `identity` are rejected. This crate does not call control-identity.

The live test also showed: a second consume of the same grant is 409; cross-person list, detail, output, and terminate are 403 and do not contain the transcript marker; shrinking the bound claim yields 403 `permission_changed` with no transcript bytes; `limit=51` is 422; a production-profile `serve` exits 2 with `runtime_unavailable` or `production_driver_absent` and does not write a listening port.

## How S1–S11 is reached

`session_release::run` performs S1, S2, S3, S4, and S8 and stops in `releasing`. This crate does not edit that reducer or that release crate. Owned adapters then record S5 (legal skip only when the snapshot is absent), S6 (gateway-token-store revoke, then lookup `Revoked`), S7 (owned proxy-mapping row present-count 0), S9 (destroyed slot plus a new empty pool directory), S10 (permit held count 0), and S11 (release audit, then `CompleteRelease`). HTTP 200 is returned only when `CloseReceipt::acceptable()` is true. A releasing receipt is 409.

S6 revoke, S7 mapping removal, and S10 permit release run before `session_release::run`. The reducer records those steps after the fixture succeeds. A crash inside `run`, before S1–S4 and S8 are confirmed, returns `unproven_resume` and does not call `run` again. A true `physical_release_certified` flag fails closed.

## Raw exits

Final builder `linux/impl/`:

| file | exit |
| --- | --- |
| version.exit | 0 |
| clippy-component.exit | 0 |
| lock.exit | 0 (`lock_present`) |
| fetch.exit | 0 |
| test.exit | 0 |
| clippy.exit | 0 |
| docker-run.exit | 0 |

`version.txt`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `image.txt`: `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` `os=linux` `arch=arm64`. `hostconfig.txt`: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a387-eef831aa-net`, CapDrop `["ALL"]`, Memory 6442450944, NanoCpus 1000000000. `container-removed.txt`: `removed`.

Unit tests 2 passed. Binary tests 0. Contract 3 passed. Lock subset 1 passed. Live 1 passed.

Preserved failures:

| archive | test | clippy | docker-run | cause |
| --- | --- | --- | --- | --- |
| attempt1 | 101 | 1 | 1 | six E0716 borrows; unused mut; clippy component absent |
| attempt2 | 0 | 101 | 1 | dead field `physical_release_certified` |
| attempt3 | 0 | 101 | 1 | `clippy::collapsible_if` in `main.rs` |
| attempt4 | 0 | 101 | 1 | `clippy::assertions_on_constants` |
| attempt5 | 0 | 101 | 1 | `clippy::single_match` in `lock_subset.rs` |

Attempts 2–5 each passed the same live test before Clippy failed. The final run is the first run with test 0 and clippy 0 together.

Precode `linux/precode/`: `version.exit` 0, `docker-run.exit` 0, fetch and locked tests of session-transition, session-store, and session-release all 0. Console records `release: 1.94.0` and `host: aarch64-unknown-linux-gnu`. The session-control-api crate was absent during that smoke.

PostgreSQL `pg/up.exit` 0. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. Container `34117f9701e4fa97d6a0188c46b70a98b26f88a591ac20564453a8ffca6350ec`, name `oo-a387-pg`, network `oo-a387-eef831aa-net`, no host port, privileged false. Certificate subject `CN=oo-a387-pg`, SAN `DNS:oo-a387-pg`. The container, its volume, the task network, the cargo volume, both target volumes, and `/tmp/oo-a387-eef831aa-dba4-4af2-a78b-541bc9e0a387` were removed after the green run. `cleanup.txt` records no remaining a387 container, volume, or network. Foreign databases `oo-a384-pg`, `oo-a389-pg`, and `oo-a373-pg` were still up. `docker prune` was not run.

## Startup

CLI argv is exactly `install`, `issue-grant`, or `serve`. Config and passwords are stdin JSON and are wiped. `serve` binds `127.0.0.1:0` only for profile `localprocess`, writes `port.txt`, and prints one listening JSON with `production_signed` false. Profile `production` does not listen. Exit 0 is install, issue, or listening. Exit 2 is a refused profile or other business refusal. Exit 1 is malformed input. Exit 3 is `commit_unknown` or connect failure. Listening is not a closed session.

Recipe parents, recorded by `shasum` at authoring time:

- impl launch `2b26de59d52c5a723c6a3f4f1577d44200629740663900a05f49847da091d6da` and inside `1926654031dc97aae17489b9b0b5f28674e591c2b0a7c090e881bab419b844a1` from a295 `linux/precode`
- pg up `4e53f23254965adbcb9197add9fb1d330e597f734733bb5028f3e429144596d9` and wrap `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` from a295 `pg`
