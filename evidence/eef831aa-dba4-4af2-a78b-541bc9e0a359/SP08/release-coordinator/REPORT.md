# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a359`. Fixed base `a13b1cc80ed66c07199a41d47b31fd42530f2be9` is an ancestor of product commit `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. Parent of that commit is `b4deca7a3c64df6baa49b4db12b567ade8bc247e`. Message: `release-coordinator: publish one candidate through version CAS`.

This packet records one private PostgreSQL proof of `tools/release-coordinator`. `full_sp08_accepted` is false. The parent goal is not accepted. No acceptance ledger or count file was edited.

## What landed

`tools/release-coordinator` is its own workspace. Contract `oo.release-coordinator/v1`. Schema `oo_release_control`. CLI `release-coordinator` with `install`, `stage`, `status`, `gate-record`, `plan`, `activate`, and `recover`.

The role pack is sealed by `oo_builder::compile_role_pack`. This crate does not contain a second compiler. The live test builds the shop catalog through `compile_captured_pg_catalog` and `compile_reviewed_effective` (`public.customer`, columns `id` and `contact`, role `oo_r_clerk`). Compiler id on the seal is `oo-builder/role-pack/v1`.

Standing flags on the stored receipts stay false: `full_sp08_accepted`, `public_deployment_claimed`, `customer_ownership_claimed`, `standby_configured`, `production_ready_certificate`, `owner_authenticated`, `credential_verified`, `integrity_verified`, `customer_gate_verified`, `runsc_verified`. `test_mode_cannot_produce_production_certificate` stays true. A successful test-scope activation receipt uses `certificate_class=test_scope_not_production`. A later status call reports `certificate_class=not_issued`. Status does not issue a certificate.

Top-level `product_mode_can_activate: true`, `all_accepted`, and `allaccepted` are malformed and write nothing. The external gate names `credential`, `integrity`, `authenticated_owner`, `customer`, and `runsc` are never synthesized as verified.

## Proof that ran

Linux arm64. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arch arm64. Database `oo_a359` on bridge `oo-a359-eef831aa-net`, container `oo-a359-pg`, no published host port. Builder 6 GiB, 1 CPU, `--cap-drop ALL`. PostgreSQL 1 GiB, 1 CPU. `RUSTUP_TOOLCHAIN=1.94.0`. `--pull never`.

This continuation found those containers already gone. It did not start another builder and did not recreate or reset the database. The exits below are the files already on disk.

Smoke, before the product proof:

