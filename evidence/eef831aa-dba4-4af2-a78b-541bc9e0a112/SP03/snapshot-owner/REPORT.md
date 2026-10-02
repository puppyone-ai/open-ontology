# REPORT — V01-PG-02-R1 snapshot owner

Session `eef831aa-dba4-4af2-a78b-541bc9e0a112`. Slot A. Grok 4.7 xhigh. No subagents and no worktree.

This is R1 only. R2 (bounded Arrow batch stream and budgets), R3 (key planning, cancellation, and whole-round failure), and R4 (full reader acceptance) are unsigned. This is not a bounded reader. Nothing here is signed. No push, pull request, or release.

## Baseline and commit parent

Frozen overlay base, used for the smoke and for the R1 compile tree:

- commit `170dffba8375ca3631df0cc52fdbe65706b702a0`
- tree `f090485c9fedc83d4b34af3eda3fc5732bde597e`
- parent `8b853d21bd66e5499baea07e8d3bb97232492d0c`
- archive digest `23e7fcf7316ef1ffed9887a88a8f74d9dc8a9e8fcb394aafbba8c60c4a4ecec3` (`smoke/snapshot.sha256`)

The delivery commit's parent is whatever `git rev-parse HEAD^` prints after this commit. At authoring time that parent was `e5a066622e0409762fe44f11c4d303d2468a3290`, tree `5071b838764780759ee1ed016417b6d37b0bc9f8`, subject `coordination: record read-only dependency analysis handle`. `170dffba` is an ancestor. Committed `oo-connectors` product files did not change between `170dffba` and that parent. The overlay was the frozen archive plus the owned patch, not `git archive HEAD`.

`config.rs` is unchanged. No root manifest, lock, or vendor change. `catalog.rs` and `access.rs` are unchanged.

## Handles at resume

Coordinator tool handle 5564 was absent. a107 handle 80154 was absent. Host `cargo` / `rustc` / `rustdoc` / `rustup` / `clippy-driver` were absent. The only own container was `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-base`, already exited 101. No PostgreSQL container and no running rust builder. Missing handles are not a test pass.

## What shipped

`SnapshotOwner` (`oo.source.pgsql.snapshot/v1`) takes an admitted `PgSourceConfig`, an `AdmittedCatalog`, the config and type-policy identities, a `PgTypePolicy`, a generation, a deadline, and the expected principal, database, recovery flag, and `pg_postmaster_start_time()::text`.

`open` refuses generation 0, a past deadline, and an identity mismatch before it reserves the generation. It then connects once through `PgConnector`, checks the existing version, TLS, startup-role, and principal boundary, and runs one `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`. `PgSession::catalog` is not that transaction. `collect_catalog` runs inside the owner's transaction. The owner re-reads relation, attribute, privilege, type, and backend facts before it returns.

`read_probe` is one primary-key lookup with bound values and `LIMIT 2`. Zero rows is an empty `TypedBatch`. Two rows is `probe_key`. Decoding uses the existing `decode_payload` and `assemble_batch`. The public API does not return the connection or accept caller SQL.

`finish` returns `SnapshotFinished` only after `ROLLBACK` is accepted, close succeeds, the owner is not poisoned, and the deadline has not passed. `abort` returns `SnapshotAborted` on that same cleanup. `Drop` spawns rollback and close when a Tokio runtime is current and does not return either type. Cleanup failure is never `SnapshotFinished`.

`types/mod.rs` re-exports `decode_payload` and `guard_plans` as `pub(crate)` so the owner can call them. Mapping and decode semantics are unchanged. That file is part of this commit.

## Linux 1.94 proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. `RUSTUP_TOOLCHAIN=1.94.0`. Clippy was installed in the container (`airgap_claim=no`). Final builder `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-r1` id `d0dec4c423a3b7d8d31e6c041f6c32330a9611f0a4d7e545afc8c11089e0cc8e`, exit 0, oom false, privileged false, bridge network, no published ports, cap-drop ALL.

| Phase | Exit |
| --- | --- |
| clippy component | 0 |
| `cargo test --locked --offline -p oo-connectors --lib --test snapshot_live -- --test-threads=1` | 0 |
| lib tests | 49 passed, 0 failed |
| `snapshot_owner_live` | 1 passed |
| `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` | 0 |
| worst | 0 |
| package `tests/live_pg.rs` | not-run |
| package `tests/lossless_live.rs` | not-run |

The focused command does not run `tests/live_pg.rs`. That test still panics when `OO_PG_FIXTURE` is absent. `lossless_live` stays ignored.

Frozen smoke, before the product edits, remains exit 101. Lib 45 passed, clippy 0, fetch 0. The package test panicked because `OO_PG_FIXTURE` was absent. That result stays not-run, not green. Container `47a29b7603581c83a46cb3087f663df08b9d62bc15a381410975ed627d9f4c64`.

## PostgreSQL 17.10

Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, created 2026-08-05, `linux/arm64`. Container `eef831aa-dba4-4af2-a78b-541bc9e0a112-pg` id `4e85771679bba1f967f0794fddf49ef161417524b482505f79700058c1332b3a`. Not privileged. No host port. Cap-drop ALL, with `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID` added so the official entrypoint can start. verify-full, caller CA, alias `oo-snap-pg`, database `snapdb`.

Tables `snap.parent` and `snap.child`, plus trigger counter `snap.touch`. Function `snap.bump()` is not security definer. The writer needed `SELECT` and `UPDATE` on `snap.touch` because the trigger reads `n` and `id`. Attempt 3 failed before that grant (`42501` on `touch`). The grant is in `live/init.sql`. The passing run is after it.

