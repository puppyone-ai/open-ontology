# REPORT

Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a366`. Fixed product `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` (`release-coordinator: publish one candidate through version CAS`). Parent `b4deca7a3c64df6baa49b4db12b567ade8bc247e`. Ancestor base `a13b1cc80ed66c07199a41d47b31fd42530f2be9`. Author packet a359 is the implementation proof. This packet is the independent review of that same commit.

## Verdict

`APPROVE_LIMITED` for original frozen E53 at `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3`. `verdict/VERDICT.txt` repeats that token.

The private database exercised the frozen contract on the original candidate: the seal is `oo_builder::compile_role_pack`, G1–G13 missing / not_run / stale / conflict plans stay unmoved, fence and replica conflict stays unmoved, version CAS keeps the loser failed, `applying` leaves the pointer in place and `recover` moves that candidate to `failed`, activation and the `replica_ready` / `activated` audit commit together, replay and rollback restore the previous pointer, a new process after a database restart still reports the winner, and the reader role can read only `publication_status`. A `test_issuer` row for credential, integrity, authenticated owner, customer, or runsc leaves those verified flags false. Status after restart reports `certificate_class=not_issued`.

External gates stay open. `full_sp08_accepted` stays false. The parent goal stays open. Acceptance counts were not edited. This review does not accept SP08, a public deployment, customer ownership, a standby, or a production certificate.

## What ran

Linux arm64. Rust `1.94.0 (4a4ef493e 2026-03-02)`, cargo `1.94.0 (85eff7c80 2026-01-15)`, toolchain `1.94.0-aarch64-unknown-linux-gnu`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arch arm64. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arch arm64. Both images were already local. `--pull never`.

Smoke, before the review oracle was written: `linux/smoke-before/docker-run.exit` is `0`. Console ends with the rustc and cargo lines above. Host config: `oom=false exit=0 privileged=false memory=6442450944 nano=1000000000 network=none`. The smoke container was removed.

Private PostgreSQL `oo-a366-pg` on network `oo-a366-eef831aa-net` (`b0aca1f4bb46491f251c4bfffda58bf926bbe931dd376a83d5fa04fffde6a092`). Container `6d1ecc8e41e43e6583be26b7c9b95715b050b293f6082a8e3cf7bebf6872d232`. Volume `oo-a366-eef831aa-pgdata`. Memory `1073741824`, nano cpus `1000000000`, `privileged=false`, `oom=false`. Capability drop `ALL`, add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. `pg/ports.json` is `{}`. Inspect shows `5432/tcp` with a null host binding. `pg/up.exit` is `0`. Bootstrap output is three `CREATE ROLE` lines and two `CREATE DATABASE` lines. Databases `oo_a366` and `oo_a366_author`, owner `oo_release_admin`. Login `oo_release_reader_login` is in role `oo_release_reader`. Superuser `oo_a366_admin`. SAN `DNS:oo-a366-pg`. Password file mode `600`. Public certificate hashes are in `pg/cert-public.sha256`. Secrets stayed under `/tmp/oo-a366-eef831aa-dba4-4af2-a78b-541bc9e0a366/secrets`.

`pg/pg-wrap.sh` SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` matches the a274 reviewed copy. `pg/up.sh` SHA-256 `d4742d4341e170d046bb652b425b30f43a927cc2ba2866d42f374dc62e663edd` keeps that flag pattern with a366 names. Provenance is `origin/recipes.txt`.

The immutable archive of `9d5df65` is tar SHA-256 `2ba23f2a22c4503993394e31a21c855c58c46eb6935fb4450d3c4cdadf7d4870`. `origin/source-inventory.txt` ends `mismatches=0` for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/oo-builder`, `crates/oo-core`, `crates/oo-jobs`, and `tools/release-coordinator`. The builder used a synthetic workspace `Cargo.toml` whose members are those three crates, so the sparse tree can compile. That file is harness. It is not a product edit. The review tree differs from that harness only by `#[cfg(test)] mod review_oracle;` and `src/review_oracle.rs` (`linux/build-1/review-tree.diff`). Child lock SHA-256 `19daa69f41a5aed2cdf5348e48dad389a5e5bc4ef1bca5273b31604c2b54c5ab`. `linux/build-1/lock-subset-check.txt` is `registry_checksums_subset_of_root=yes`. No `cargo update`, no `cargo generate-lockfile`, no root manifest edit.