- `linux/smoke/docker-run.exit` is `0`, and `linux/smoke/console.log` ends with `rustc: command not found`. That run used a login shell and is not a toolchain proof. `linux/smoke/hostconfig.txt`: `NetworkMode=none`, `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `CapDrop=["ALL"]`.
- `linux/smoke-attempt-2/docker-run.exit` is `0`. Console: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, `aarch64`. Same host limits.

Final builder, `linux/build-1/console.log`, same rustc line, `RUSTUP_TOOLCHAIN=1.94.0`:

- `rustup component add clippy --toolchain 1.94.0`, then `cargo clippy --locked --offline --all-targets -- -D warnings`, `Finished dev profile in 0.81s`.
- `cargo test --locked --offline -- --test-threads=1`: 13 passed, 0 failed, finished in 4.54s. The library tests include `private_postgres_activates_only_a_test_scope_candidate` and `compiler_seal_is_the_role_pack_and_not_a_production_certificate`.
- `cargo build --locked --offline --bin release-coordinator`, then `BUILD_OK`.
- `linux/build-1/docker-run.exit` is `0`. `linux/build-1/hostconfig.txt`: `oom=false exit=0 privileged=false memory=6442450944 cpus=1000000000`. Image id matches the pinned Rust digest, arch arm64.
- `linux/build-1/capacity-build.txt`: `own=0 global=0 host_tools=0` before that container started.
- `linux/build-1/lock-subset-check.txt`: `registry_checksums_subset_of_root=yes`, `forbidden_newer_versions=none`, path packages `oo-builder,oo-core,oo-jobs,release-coordinator`. Child lock SHA-256 `19daa69f41a5aed2cdf5348e48dad389a5e5bc4ef1bca5273b31604c2b54c5ab`. 212 package names. `cargo generate-lockfile` was not the accepted lock. Root `Cargo.toml` and root `Cargo.lock` are unchanged in the product commit.

`aws-lc-sys v0.45.0` and `aws-lc-rs v1.18.1` compiled in the earlier failed runs `console-compile-1.log` and `console-clippy-9.log`. The final console reused that target volume and does not reprint those compile lines.

Private PostgreSQL, one serial test against the compiler-sealed pack:

- Install of authority `fix-shop` returns `installed`, the same statement replays, and `other-auth` returns `install_drift` with `database_used=true`.
- A stage whose base is `nope` while the pointer is empty returns `base_mismatch` and writes no candidate.
- Candidates A, B, C, D, and F are sealed while the pointer is null. A and D have different source hashes.
- G8 is a component-authority recompile. The stored disposition is `admissible` with detail `role_pack_samples_omitted`, `pii_database_scan=not_performed`, and `production_admissible=false`. Caller status `not_run` or `fail` does not decide G8.
- G9 is the installed fixture authority hash. The other G gates in this test are explicit `test_issuer` reports. A second record of G1 replays the same certificate hash.
- A checkpoint with `sessions_terminated=true` is stored as `fabricated_session`. `sessions_terminated_by_tool` stays false.
- Production plan and production activate on the fully test-gated candidate A return `production_blocked`, including `credential_missing` and `fence_not_production_admissible`. The candidate stays `candidate_built` and the pointer stays null.
- Test-scope plan moves A to `gates_passed`. `enter_applying` then leaves the pointer null and `half_published=false`. `recover` moves A to `failed` and replays. A second plan returns `gates_passed`.
- Candidate B: omitted G4 plans as `G4_missing`; a `not_run` G4 plans as `G4_not_run`; a second passing G4 beside that row plans as `G4_conflict`; two fresh fences plan as `conflict`.
- Candidate C: a fixture certificate with the wrong authority hash is `foreign_receipt` on G3, a barrier `stale` fence is `not_fresh`, and the plan reasons include `G3_foreign_receipt` and `stale_fence`.
- Candidate F records G8, then a restage with visibility columns limited to `contact` changes `source_hash` and `pack_digest`, returns to `candidate_built`, and reports `G8_missing`. A later `test_issuer` row named `credential` keeps `credential_verified=false`.
- D is planned at expected version null. A and D both enter `applying` with the pointer still null. Commit of D activates it: `certificate_class=test_scope_not_production`, `previous_version` null, `half_published=false`. Commit of A returns `cas_conflict`, state `failed`, pointer `ver-d`. A second activate of D replays the same `audit_id`. Status of D does not list `version_mismatch`.
- G is staged on base `ver-d`, activated, and D becomes `inactive` with its pack digest unchanged. Rollback of G returns the pointer to `ver-d`, restores D to `active`, leaves G `inactive`, and keeps both pack digests. A repeat rollback replays.
- Abort of B replays as `aborted`. Retire of C returns `retired`. Abort of active D and a new-hash restage of active D both return `seal_conflict`. D stays `active` with the same pack digest.
- Login `oo_release_reader_login` reads `publication_status` at version `ver-d` with the five claim flags false. `UPDATE` on `active_pointer` and `SELECT` on `candidate` return SQLSTATE `42501`.
- A fresh connection and the CLI `status` both report D active at `ver-d` with `production_ready_certificate=false`.
- Status with CA bytes `not-a-certificate` returns `StoreError::Tls`.
- The test wrote `observation.json` for candidate `cand-d`: `certificate_class=test_scope_not_production`, `production_ready_certificate=false`, `pointer_version=ver-d`, manifest `45ef429ea22edc9f6278c2b3fa8038a6813845c57c009501bb2f58f2296ec5bd`, pack `349b660592d39933061064221839869545e5ddaf0d40a83bbc8d84355dfcbfd1`, source `2c6b37d9df5bf94583111e0861b9ba0e85a88bc71e327a5200701e4285c7daa7`.

Host restart, after that test, is `linux/restart-status/`. `docker restart oo-a359-pg` then `pg_isready` on `oo_a359`: `/var/run/postgresql:5432 - accepting connections`. Inspect: `status=running oom=false started=2026-10-07T21:52:25.109048134Z`. The status container used `--cap-drop ALL`, `--memory 6g`, `--cpus 1`, network `oo-a359-eef831aa-net`, and stdin attached. It did not receive the Docker socket. `linux/restart-status/exit.txt` is `0`. `stderr.txt` is empty. `hostconfig.txt`: `oom=false exit=0 privileged=false`.

`linux/restart-status/stdout.json` is that CLI status of `cand-d`:

- `outcome=status`, `state=active`, `pointer_version=ver-d`, `pointer_is_candidate=true`, `half_published=false`.
- `certificate_class=not_issued`, `production_ready_certificate=false`, `production_blocked=true`.
- `test_reasons` is empty. `plan_scope=test_scope`, `plan_epoch=1`, `role=oo_r_clerk`, `database_used=true`.
- Gates G1, G2, G3, G4, G5a, G5b, G6, G7, G8, G9, G10, G11, G12, G13 are `admissible`, `test_admissible=true`, `production_admissible=false`.
- `production_reasons` has 21 entries: each `G*_not_production_admissible`, plus `credential_missing`, `integrity_missing`, `authenticated_owner_missing`, `customer_missing`, `runsc_missing`, `fence_not_production_admissible`, `replica_not_production_admissible`.
- Nine pack files, including `manifest.json` at the manifest hash above.
- `sessions_terminated_by_tool=false`. Contract `oo.release-coordinator/v1`.

PostgreSQL before removal: container `964da70de236d9ca1f971ee3d29119a9a8bb3b8e928a175026ed84b40438f39b`, image digest as pinned, `memory=1073741824`, `cpus=1000000000`, `privileged=false`, `oom=false`. Capability drop `ALL`, add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Ports `{}`. Network `a1e3aa5070e5128ea1a42d6c96463d9066952f903d519247e160a2843de99db6`. Volume name `oo-a359-eef831aa-pgdata`. `pg/up.exit` is `0`. Bootstrap: `CREATE ROLE` reader, reader_login, admin, and `CREATE DATABASE oo_a359`. Public certificate hashes are in `pg/cert-public.sha256`. Server SAN is `DNS:oo-a359-pg`. `pg-wrap.sh` origin commit `b91b67bee0f603725b2cb7cc9c284588a2699392`, path `deploy/trusted-demo/pg/pg-wrap.sh`, SHA-256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`.

