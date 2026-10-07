# REPORT — a338 retention completion

Verdict: `parent_complete` is false. Linux post-edit-4 exited 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a338`. Baseline `e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd`. Addendum commit `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81` (blob `3a9de3efdca24ec03b3b4e197a1119f2afa8a2cd`). Product commit `def1f7eea53794449d3975b1987d700727cb7d60`. Prior product `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Historical delete contract `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. Review archive `53931278e69d723d0ce38961ded897fd6e41d5d3` is an ancestor of the baseline. The baseline, the addendum, the prior product, and the historical contract are ancestors of the product commit. The product commit's parent is `0a4ae4baee11d0c45f7725035439164ed2fcc8d7`, which is `linux/post-edit-4/repo-head.txt`.

Owned product paths in that commit: `crates/oo-control/src/file_hosting.rs`, `tools/artifact-retention/CONTRACT.md`, `tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md`, `tools/artifact-retention/src/lib.rs`, `tools/artifact-retention/src/main.rs`, `tools/artifact-retention/src/reaper.rs`, `tools/artifact-retention/tests/live.rs`. The frozen addendum was already committed and was not edited again. Root `Cargo.toml`, root `Cargo.lock`, `crates/oo-control/src/lib.rs`, `crates/oo-control/Cargo.toml`, `crates/oo-control/tests/file_hosting.rs`, and `tools/artifact-retention/Cargo.toml` were not edited. No registry dependency was added. `artifact-retention` still has no `object_store` dependency.

The post-edit-4 snap of those seven files matches the product commit: `file_hosting.rs` sha256 `f37377cb0bc359c84c2efb1bce55a69d429770e334bec9544906daca49e6e5ce`.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder, 6 GiB, 1 CPU, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, pids 4096, tmpfs `/tmp` `nosuid,nodev,noexec,size=512m`, `HOME=/tmp`, private `CARGO_HOME` and `CARGO_TARGET_DIR`. PostgreSQL was not started. The pinned Postgres image was not used. Hash commands are `shasum` and `git hash-object`. Product tests and the lock oracle are Safe Rust. No Python oracle.

This task launched five builders, one at a time: pre-edit, then post-edit through post-edit-4. The development cap is 20. No review slot was used. `capacity.txt` on the green run: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Post-edit-2 and post-edit-3 recorded `global=1` from another actor. This task's own running count stayed under 1.

The fixed archive is `git archive` of baseline `e9cba7df` with `rust-toolchain.toml` moved aside so the image compiler 1.94.0 runs. The toolchain file at the baseline names channel 1.98.1. Post-edit proofs overlay the owned sources onto that archive. They reuse the pre-edit network and volumes. Network `oo-a338-eef831aa-net` is `internal=false` because the image can still download rustfmt and clippy components. Both components were already present (`phase_exit=0`).

Recipe origins are in each `recipe-origin.txt`:

- This child's pre-edit launch sha256 `b378098c605d3cc29c4ed34d32b2f02016de7423e345f70b7834831b7500014e`.
- a336 preoverlay launch `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review/linux/preoverlay/launch.sh`, sha256 `a9a50886591cfbcf4d63ac30587f4a3b0c9bc9dd266d38f2fd1da16bc17c39f9`.
- a323 proof-3 launch sha256 `8448a0df6061ac1faf7a17a06de9ca549cc9a413d6c705f65b91d114bb1b2c01`.
- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`.
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. Postgres omitted.
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, sha256 `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537`.
- a311 lock oracle copied byte-for-byte, sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`. `check_ok registry_pins=185 packages=189`.

## Smoke, before the product edit

`linux/pre-edit/docker-run.exit` is 0. Console ends `preedit=pass`. Container `oo-a338-preedit`, id `f337d761f6e0036a64ee86fb9cce9ba970dde09e876d4fb3bc8ed4c19cc497b4`, removed by the launch script. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a338-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. Worktree at launch `6c78e2673837da9dc653c3304c974479ea10aea2`. The archive is the baseline, so this smoke is the previous product.

`cargo test --locked --offline -p oo-control --test file_hosting`: 16 passed in 36.74s. `delete_without_lock`: 1 passed. Retention live tests: 13 passed in 0.16s. Lock subset: 1 passed. `phases/worst.exit` is 0.

## Attempts kept

| Run | docker-run.exit | Container | Id | What the console shows |
|---|---|---|---|---|
| post-edit | 101 | `oo-a338-postedit` | `f4e8568df7fcd2fc77d4b831eb1ca28939f1754500e47e38a14ac33c4a9f9bee` | `file_hosting` 16 passed. Lib 5 passed, 1 failed: `legacy_quota_released_reconciles_without_a_second_release` panicked at `file_hosting.rs:4609` because `std::fs::write` of the planted tombstone returned `Os { code: 2, kind: NotFound }`. `meta/tombstone` does not exist until the host writes one. |
| post-edit-2 | 101 | `oo-a338-postedit2` | `3857a9a9058a90066d2dac9ebed04fc4f420bd014102c99dbeb51580674af2cf` | `file_hosting` 16 passed. All 6 lib tests passed. Clippy `-D warnings` failed: `collapsible_if` at `file_hosting.rs:1970`. Retention tests did not run. |
| post-edit-3 | 101 | `oo-a338-postedit3` | `7913733c2df17fbaac82141dc77bb3e2efbdd2ec64bda1ab42ec92333341368f` | fmt, `file_hosting` 16, lib 6, and `oo-control` clippy exited 0. Retention live tests: 11 passed, 3 failed. `published_reap_observes_absence_and_releases_quota_once` at `live.rs:355` (`again.deletion_success`). `kill_before_bytes_keeps_quota_until_restart` at `live.rs:779`. `kill_after_bytes_before_quota_releases_once` at `live.rs:895`. The kill windows themselves had already resumed. The failing asserts are the repeated reap after `begin` of the same upload config, which rewrites the shared idempotency pointer. |
| post-edit-4 | 0 | `oo-a338-postedit4` | `a69326f489f5482ab60d64fe3324ef4c98e969e7fa7b3b696657d284ef51e8aa` | Owned-file `rustfmt --check` passed. Tests and clippy passed. `phases/worst.exit` is 0. Console ends `postedit=pass`. |

Failed consoles were not rewritten. Named containers were removed by their launch scripts. `OOMKilled=false` and `Privileged=false` on every run. Memory stayed `6442450944`. `NanoCpus` stayed `1000000000`.

## Green proof

`linux/post-edit-4`. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a338-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`.

