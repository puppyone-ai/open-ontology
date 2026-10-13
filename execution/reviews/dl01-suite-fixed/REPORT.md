# DL01-W1-27 fixed suite review

Reviewer Grok grok-4.7/xhigh, session `28b6fc31-611c-4b9a-bcd9-996589ca83ae`. The frozen child is DL01-W1-07 in `execution/batches/dl01-wave1-20261009/DL01-W1-07-suite.md`. Fixed source `76928401921e941d0dc9454a0d2dd1db6088b928`. Evidence seal `7216af5b0ee60f48e026449055f43dc84ca41242`. Dependency archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`.

Verdict: approved, bounded to the claim-checker child. The child checks supplied matrix claims and evidence packages, prints `oo-source-conformance/report/v1`, and leaves `promoted` and `matrix_rewritten` false. DBX-W14 stays open. Matrix integration stays pending. This review does not accept a source capability grade, a verified grade, or a 14-day nightly promotion.

## Identity

Git ancestry holds: the dependency commit is an ancestor of the source commit, and the source commit is an ancestor of the seal. The source parent is `e6b7b80af2deaf97a78b6b84a63300317f62d8b2`. The crate tree is identical at the source commit and the seal. The worktree copy of `tools/source-conformance` matches the source commit.

The seal adds `receipt.json` and `SHA256SUMS` only. All 522 checksum lines match blobs in the seal, and those two files are excluded from the sums. Sealed `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `linux/post-edit/journeys/positive.stdout` match the receipt hashes. The sealed support matrix is `c4fc6698cfb08d74b08fcadbe1f5a6acb2ba41a769905b06328b28f3ab8944b4`. Historical `evidence/gpt-minimal-closure/REPORT.md` and `HANDOFF.md` match the receipt file hashes. The manifest field `manifest_sha256` is `f32a98fc478f0015345c5f2d52c0deb0cbcb51d7027b3c845f49d5b1c77dbf66`.

Sealed green exits are test 0, Clippy 0, journeys 0, and smoke 0. Preserved failures remain in the seal: post-edit attempt 1 test 101, Clippy 101, and each journey exit 127; attempt 2 Clippy 101 and smoke 101. The attempt 1 phase file `phases/journeys.exit` is 1 because the journeys missed their expected exits. The receipt's journey figure 127 matches the individual journey files.

`tools/source-support` is absent from the dependency archive. The source commit tree inherits that directory from the shared branch. The compile input did not include it.

## Compile gate

One Linux arm64 Rust 1.94 builder. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `--pull never`. Recipe origin is the sealed author `linux/post-edit/launch.sh` (`1da7bdd28cbc9045701ec12959f1d25c5e42d39338d2660b02236b7a92f681ed`) and `inside.sh` (`c64e37363514943164b923f3cb5dcfb5e71071ee1b838ff63cf58bc1f93f5299`), adapted to prefix `oo-dl01w1-27` and this session. The container was `oo-dl01w1-27-build`, `--cpus 1`, `--memory 6g`, `CARGO_BUILD_JOBS=1`, `--cap-drop ALL`, network and volumes private to this prefix. `rustc` inside the container was `/usr/local/cargo/bin/rustc`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. No host Rust compiler ran. No Postgres container was started: the author receipt has `database_count` 0, and the historical proof is files.

Admission under `/tmp/oo-dl01-wave1-resources.lock` saw `global_rust_builders` 1 and `own_rust_builders` 0, under the cap of 10. Compile input was `git archive 577160c22de63adb9b38e0147f3bc2da893c4d9d` plus `tools/source-conformance` from `76928401921e941d0dc9454a0d2dd1db6088b928`, mounted read-only. `source-support-absent.exit` is 0. `cargo update` was not invoked.