Builder limits: 6 GiB, 1 CPU, `--cap-drop ALL`, no new privileges, no host network on the smoke, the review network only on the build and the status container, no Docker socket, `HOME=/tmp`. Clippy for 1.94.0 was added inside the builder, then `CARGO_NET_OFFLINE=true`. The final host config is `oom=false exit=0 privileged=false memory=6442450944 nano=1000000000 network=oo-a366-eef831aa-net`.

Successful build `linux/build-1/capacity.txt`: `own=0 global=1 host_tools=0`. That is inside the global cap of 10 and this task's cap of one builder. Host `cargo` / `rustc` / `rustdoc` / `rustup` count was 0.

Author baseline on the unmodified archive, database `oo_a366_author`, before the review result was accepted:

| Check | Exit |
|---|---|
| author clippy | 0 |
| author tests (13) | 0 |
| author binary build | 0 |

Independent oracle on `oo_a366`, filter `review_`, `--test-threads=1`:

| Check | Exit |
|---|---|
| review clippy | 0 |
| review tests (13 passed, 13 filtered) | 0 |
| docker run | 0 |

Console ends `REVIEW_OK` then `BUILD_OK`. Review tests finished in 7.07s. Author tests finished in 4.78s. The status binary used later is `/target/product/debug/release-coordinator` from the archive target, not the review-test copy.

Before that green run, the same 13 review tests had already passed and review Clippy had exited 101 on `record_gate` (`too_many_arguments`, 8/7). That console and those exits are in `failed-attempts/build-1-clippy-too-many-arguments/`. The helper was rebuilt as `gate_in` (6 parameters) plus `GateIn`. `oo_release_control` was then dropped on both databases (`pg/schema-reset.txt` is two `DROP SCHEMA` lines). Roles and databases stayed. The green packet is the run after that drop.

## Live result

Hand expectations are `oracle/hand.txt`. Pack file bytes are hashed in the test with `sha2`. The shop inputs match the author fixture, so the wide pack digest matches the author packet: `349b660592d39933061064221839869545e5ddaf0d40a83bbc8d84355dfcbfd1`. Manifest `45ef429ea22edc9f6278c2b3fa8038a6813845c57c009501bb2f58f2296ec5bd`. Review source hash for `rev-d` / `rv-d` is `85d3bdcf6a63f11868d40422868a92948c56392f28c771f4fa5e8dd79c960766`. The author source hash for `cand-d` / `ver-d` is `2c6b37d9df5bf94583111e0861b9ba0e85a88bc71e327a5200701e4285c7daa7`. Compiler id on the seal is `oo-builder/role-pack/v1`.

The 13 tests held:

- Wide and narrow seals differ. `product_mode_can_activate: true`, `all_accepted`, and `allaccepted` each exit 1 with empty stdout and write nothing.
- Install `fix-shop` is `installed`, the same statement replays, `other-auth` is `install_drift` with `database_used=true`, and a stage with base `nope` is `base_mismatch` with candidate count 0 and a null pointer.
- `rev-m` omits G4. Plan reason `G4_missing`. State stays `candidate_built`. Pointer stays null. G8 with caller status `fail` is still `admissible`, detail `role_pack_samples_omitted`, `production_admissible=false`.
- `rev-n` records G4 `not_run`. Plan reason `G4_not_run`. Pointer stays null.
- `rev-s` records G1 against a reconstructed source hash (`stale`), G3 against a foreign fixture hash (`foreign_receipt`), and a fence barrier `stale` (`not_fresh`). Plan reasons include `G3_foreign_receipt` and `stale_fence`. Pointer stays null.
- `rev-k` records G4 `not_run` and G4 `pass`, plus two fresh fences. Plan reasons include `G4_conflict` and `conflict`. State stays `candidate_built`.
- `rev-a` is fully armed. A checkpoint with `sessions_terminated=true` is `fabricated_session`, and `sessions_terminated_by_tool` stays false. Production plan and production activate return `production_blocked`, including `credential_missing` and `fence_not_production_admissible`. State stays `candidate_built` and the pointer stays null. Test plan reaches `gates_passed`. `enter_applying` sets `applying`, `half_published=false`, pointer still null.
- `recover` of `rev-a` sets `failed` and leaves the pointer null. A second recover replays. A later test plan returns `gates_passed`. `rev-a` is not activated.
- `rev-d` and `rev-w` both enter `applying` while the pointer is null. Commit of `rev-d` is `activated`, `certificate_class=test_scope_not_production`, `previous_version` null, `half_published=false`, with audit events `replica_ready` and `activated`. Pointer becomes `rv-d`. Commit of `rev-w` is `cas_conflict`, state `failed`, pointer stays `rv-d`, pack digests unchanged.
- A second activate of `rev-d` replays the same audit id and the same activation class.
- `rev-g` staged on base `rv-d` activates to pointer `rv-g` and leaves `rev-d` `inactive`. Rollback returns pointer `rv-d`, restores `rev-d` to `active`, leaves `rev-g` `inactive`, `certificate_class=not_issued`, both packs unchanged. A second rollback replays.
- On `rev-n`, `test_issuer` pass rows for `credential`, `integrity`, `authenticated_owner`, `customer`, and `runsc` keep those verified flags false and `certificate_class=not_issued`. Deployment claim flags stay false. Abort of `rev-k` is `aborted` and replays. Retire of `rev-s` is `retired`. Abort of active `rev-d` is `seal_conflict`. A restage of active `rev-d` with narrow visibility is `seal_conflict` and the pack digest stays. Status of `rev-d` is `active`, pointer `rv-d`, `pointer_is_candidate=true`, `certificate_class=not_issued`, `production_ready_certificate=false`, `production_blocked=true`, `half_published=false`, `test_reasons` empty.
- `oo_release_reader_login` reads `publication_status` at `rv-d` with the five claim flags false. `UPDATE` of `active_pointer` and `SELECT` of `candidate` return SQLSTATE `42501`. Status with CA bytes `not-a-certificate` returns `StoreError::Tls`.

