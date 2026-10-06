# Paired trial report adapter proof

Session `eef831aa-dba4-4af2-a78b-541bc9e0a250`. Fixed product archive `8f192847ea7c4906c13f8419b5de2669875f7ef9`. This is the SP12-W02 adapter B-D05: two trusted SQL runner reports, plus explicit paired variant, budget, and scorer labels, become one full question and trial vector document.

The adapter writes the statistics input `oo.benchmark.trial-statistics/v1`. It does not compute pass^k, a bootstrap, a percentile, or a gate. The existing `trial-statistics` CLI, built from the same fixed archive, accepted that input and wrote the gate report. This report does not accept B-D05, a parent completion rate, a snapshot certificate, a private TLS deployment, or a release.

## What was built

`benchmarks/trial-report-adapter` is its own Cargo workspace. It is not a root workspace member. The root manifest, root lock, and product dependencies were not edited. `oo-core` is a path dependency. The proof compiles that path against a `git archive` of the fixed commit, with the archive's `rust-toolchain.toml` renamed so the image stays on 1.94.0. The archive also supplies the already committed `sql-runner` and `trial-statistics` workspaces. Those crates are built locked and offline. They are not edited.

The standalone lock was created with `cargo generate-lockfile`, not `cargo update`. Every registry name, version, and checksum in that lock is already in the fixed root lock: 115 registry packages, zero mismatches. The two path packages are `oo-core` 0.1.0 and `oo-trial-report-adapter` 0.1.0. `linux/impl-3/lock-drift.txt` is empty. The pins are serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The verified lock is `benchmarks/trial-report-adapter/Cargo.lock`, copied from the attempt 3 archive. Attempts 1 and 2 generated the same lock bytes.

## Proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0. One CPU, 6 GiB, cap-drop ALL, no new privileges, private network `oo-a250-eef831aa-net`, private cargo and target volumes. No PostgreSQL. Each container was removed after its run. No prune.

Smoke, before the adapter sources were archived: `oo-core` test `yaml_and_json_numbers_stay_lexical`, 1 passed, 18 filtered, exit 0. The adapter tree was absent from that archive (`adapter-absent.exit` 0) and the renamed toolchain file was absent (`toolchain-absent.exit` 0). The fresh cargo volume missed the offline fetch (exit 101) and completed the online fetch (exit 0). Recorded in `linux/smoke/`. Container `4e84e09751ba759a5c7ecb4c42d3ebdf82d64ae7c43b548ec42a9d8737be4c21`. Host config: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a250-eef831aa-net`. Capacity at launch: `own=0 global=0 host_cargo=0`.

The adapter sources were drafted on the host while that smoke container was already sealed. The smoke result does not depend on those drafts. Implementation started after smoke exit 0.

Implementation attempts 1 and 2 are preserved and failed. Attempt 3 is the proof.

| Attempt | Docker exit | Cause |
| --- | --- | --- |
| `linux/impl` | 101 | `tests/cli.rs` searched for `"model":"trusted_sql"` with a 20-byte window. The needle is 21 bytes, so the assertion failed after the fixture bytes had already matched. rustfmt, clippy, and the CLI fixture comparison exited 0. Container `0371870757ce2ee55cc4a6ad2dcef2f6b3b1e8b23af9ff0f6ca59ba71e056d67`. |
| `linux/impl-2` | 1 | rustfmt wanted `contains_bytes` wrapped. Tests, clippy, and the CLI fixture comparison exited 0. The live runner proof passed in 8.42s. Container `67ed3e4067dcdeb52b58610f6934fb41ade36c65a1fe01ab6a255995f65102d9`. |
| `linux/impl-3` | 0 | tests, rustfmt, clippy, and CLI fixtures. Container `ae64ee51e2b861c3d5f6169924de4adbd862eda02b927ab382ebb6b21ae2eaa8`. |

Attempt 3 host config: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a250-eef831aa-net`. Phase exits for rustc, source hash, lock, lock drift, sql-runner build, trial-statistics build, test, component install, rustfmt, clippy, CLI, and docker are all 0.