`cargo test --locked --offline -p oo-control --test file_hosting`: 16 passed in 36.52s. `phases/file-hosting-test.exit` is 0. The sixteen names are the same set as the pre-edit smoke, including `abort_releases_quota_and_leaves_foreign_bytes`.

`cargo test --locked --offline -p oo-control --lib`: 6 passed in 0.03s.

- `file_hosting::delete_without_lock::new_returns_unsupported_before_store_io`
- `file_hosting::retention_completion::abort_reaps_platform_multipart_staging`
- `file_hosting::retention_completion::legacy_quota_released_reconciles_without_a_second_release`
- `file_hosting::retention_completion::mismatched_quota_body_writes_nothing`
- `file_hosting::retention_completion::quota_stays_until_bytes_are_absent`
- `file_hosting::retention_completion::symlink_staging_keeps_the_quota_and_writes_no_tombstone`

`oo-control` clippy `-D warnings` exited 0.

`cargo test --manifest-path tools/artifact-retention/Cargo.toml --locked --offline`: library 0, binary 0, doctests 0, `tests/live.rs` 14 passed in 0.24s, `tests/lock_subset.rs` 1 passed. Retention clippy `-D warnings` exited 0.

The 14 live tests:

- `completed_open_registration_uses_the_live_manifest`
- `current_tuple_refuses_the_pull_without_a_url`
- `download_race_keeps_a_finished_pull_and_fails_an_open_one`
- `guessed_paths_and_symlink_roots_are_refused`
- `kill_after_bytes_before_quota_releases_once`
- `kill_before_bytes_keeps_quota_until_restart`
- `open_abort_proves_part_absence_and_reaps_platform_staging`
- `open_and_published_reap_verify_absence_parent_stays_open`
- `operator_uuid_is_the_directory_name`
- `production_clock_matches_the_process_clock`
- `published_reap_observes_absence_and_releases_quota_once`
- `tamper_drift_symlink_and_read_error_do_not_delete`
- `ttl_classes_stay_distinct_and_workspace_defaults_off`
- `wrong_tuple_or_hash_does_not_delete`