| Phase | Exit |
| --- | --- |
| fetch `--locked` | 0 |
| build `--locked --offline --bin source-conformance` | 0 |
| test `--locked --offline` | 0, `passed=9 failed=0 ignored=0` |
| clippy `--locked --offline --all-targets --no-deps -- -D warnings` | 0 |
| smoke | 0 |
| docker wait | 0 |

Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`, `CapDrop=["ALL"]`. The debug binary hash is `3d7b79c63e4b49b0bda5ac7039cfddc7490d348fda8902a9bea5a485e5eec88c`, size 4743216 bytes. The binary was removed with the private target volume.

The crate's own 9 tests ran as the compile gate. They are not the acceptance result below.

## Independent probes

Inputs are under `harness/fixtures/` and use target ids `rev-w27-*`. They are not the author's `fixture-pg` gold. Raw argv, stdout, stderr, and exit files are under `linux/probes/`.

| Probe | Exit | Decisive result |
| --- | --- | --- |
| `map` | 0 | schema `oo-source-conformance/case-map/v1`, CC-01 through CC-13, five certified case ids |
| usage with no args | 2 | usage on stderr |
| `check` with no matrix | 2 | usage on stderr |
| matrix schema `oo-support-matrix/v0` | 1 | only `matrix.schema` |
| package schema `oo-source-conformance/evidence-package/v0` | 1 | only `package.schema` |
| raw hash tamper on `rev-w27-hash` | 1 | only `evidence.hash_mismatch` for `raw/reject-cdc.txt` |
| package target `rev-w27-foreign-source` | 1 | only `evidence.target_mismatch` |
| experimental `rev-w27-keel` batch_full package | 0 | cell `passed`, counts experimental 1, verified 0, certified 0, `promoted` false |
| designed catalog and batch_full, no package | 0 | both cells `not_required` |
| 14 suite days on designed catalog | 0 | info `nightly.not_promoted`, recommended level stays designed |
| missed day 2026-06-02 | 0 | `missed_days` is that day, trailing success 1, level stays designed |
| verified `rev-w27-yard` with two trailing failures | 1 | `nightly.streak_short` and `nightly.failure_downgrade`, recommended `experimental`, `downgrade_recommended` true, `matrix_rewritten` false |
| verified cell and one manual run, no package | 1 | `claim.manual_assertion` and `claim.missing_test`; `manual_runs` 1, `suite_runs` 0 |
| experimental RDS cell with container evidence | 1 | only `cloud.container_scope` |
| pass recorded on designed catalog | 1 | only `claim.silent_fallback` |
| sealed matrix plus sealed historical PG files | 0 | counts candidate 38, designed 181, excluded 8, experimental 0, verified 0, certified 0; historical classification `historical_draft_preview_not_conformance_suite`; `satisfies_cc_suite` false; `promotes_grade` false |
| writable copy of the sealed matrix | 0 | hash unchanged at `c4fc6698cfb08d74b08fcadbe1f5a6acb2ba41a769905b06328b28f3ab8944b4` |

Every report from these runs has `promoted` false and `matrix_rewritten` false. `source_support_consumer.status` is `pending`, `protocol_commit` is `absent-at-577160c22de63adb9b38e0147f3bc2da893c4d9d`, and `mutable_workspace` is `not-read`.

An additional observation, `identity-drift`, used `exact_version` `16.9` and `identity.service_version` `15.2`. Exit 0 with no findings. `CONTRACT.md` requires the identity object and requires `tested_versions` to equal `[exact_version]`. It does not require `service_version` to equal `exact_version`. The binary follows that contract. The observation is not a rejected child exit.

## Scope left pending

Matrix integration is pending, as the original card left the matrix read-only and forbade grade promotion. The checker does not execute a per-target suite, activate nightly CI, or write a matrix grade. The current matrix remains at zero verified and zero certified cells. A 14-day streak on a designed cell stays designed. DBX-W14 stays open. The source-support parse API in the crate stays a proposal until an immutable protocol SHA exists.

## Cleanup

At `2026-10-09T16:39:17Z`, container `oo-dl01w1-27-build`, volumes `oo-dl01w1-27-cargo` and `oo-dl01w1-27-target`, network `oo-dl01w1-27-net`, and `/tmp/oo-dl01w1-27` were removed. Each removal exit is 0. No container, volume, or network with this prefix remained. The pinned Rust image is retained. No database was created.