`cargo test --locked --offline -- --test-threads=1`: 16 passed, 0 failed. That is 10 bound tests, 3 compiled-CLI tests, 2 gold tests, and 1 live proof. Library, binary, and doc tests are empty. The live proof finished in 8.25s.

The debug binary then compared the handwritten fixtures. `k2-mixed` and `allfail` exited 0 with empty stderr. A second `k2-mixed` run matched the first stdout. `missing-trial` wrote `trial-report-adapter: trials /reports/1/trials`. `tuple-mismatch` wrote `trial-report-adapter: tuple /reports/1/tuple/project`. `duplicate-key` wrote `trial-report-adapter: json`. Those three exited 2 with empty stdout.

The attempt 3 source list, 20 files, matches the crate sources that were tested. The lock file copied into the crate is the lock that attempt 3 generated inside the archive. The source list was taken before that lock existed.

## Live runner vectors

The live proof compiles `tests/support/stub.rs` with `rustc --edition 2021 -D unsafe_code` into the container target directory and drives the archived `sql-runner`. The stubs are purpose-built executables. This unit does not start PostgreSQL and does not claim a private TLS session. `PGPASSWORD` stays unset. The password bytes and the marker `PRIVATE KEY` are absent from `linux/impl-3/proof/`.

`tuple.model` on every runner report is `none`. The adapter writes `model` `trusted_sql`. `snapshot` stays the declared label `declared-a250`. Executable hashes are shape-checked and are not copied.

The k=2 pair has five ordered cases, suite `live-k2`, budget label `k2`, seed `11`, resamples 8. Mapped vectors:

| Case | Left | Right |
| --- | --- | --- |
| desk | pass, fail | pass, pass |
| gate | fail, fail | fail, fail |
| note | needs_reannotation, needs_reannotation | needs_reannotation, needs_reannotation |
| slow | fail, fail | pass, pass |
| drop | pass, pass | pass, fail |

Runner exits were 1 and 1. Adapter exit was 0. `trial-statistics` exit was 1. Its report has gate `failed`, `question_count` 5, `reannotation_count` 1, `trial_count` 2, left `pass_k_count` 1, right `pass_k_count` 2. Percentile keys are present. Their values are not a gold file for this adapter.

The all-fail pair is one case, k=4, suite `live-allfail`, budget `k4`. Both sides map to fail, fail, fail, fail. Runner exits were 1 and 1. Adapter exit was 0. `trial-statistics` exit was 0. Its report has gate `passed`, `question_count` 1, `reannotation_count` 0, `trial_count` 4, and both `pass_k_count` values 0.

The tuple pair is two real k=1 pass reports whose `project` fields differ. Adapter exit was 2, stdout empty, stderr `trial-report-adapter: tuple /reports/1/tuple/project`. The missing-trial case starts from one real k=2 pass report, drops trial index 1 in Safe Rust, and repairs the runner counts so the refusal is the short vector. Adapter exit was 2, stdout empty, stderr `trial-report-adapter: trials /reports/1/trials`. The dropped trial is not imputed.

The handwritten `k2-mixed` fixture has four cases and is the library byte oracle. Its right note row is `needs_reannotation` then a timeout mapped to `fail`. A real runner annotation covers every trial of one case, so the live note case is reannotation on both trials. The fixture row remains a legal report. The live five-case vector does not byte-match that fixture.

## Inventory

`tools/sha256_manifest.rs` is the untouched a199 hasher. `linux/inventory/launch.sh` compiles it in the same pinned image and writes `crate-sha256.txt` and `SHA256SUMS.txt`. `linux/inventory/rerun.sh` runs after that launcher has closed its console, hostconfig, and exit files, and rewrites those two manifests. The second stdout stays at `/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250/hash-again.log` and is not copied into this directory.