Phase exits 0 include root lock, `lib.rs`, the `oo-control` manifest, child lock, and `crates/oo-builder/src/lib.rs`. The post-edit launch does not require `file_hosting.rs` or the delete contract to match the baseline.

`file_hosting.rs` content sha256 `f37377cb0bc359c84c2efb1bce55a69d429770e334bec9544906daca49e6e5ce`, git blob `8943ac612ca39be64440d8301dd12b48b5d39976`. Root lock content sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`, git blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`, identical at the baseline and at the product commit. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`.

## What the green tests measured

A completed object and an open multipart are expired and reaped through `FileHost`. After the published reap, `observe_completed_absence` is true, `completed_quota_release_count` is 1, and the object read is `NotFound`. Host-generated `original#1` and `00001#1` are gone. Planted `original#01`, `dest#1`, `notes`, and a neighbor outside the root stay. `parent_complete` is false. `deletion_success` is true for that batch. A second reap stays at count 1.

`delete_completed(..., true)` returns while the quota body is still `64`, the phase is `data_absent`, the order is `bytes_then_quota`, and the release count is 0. A concurrent begin of that size is `Quota`. The follow-up call with `false` observes absence, count 1, and the freed size can be begun again. A further begin of the same size is `Quota`.

SIGKILL at checkpoint `data_deleting` leaves the object readable and the count at 0. Restart then verifies once. SIGKILL at checkpoint `data_absent` leaves the object absent and the quota file still `64`. Restart releases once. Both restarts record `restarted_from_partial` true and `parent_complete` false. A later same-config begin reuses the idempotency slot; the repeated reap still reports count 1. `--pause-after quota_released` exits 1. `--now 1` exits 1.

An open registration's `abort` returns `Ok`. `observe_open_absence` is true. Platform part `00001` and platform staging `00001#1` and `original#1` are gone. `dest#1`, a user-kept name, and foreign notes stay. A second `abort` is `NotFound`. A wrong grant is `Denied`. A mixed open-and-published reap can set `deletion_success` true. `parent_complete` stays false.

A symlink staging name writes no tombstone and keeps the quota. A mismatched quota body writes nothing. A legacy `quota_released` tombstone with the key already gone is reconciled once: bytes are removed, the old key is not recreated, a foreign reservation stays, and the count stays 1. A reappeared mismatched body is `Conflict`.

Wrong tuple, wrong hash, symlink, and store IO refuse the delete. Workspace 24h stays default off. Uploaded source stays 180 days. `bearer_url` stays `none`. `gateway_url_revocation` stays `not_implemented`. `FileHost::new` still returns `Unsupported` before store IO.

## Original E41, as measured here

Plan SP05-W06 still asks for an authenticated download proxy and the current permission tuple, unified delete of TTL, version, cache, multipart, and temp, CSV/Excel formula escaping that leaves internal Parquet numbers alone, artifact and download metadata with reaper evidence, old URL revocation within 5 seconds, AT-FILE-05, and AT-SEC-03. Cross-user, expiry, downgrade, tampered URL, formula, and path traversal remain on that card. Refusal when the current permission cannot be shown is still wider than a same-username check.

This child closed two of the seven seams that a336 left open, and only those two:

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`

`required_integration()` now returns these five names, length 5. They are the original names:

- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

`filehost_completed_object_delete` and `filehost_quota_release_on_completed_delete` stay off that list. The quota release now waits until bytes, sidecars, parts, and platform staging are absent. The seam was not renamed. Removing the two gap names is not acceptance of the parent card. `parent_complete` stays false on every report, including a report whose `deletion_success` is true.
