# REPORT

Verdict: **APPROVE_LIMITED**

Original E52-D1/1, the durable review-ledger seam in `tools/review-ledger` at product commit `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. Parent `a08b3576969cf06c097a4769bce8f9a4758a81a2`. Message: `review-ledger: refuse an auditor batch before any row`.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a367`. This packet is the independent review. The author packet is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a358/SP07/review-ledger`. That packet was read, including `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md` for the same commit. The author did not accept E52.

Public management RBAC is not accepted. Authenticated HTTP is not accepted. Git curated apply is not accepted. Full S9 and S10 are not accepted. SP07-W05, the parent goal, and the acceptance counts are not accepted. No plan line, ledger, or count file was edited. No product file was edited.

HEAD at the time of this report is `feffec29b4c036eb8a3628c105e93ee15278e734`. The reviewed commit is an ancestor. `tools/review-ledger/src/store.rs` in the worktree is still blob `2bb8322d753211e3f5d5c083c0a1fc1760104a7a`. `tools/description-pack/src/lib.rs` at HEAD is `1d70bd086b8971e5a3bf43816d548b9df9550f62`. The reviewed archive has `69930324e1a3faa6347eaffc76ed5000966dc348`. The builder compiled the archive. Later combined-pack edits are outside this verdict.

## What this verdict covers

A private PostgreSQL 17.10 database holds the candidate queue, one decision, its revision, and its idempotency key together with one immutable audit row in the same transaction. A conflicting audit insert returns `audit.rollback` and leaves the decision unstored.

Scope is an exact object id. A dataset scope does not cover another object. A stale input hash, a wrong expected revision, a reviewer label that does not match the control file, and an owner certification from a reviewer are refused with no new decision row. The same idempotency key cannot store a second choice.

An auditor `batch` returns `role.forbidden` after the control file is read and before `connect`. With `track_counts=on`, `n_tup_ins`, `n_tup_upd`, and `n_tup_del` for schema `oo_review_ledger` stayed at the pre-call snapshot, and the row counts stayed at that snapshot. Batch id `baudit` and decision key `auditor.batch` are absent.

A description-pack receipt is stored only after the archived `description_pack::apply_description_pack` returns `packed` or `reviewed_unchanged` with authentication `not_claimed`, publication `unavailable`, and grants `not_authorized`. The stored row holds the command SHA-256, the decision candidate SHA-256, and the compiler-document SHA-256. `git_applied`, `published`, and `curated_files_written` are false. The decision row is not updated. A command file `{` is `compiler.refused` and writes no receipt.

Every inspected result keeps `authenticated=false` and `grants=not_authorized`. A control file with `authority=verified` or `management_identity=claimed` is `authority.not_verified`. Role `admin` is `control.role`. Request keys `grant`, `role`, `publish`, `authenticated`, and `auto_accept` are `not_grant` before a decision is parsed. Those checks ran before `install`.

## Archive

`git archive` of `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` for `Cargo.toml`, `rust-toolchain.toml`, `crates/oo-builder`, `crates/oo-core`, `crates/oo-jobs`, `tools/review-ledger`, `tools/description-pack`, and `tools/description-review`. Archive SHA-256 `e6b61a984fedff902ecfc8ae8036ec983e12e7a3c718daf44ad2db1568f51a86`. `origin/source-inventory.txt` has 343 `match` lines and `mismatches=0`. Each path’s git blob equals `git hash-object` of the archived file. The live repository was not mounted.

The build tree received a shim workspace `Cargo.toml` for `oo-builder`, `oo-core`, and `oo-jobs`. `rust-toolchain.toml` was inventoried and was not copied into the build tree. `tools/review-ledger` keeps its own workspace. `Cargo.lock` SHA-256 `476abb33b767a16b395756c05172d7ba39c8ac2b64f4f6d284b026de1663877d`, 50228 bytes. Migration SHA-256 `75e79573642a90b0b3d8f7704b5b080704656f7d9494b056162b9bc0dd63843c`. Commands used `--locked`. `cargo update` and `cargo generate-lockfile` were not run. The root manifest was not edited.

`linux/author/tree-before.txt`, `tree-after-test.txt`, and `linux/oracle/tree-after-oracle.txt` share SHA-256 `e7c014d58335d08f169d969109d150430c427d4b4ace4c98279638dab1149738`. The oracle source was copied into the build tree only for its own phase, then deleted. Product Clippy ran while `tests/oracle_a367.rs` was absent.

## Smoke, before the oracle and before the author rerun

`linux/smoke-before/docker-run.exit` is `0`. Console: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, `aarch64`, toolchain `1.94.0-aarch64-unknown-linux-gnu (overridden by environment variable RUSTUP_TOOLCHAIN)`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `arch=arm64`. Host config: `oom=false exit=0 privileged=false memory=6442450944 nano=1000000000 network=none`. Capacity at launch: `own=0 global=1`. The foreign builder was left running. Container `oo-a367-smoke` was removed by the smoke script.

## Private database

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `os=linux arch=arm64`. `pg/up.exit` is `0`. Server line: `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`, accepting connections. Container `oo-a367-pg` id `e8b2c7b3a6b0211740544626e15e291eafe8504a3e818680577c873ea11bfab3`, created `2026-10-07T22:41:34Z`, started `2026-10-07T22:44:52Z` after the proof restart. Labels `oo.owner=a367`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a367`, `oo.kind=postgres`. Inspect: `oom=false privileged=false network=oo-a367-eef831aa-net memory=1073741824 nano=1000000000`. `pg/ports.json` is `{}`. Caps added `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, and `CapDrop` is `ALL`.

`pg/pg-wrap.sh` SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`, the a274 reviewed bytes named in `origin/recipes.txt`. SAN text is `DNS:pg.a367.internal`. CA certificate SHA-256 `5f81fc9f35adb5083e6b88f359554bf066af3017e711617b4cb025cafbd71eb5`. Password files stayed in `/tmp/oo-a367-eef831aa-dba4-4af2-a78b-541bc9e0a367/secrets` mode `700`. They are not in this tree.

Databases created inside this cluster: `oo_a358_review` and `oo_a367_oracle`, both owned by `oo_review_ledger_admin`. The author test hardcodes the first name. The independent oracle uses the second, login `oo_a367_runtime`, project `ora367`.

## Author tests on this cluster

Builder `oo-a367-rust`, 6 GiB, 1 CPU, `--cap-drop ALL`, private network, `RUSTUP_TOOLCHAIN=1.94.0`. Each phase inspect is `exit=0 oom=false privileged=false network=oo-a367-eef831aa-net memory=6442450944 nano=1000000000`. Caps files are `["ALL"]`.

`linux/author/test.exit` is `0`. `linux/author/test.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `Finished test profile in 0.05s` on the cache from the first compile, `sha256::tests::known_answers` ok, contract `8 passed; 0 failed` including `commit_unknown_is_ambiguous_and_not_a_success`, `private_postgres_matrix` ok in 2.01s.

`linux/author/clippy.exit` is `0`. `linux/author/clippy.log`: `rustc 1.94.0`, `info: downloading component clippy`, `Finished dev profile in 20.58s`. No `warning:` line. The command was `cargo clippy --locked --offline --all-targets -- -D warnings` while the oracle file was absent.

`linux/author/restart.exit` is `0`. After `docker restart` of `oo-a367-pg`, `private_postgres_matrix` ok in 0.12s. `linux/author/history.sha256` is 64 bytes and equal to the author marker `c7e08e02cde66ab86b75394488f0d263ec9475cecc75884599de3703db8264ce`.

## Independent oracle

`linux/author/oracle.exit` is `0`. `linux/oracle/oracle.log`: `independent_boundary` ok in 0.78s, then `Finished dev profile in 0.26s`. No `warning:` line and no `error:` line. Command: `cargo test --locked --offline --test oracle_a367` and `cargo clippy --locked --offline --test oracle_a367 -- -D warnings`.

`oracle/summary.json`:

- `auditor_batch` `role.forbidden`
- `auditor_tuples_unchanged` true
- `track_counts` `on`
- `pack_outcome` `packed`
- `pack_command_sha256` `36e1e461b388e7a6eaf084109f2709b7d291865db171fb8c6a02d60ebb6b00cb`
- `pack_candidate_sha256` `6161616161616161616161616161616161616161616161616161616161616161`
- `pack_outcome_sha256` `2317e15eac50d9aad9ae7f958212a7390e21da21f4749e6c015976946c0fb5d3`
- `git_applied` false, `published` false, `curated_files_written` false
- `grants` `not_authorized`, `authenticated` false
- database `oo_a367_oracle`, project `ora367`

The command hash, candidate hash, and outcome hash equal the author receipt for `class.b.note` in the a358 report. This oracle hashed the compiler bytes itself. The fixture is `tests/support/world.rs` `pack_fixture` from the archive, called twice, and the two compiler documents were equal. Outcome protocol is `description_pack::PACK_PROTOCOL`.

The same test also stored one label approval as one decision and one audit, replayed it with both counts unchanged, refused a second choice as `idempotency.conflict`, and refused pre-inserted audit key `poison.a367` as `audit.rollback` with decision count unchanged. Malformed `{` left `compiler_receipt` count unchanged and key `class.bad` at 0. The positive receipt for `class.pack` is `bound`, then `replay`, with receipt count 1, and the decision `body_sha256` unchanged.

`linux/proof.exit` is `0` from that oracle rerun. `linux/proof-driver.log` ends `phase oracle failed` from the prior attempt. `linux/proof-driver-oracle.log` is `own=0 global=1 host_tools=0`.

## Failed attempts

Attempt 1 compiled in 1m 42s. Contract tests passed. `private_postgres_matrix` panicked at `tests/live.rs:573` because `REVIEW_LEDGER_MARKER` pointed at `/evidence/author/history.sha256`. The mount is `linux/author`. Log: `failed-attempts/attempt-1-marker.log`. The marker path was corrected. `oo_a358_review` was dropped and recreated in this cluster.

Attempt 2 passed the author test, product Clippy, restart, and `independent_boundary`. Clippy of the oracle test then failed: unread `ReviewFixture` fields when only `pack_fixture` is compiled, and `choose` has 11 arguments. Log: `failed-attempts/attempt-2-oracle-clippy.log`. `failed-attempts/oracle.log` is the same bytes, SHA-256 `11147ac96db42f46e324a5707ccbd6816a8d76dca6b80713e552137c7c1b3c96`. The evidence oracle gained the allow attributes. `oo_a367_oracle` was dropped and recreated. The product tree was not edited.

`failed-attempts/none.txt` was not written. The first failure record exists.

Raw exit index: `linux/exits.txt`.
