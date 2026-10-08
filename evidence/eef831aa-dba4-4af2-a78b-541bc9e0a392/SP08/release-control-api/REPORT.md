# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a392`. This packet is the private PostgreSQL proof of `tools/release-control-api`. `full_sp08_accepted` is false. SP08 is not accepted. The parent goal is not accepted. No ledger, plan checkbox, frontend file, root `Cargo.toml`, or root `Cargo.lock` was edited.

The proof container started at git `f30d06bef4e1f186481587899cbf9b4bd4145c1a`. At that start, `git diff` against producer `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` for `tools/release-coordinator` was empty, and `git diff` against identity `a6f92da8a0bf872f8a30efb116c2c7b14463505b` for `tools/control-identity` was empty. Commits through `b35ae9c5aae756e144a20499a9edf70a875fc11d` do not change those two trees or `crates/oo-builder`.

## What landed

`tools/release-control-api` is its own workspace. Contract `oo.release-control-api/v1`. Schema `oo_release_api`. Commands `install` and `serve`. The service calls `release-coordinator` at the pinned tree for stage, gate, checkpoint, plan, activate, recover, and status. It does not reimplement pointer CAS.

Publish is HTTP 202, then a worker. `getActiveRelease` reads `oo_release_control.publication_status` as `oo_release_reader`. `getActiveRolePack` returns paths, sha256, and byte sizes for the active role. Rollback calls producer `recover` action `rollback` only after the restored visibility is a reviewed subset of the active version. A failed or missing gate does not move the active pointer.

OpenAPI, the six schema files, `CONTRACT.md`, and `migrations/oo_release_api_v1.sql` were already frozen when the green run compiled. This session did not weaken them.

## Proof that ran

Linux arm64. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Database `oo_a392`. Bridge `oo-a392-eef831aa-net`. Container `oo-a392-pg`, no published host port. Builder 6 GiB, 1 CPU, `--cap-drop ALL`. PostgreSQL 1 GiB, 1 CPU. `RUSTUP_TOOLCHAIN=1.94.0`. `--pull never`.

Smoke, before product edits, is `linux/smoke/console.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. The container `oo-a392-smoke-pre` was already removed. It was not run again. See `linux/smoke/note.txt`.

Final builder, `linux/build-1/console.log`:

- `cargo clippy --locked --offline --all-targets -- -D warnings`, `Finished dev profile in 1.72s`.
- `cargo test --locked --offline -- --test-threads=1`: library 4 passed, contract 5 passed, live 1 passed in 9.47s, binary tests 0, doc-tests 0. 10 passed, 0 failed.
- The live test is `private_postgres_publish_and_rollback`.
- `linux/build-1/docker-run.exit` is `0`. Console ends `BUILD_OK`.
- `linux/build-1/hostconfig.txt`: `oom=false exit=0 privileged=false memory=6442450944 cpus=1000000000`.
- `linux/build-1/capacity-build.txt`: `own=0 global=2 rust_images=2 host_tools=0` before that container started.
- The inside script refuses a child lock checksum that is absent from the root lock. The green run reached `cargo fetch --locked` and the tests, so that check passed. `cargo generate-lockfile` ran on the first builder, when the child lock did not yet exist. Later runs kept that lock. Root manifests were not edited.

`linux/proof/private-pg.json` is the live test's step list. `production_ready` is false. `full_sp08_accepted` is false. `customer_approval` is `not_issued`. `test_scope` is `not_production`. Steps, in order:

- install `installed`.
- active pointer null. Pack read is 409 `inactive`.
- no cookie 401 `unauthenticated`. Duplicate key and waiver key 400 `malformed`. No operation row was added.
- bad origin 403 `origin.rejected`. Bad CSRF 403 `csrf.rejected`. Wrong revision 409 `revision.conflict`.
- missing operation 403 `forbidden` `scope`. Unknown route 404. `POST /control/v1/releases/active` 405.
- reviewer rollback 403 `capability`. Cross-project current 403 `scope`. Business owner publish 403 `capability`.
- omitted G7 refused, reason `not_ready`, pointer null.
- fabricated fence (`sessions_terminated=true`) refused, reason `not_ready`, pointer null. The test required gate token `stale_fence`.
- production scope with `test_issuer` customer, no owner store, and no runsc store refused, reason `production_blocked`, class `not_issued`, pointer null. The test required `authenticated_owner_missing`, `runsc_missing`, `customer_not_production_admissible`, `fence_not_production_admissible`, and `replica_not_production_admissible`.
- test publish `ver-n` succeeded, class `test_scope_not_production`, outcome `activated`.
- same idempotency key replayed 202 with `replay=true` after the pointer had moved. A different body on that key is 409 `idempotency.conflict`.
- a fresh key with a null base while `ver-n` is active is 409 `pointer`. No new row.
- production candidate based on `ver-n` refused. Pointer stayed `ver-n`.
- test publish `ver-w` from `ver-n` succeeded.
- reader view and auditor pack are the active wide version, class `test_scope_not_production`. Reviewer pack is 403 `capability`. The audit role is 403 `role`. The test also required API-admin `SELECT` of `publication_status` to fail with SQLSTATE `42501`.
- rollback of `ver-w` succeeded, outcome `rolled_back`, class `not_issued`, pointer `ver-n`. The old publish stayed `succeeded`. The test required producer state `cand-n` active, `cand-w` inactive, and producer audit event `rolled_back`.
- a later narrow publish `ver-n2` on top of `ver-w2` succeeded. Rollback of `ver-n2` is 409 `compat.refused` `permission` and adds no row. Pointer stayed `ver-n2`.
- a restarted serve marked a running row with no candidate as `resume_uncertain` and left the pointer at `ver-n2`.
- a queued row inserted while serve was stopped completed after restart. Pointer became `ver-restart`, class `test_scope_not_production`. The test required an API-admin `UPDATE` of `audit_event` to fail with `42501`.

PostgreSQL before removal: container `ee3ac54513273aa8ed462b904c2a0d2ea2e574f0b8a90c4be8f041750bce309d`, `pg/up.exit` `0`, `memory=1073741824`, `cpus=1000000000`, `privileged=false`, `oom=false`. Ports `{}`. Capability drop `ALL`, add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. `pg-wrap.sh` content hash `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. Bootstrap stdout is the nine command tags only. Password scan of that stdout and stderr passed inside the harness.

## Failed attempts kept

| File | Exit | What it shows |
| --- | --- | --- |
| `linux/failed-attempts/build-1.console.log` | 101 | `boot(self: &Self)` and `const fn` `PartialEq`. |
| `linux/failed-attempts/build-2.console.log` | 101 | unused `detail_ok`, `from_columns` 8/7, `insert_operation` 22/7. |
| `linux/failed-attempts/build-3.console.log` | 101 | `serde_json::Serialize` is not in `serde_json`. |
| `linux/failed-attempts/build-4.console.log` | 101 | missing `reply_from`, `Idp::start`, `forbid` expected `String`. |
| `linux/failed-attempts/build-5.console.log` | 101 | unused issuer `Mode` variants, `collapsible_if`, `bool_assert_comparison`. |
| `linux/failed-attempts/build-6.console.log` | 101 | `expect_err` on `Prepared` without `Debug`. |