`linux/build-1/observation.json` records `rev-d` at `rv-d`, activation class `test_scope_not_production`, status class `not_issued`, `production_ready_certificate=false`, `full_sp08_accepted=false`.

## Fresh process

`linux/restart-status.sh` restarted `oo-a366-pg` once. `pg_isready` on `oo_a366`: `/var/run/postgresql:5432 - accepting connections`. The ready loop is at most 12 sleeps of 5 seconds. A new container `oo-a366-status` ran the archive binary `status` for `rev-d` with the source hash from the observation. `--cap-drop ALL`, 6 GiB, 1 CPU, the private network, stdin attached, no Docker socket. `linux/restart-status/capacity.txt` is `own=0 global=0`. `exit.txt` is `0`. `stderr.txt` is empty. Host config: `oom=false exit=0 privileged=false memory=6442450944 nano=1000000000`. The status container was removed.

`linux/restart-status/stdout.json`:

- `outcome=status`, `state=active`, `pointer_version=rv-d`, `pointer_is_candidate=true`, `half_published=false`, `plan_scope=test_scope`, `plan_epoch=e1`, `role=oo_r_clerk`.
- `certificate_class=not_issued`, `production_ready_certificate=false`, `production_blocked=true`, `test_mode_cannot_produce_production_certificate=true`, `full_sp08_accepted=false`.
- `credential_verified`, `integrity_verified`, `owner_authenticated`, `customer_gate_verified`, `runsc_verified`, `customer_ownership_claimed`, `public_deployment_claimed`, `standby_configured`, and `sessions_terminated_by_tool` are false.
- `test_reasons` is empty. `production_reasons` lists each G gate as not production-admissible, then `credential_missing`, `integrity_missing`, `authenticated_owner_missing`, `customer_missing`, `runsc_missing`, `fence_not_production_admissible`, `replica_not_production_admissible`.
- Nine pack files. `manifest.json` sha256 matches the seal. Source hash and pack digest match the observation. Contract `oo.release-coordinator/v1`.

## One handle observation

`coordinator/wait.out` printed `OBSERVE_HANDLES` once at `2026-10-07T22:35:39Z`. `coordinator/OBSERVATION.md` is the single snapshot at `2026-10-07T22:36:09Z`. Running container then: `oo-a366-pg`. Running rust-builder count: 0. Host toolchain count: 0. Exited a064 containers and `oo-a353-node-smoke` were left in place. Frontend trees a351, a353, and a352 were not opened. No second snapshot was taken.

## Failed attempt kept

| Path | Exit | What it shows |
|---|---|---|
| `failed-attempts/build-1-clippy-too-many-arguments/review-clippy.exit` | 101 | Review helper `record_gate` had 8 arguments. Author clippy, author tests, author build, and the 13 review tests were already 0. |
| `failed-attempts/build-1-clippy-too-many-arguments/docker-run.exit` | 1 | The container exited because Clippy was 101. `oom=false`. |