## Failed attempts kept

| File | Exit | What it shows |
| --- | --- | --- |
| `linux/smoke/console.log` | 0 | Login shell dropped `PATH`. `rustc` was not found. |
| `linux/build-1/console-fetch-locked-refused.log` |  | `cargo fetch --locked` refused to update a lock that was not yet fetchable. |
| `linux/build-1/console-lock-rejected.log` |  | `cargo generate-lockfile` locked 211 packages to newer compatible versions. That lock was rejected. Copies: `Cargo.lock.rejected`, `Cargo.lock.fresh`. |
| `linux/build-1/console-compile-1.log` | 101 | `query_error` borrowed `tokio_postgres::Error`. 27 compile errors. `aws-lc-sys` compiled in this run. |
| `linux/build-1/console-test-1.log` |  | 10 passed, 3 failed. `argv` returned `extra_arg` where the test expected `config`. The seal returned `manifest_absent`. Install drift had `database_used` null. |
| `linux/build-1/console-clippy-missing.log` |  | `cargo-clippy` was absent for `1.94.0-aarch64-unknown-linux-gnu`. |
| `linux/build-1/console-clippy-9.log` | 101 | Nine `-D warnings` errors, then `aws-lc-rs` was already compiled. |
| `linux/build-1/console-test-2.log` | 101 | `enter_applying` while A was still `candidate_built` returned `not_ready`. Pointer stayed null. |
| `linux/build-1/console-tests-passed.log` | 1 | 13 passed in 4.82s. The script then failed `test -x` because `cargo test` had not built the package binary. |
| `linux/build-bin/console.log` | 0 | Binary-only rebuild. Ends `BIN_OK`. This is not the final clippy-plus-test proof. |
| `linux/restart-status/exit-attempt-1.txt` | 1 | `release-coordinator: malformed json`. `docker run` had no `-i`, so stdin was empty. Postgres was already accepting connections. |
| `linux/restart-status/stdout-before-self-pointer.json` | 0 | Status after restart, before the pointer label fix. State `active`, pointer `ver-d`, `test_reasons` contained `version_mismatch`. |

Schema drops, only `oo_release_control` in `oo_a359`, are `pg/schema-reset-after-test-1.txt`, `pg/schema-reset-after-test-2.txt`, and `pg/schema-reset-after-status-1.txt`. Each one was so the next install assertion could see a fresh schema. The final green test and the final status ran after the third drop. The first bootstrap attempt is `pg/bootstrap-attempt-1.txt`: role `postgres` does not exist. The successful bootstrap used `-U oo_fixture_admin`.

## Resources after the proof

Passwords and private keys stayed in `/tmp/oo-a359-eef831aa-dba4-4af2-a78b-541bc9e0a359/secrets` and are not in this tree. A scan of this evidence path found no `BEGIN PRIVATE` block and no `*.pem`, `*.key`, or `*.pass` file. `status-config.json` names `password_file` and `ca_file`. It does not contain the password or the certificate bytes.

Owned cleanup is `CLEANUP.md`. The database volume is gone, so the activated rows are not still queryable. The durable record of the final state is `linux/build-1/console.log`, `observation.json`, and `linux/restart-status/stdout.json`.