Measured outcomes from `live/phases/outcome.txt`:

- Reader `inspect_access` allowed registration. Catalog hash `1ff301767c85e585b52df42ae0ab47d1cbfe6c15205b6a0fcef98605badd8236`. `type_conversion` stayed unsupported.
- GUC role: `startup_options`.
- While the owner was between statements, admin saw `idle in transaction`, `xact_start` set, `application_name=oo-pg-source`, user `oo_snap_reader`, matching `(pid, backend_start::text)`.
- Deadline after an idle wait: probe and finish returned `deadline`. Backend row gone within 5000 ms.
- Abort consumed the generation. Reuse returned `generation_reused`.
- Drop released the backend. The test records `drop_released`. Drop is not `SnapshotFinished`.
- `pg_terminate_backend` logged `FATAL: terminating connection due to administrator command`. The owner returned fact `database` through the existing driver mapper and did not return `SnapshotFinished`. Backend row gone within 5000 ms.
- OO-only window left `n_tup_ins`, `n_tup_upd`, `n_tup_del`, and `snap.touch.n` unchanged (`touch=0` across that window).
- Same cut: owner and a separate repeatable-read oracle both still read parent `alpha`, child 10 qty `2`, and child 12 absent after the fixture writer committed `beta`, qty `9`, and child 12 qty `4`. Canonical hashes of the two owner probes matched. A new generation read `beta`, qty `9`, and qty `4`, with the same schema hash. Writer trigger delta was `touch=3` (`ins=1 upd=5 del=0` on the stat counters for that window).
- `ALTER TABLE snap.parent ADD COLUMN note text` while the owner was open: the probe still returned the old shape, label `beta`, and `finish` was `SnapshotFinished`. Recorded `schema_inflight=old_cut`. The next generation opened with the old admitted shape returned `catalog_identity`.
- `REVOKE SELECT ON snap.parent` while a later owner was open: the probe failed with fact `insufficient_privilege` (SQLSTATE `42501` through the existing mapper) and `finish` was not `SnapshotFinished`. Recorded `privilege_inflight=insufficient_privilege`. The next generation, opened with the updated admitted shape, returned `select_not_granted`. This run does not claim that MVCC freezes ACL checks.

## SQL audit

`live/postgresql.log` sha256 `8000584abe0ebc201c19c5ac1de8715823152f812965ae82f786106466f93bbc`. Password scan against the four fixture password files found none. The file also contains attempt 3's earlier statements, including the touch permission error and one terminate. The passing test audited that combined file.

Reader `oo-pg-source` / `oo_snap_reader` verb lines: 864. Histogram: `SELECT` 805, `START` 29, `ROLLBACK` 26, `WITH` 4. No write verb. Writer `oo-fixture-writer`: `UPDATE` 3, `INSERT` 1. `START` appears before the parent probe in the test's own scan.

## Failed attempts retained

| Attempt | Result |
| --- | --- |
| 1 | `E0502` in `snapshot.rs` probe borrow. Builder `afb86609fc821c91a45d39642f8a9589e1dc350dda5c786b30cc91e584debe9f` exited 101. Log in `live/attempt1/`. |
| 2 | Integration-test borrows and `audit().await`, plus clippy `manual_non_exhaustive`. Exited 101. Log in `live/attempt2/`. |
| 3 | Lib 49 passed. Live test panicked: writer trigger could not `UPDATE snap.touch` (`42501`). Clippy `collapsible_if`. Exited 101. Log in `live/attempt3/`. |

Those containers were removed after the logs were copied so the next run could reuse the name. The exits stay 101.

## File hashes at authoring

```text
86361bc3254edfdc67f4fa70e2a304025da2298ecc76c86b71dd7e65b846e088  crates/oo-connectors/src/snapshot.rs
b43dc30e4b791c0c5677906b41ce2c2714d615df701dbc1b589027881fb20bb7  crates/oo-connectors/tests/snapshot_live.rs
86fa06414a49e6b97425651f0ffc750bc634fe5423d1af70216b18da03be4b25  crates/oo-connectors/src/types/mod.rs
70c16cea3aa12af9d09981714816f03522dcfdba4855116892c271d58c535e10  crates/oo-connectors/src/session.rs
9434b059e24dd7ae9f113fa2e7572ed78f274144821d7fa86345e6e638e227ae  crates/oo-connectors/src/sql.rs
49b19e22888247ce40e5972ccdd95136a6e6af9757fe86fae87b30b1699a4896  crates/oo-connectors/src/error.rs
460ae2460ace386f54a715a626de5a73d1ba9a9104aa6cf656ff5dfab0abab06  crates/oo-connectors/src/lib.rs
0b555e0c6b0a908a8d4492350a9a0b7c71c2ac1a94981ac5b8f7f5d5cb6a163d  crates/oo-connectors/README.md
095dd3b364d8284ec4a665a9125bfe0bfdff3b62736d461ae61a4fb271e571e0  contracts/source/pgsql/snapshot-owner-v1.md
```

## Not claimed

R2, R3, and R4. A full batch stream. In-flight cancellation beyond this owner's own finish, abort, drop, deadline, and terminate observations. A cluster generation lease. Frozen ACL checks. Supply approval of the types crate. Package `OO_PG_FIXTURE` or `OO_PG_TYPES_FIXTURE` runs. A signature on this commit.
