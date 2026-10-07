# REPORT — a323 completed-object delete

Verdict: `parent_complete` is false. Linux proof-3 exited 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a323`. Baseline `895864a5bf64e50cef84075d20fd7b0c9e41c264`. Contract commit `e6ebf93b5ac261c097a24d145611e0300e4f88b5` (`FILEHOST-DELETE-CONTRACT.md` blob `cb8de1af2712005b481ac2a0a792a9cff0ab9014`). Product commit `c27525a2a7c31dfbe9cb64b183312c7dab354511`. The baseline and the contract commit are ancestors of the product commit. Proof-3 launched while `linux/proof-3/repo-head.txt` was `3517cd131719cfdd572ba2b85bb7013e6c90588e`.

Owned paths: `crates/oo-control/src/file_hosting.rs`, `tools/artifact-retention/**`, and this evidence directory. Root `Cargo.toml`, root `Cargo.lock`, `crates/oo-control/src/lib.rs`, and `crates/oo-control/Cargo.toml` were not edited. No registry dependency was added.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder, 6 GiB, 1 CPU, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, pids 4096, tmpfs `/tmp` `nosuid,nodev,noexec,size=512m`, `HOME=/tmp`, private `CARGO_HOME` and `CARGO_TARGET_DIR`. PostgreSQL was not started. The pinned Postgres image was not used. Hash commands are `shasum` and `git hash-object`. Product tests and the lock oracle are Safe Rust. No Python oracle.

The fixed archive is `git archive` of baseline `895864a5` with `rust-toolchain.toml` moved aside so the image compiler 1.94.0 runs. The toolchain file at the baseline names channel 1.98.1. Proofs overlay `crates/oo-control/src/file_hosting.rs` and `tools/artifact-retention` onto that archive. They reuse the pre-edit network and volumes.

Recipe origins are in `linux/pre-edit/recipe-origin.txt` and each proof `recipe-origin.txt`:

- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. Memory 8g to 6g, CPUs 4 to 1, global cap 6 to 10, owner a323.
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. Postgres omitted.
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh`, sha256 `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537`. No Python hashlib.
- a209 smoke shape `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a209/SP05/download-eof-review/pre-edit/inside.sh`, sha256 `b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9`.
- a311 proof-6 launch and inside, lock oracle copied byte-for-byte. Oracle sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`. The a311 gate that fails when `file_hosting.rs` differs from baseline was dropped. Emitter binary is `/target/lock_from_baseline`.

## Smoke, before the product edit

`linux/pre-edit/docker-run.exit` is 0. Console ends `smoke=pass`. Container id `d1910761a912573a537b7087382c047511c88d859f321d5e7732a6b596fcee05`, removed by the launch script. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a323-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`.

## Attempts kept

| Run | docker-run.exit | Container id | What the console shows |
|---|---|---|---|
| proof-1 | 101 | `3dd98155cc155e62a6f9f4c47902b03e411610bff8279633cff4def2c3b318d4` | `file_hosting` 16 passed. `delete_without_lock` passed. Clippy `-D warnings` on `oo-control` failed: two `too_many_arguments` (8/7) and one `manual_is_multiple_of`. Retention tests did not run. |
| proof-2 | 101 | `a6a6ea9ee4313fcde531548da304dbcfb5d91b97a5cb0ecd73db2904bb570e23` | `file_hosting` 16 passed. `delete_without_lock` passed. `oo-control` clippy passed. Retention 13 live tests and the lock-subset test passed. Retention clippy failed on two `collapsible_if` sites in `reaper.rs`. `cargo fmt -p oo-control` rewrote foreign crate files inside the proof snap. The launch copies formatted sources back only on exit 0, so the host tree was left unchanged by that snap. |
| proof-3 | 0 | `0f2b487653d6dedec310e8dc8fc3df68db22567575368621fecfb235e728af4b` | Owned-file `rustfmt --check` passed. Tests and clippy passed. `phases/worst.exit` is 0. Console ends `proof=pass`. |

Failed consoles were not rewritten. Named containers were removed by their launch scripts.

## Green proof

`linux/proof-3`. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a323-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Network `internal=false` because the image can still download rustfmt and clippy components. Both components were already installed (`phase_exit=0`). Lock check line: `check_ok registry_pins=185 packages=189`.

`cargo test --locked --offline -p oo-control --test file_hosting`: 16 passed in 37.79s. `phases/file-hosting-test.exit` is 0.

- `abort_releases_quota_and_leaves_foreign_bytes`
- `checksum_and_size_failure_leave_no_visible_object`
- `concurrent_complete_and_part_keep_one_object`
- `concurrent_quota_two_hosts_and_child`
- `conflicting_part_and_config_do_not_overwrite`
- `download_deadline_does_not_start_bytes_after_held_get`
- `download_eof_rejects_flipped_bytes_and_keeps_a_clean_object`
- `download_pull_memory_is_bounded`
- `download_pull_refuses_other_context_and_altered_bytes`
- `download_pull_verifies_identity_without_an_object_buffer`
- `download_regressions_deadline_cancel_manifest_truncate_quota`
- `generic_store_refuses_before_any_write`
- `limits_and_names_are_refused_before_write`
- `multipart_reopen_reads_manifest_and_lineage`
- `streaming_complete_memory_is_bounded`
- `two_contexts_deny_a_known_key`

`cargo test --locked --offline -p oo-control --lib delete_without_lock`: `new_returns_unsupported_before_store_io` passed. `oo-control` clippy `-D warnings` exited 0.

`cargo test --manifest-path tools/artifact-retention/Cargo.toml --locked --offline`: library 0, binary 0, doctests 0, `tests/live.rs` 13 passed in 0.15s, `tests/lock_subset.rs` 1 passed. Retention clippy `-D warnings` exited 0.

The 13 live tests:

- `completed_open_registration_uses_the_live_manifest`
- `current_tuple_refuses_the_pull_without_a_url`
- `download_race_keeps_a_finished_pull_and_fails_an_open_one`
- `guessed_paths_and_symlink_roots_are_refused`
- `kill_at_quota_release_restarts_without_a_second_release`
- `open_abort_does_not_claim_part_absence`
- `open_abort_keeps_batch_deletion_success_false`
- `operator_uuid_is_the_directory_name`
- `production_clock_matches_the_process_clock`
- `published_reap_observes_absence_and_releases_quota_once`
- `tamper_drift_symlink_and_read_error_do_not_delete`
- `ttl_classes_stay_distinct_and_workspace_defaults_off`
- `wrong_tuple_or_hash_does_not_delete`

Phase exits 0: root lock, `lib.rs`, `oo-control` manifest, child lock, `FILEHOST-DELETE-CONTRACT.md`, and `crates/oo-builder/src/lib.rs` (foreign file, sampled so owned-only rustfmt cannot hide a workspace rewrite).

`file_hosting.rs` content sha256 `8804b4eea7463c4853ca159e795eb6edcd2bf53d9bcbb86ece8af98752442627`, git blob `aa16be987070522105b7b4079ed00ef89899b7f3`. Root lock content sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`, git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`.

## What the live tests measured

A published upload is completed and downloaded through FileHost, then expired and reaped. The reap calls `FileHost::delete_completed`. After the reap, `observe_completed_absence` is true, `completed_quota_release_count` is 1, and `read_object` is `NotFound`. `parent_complete` is false. A second reap stays at count 1. The same project can begin a new session with the freed bytes. A further begin of the same size is `Quota`. A second project registered inside the uploaded-source window stays readable. Planted `dest#1` and `notes` names under the part prefix remain.

A wrong digest returns `NotFound` and leaves the object. A wrong grant, and another project holding the real digest, return `Denied` and leave the object. Count stays 0.

Killing the reaper after checkpoint `quota_released` prints no success JSON. Restart completes the delete once. The release count stays 1. The checkpoint file still records `absence_verified false` and `deletion_success false`.

A finished download stays `is_complete` after the later delete. An unfinished small pull then sees `Corrupt`. A pull that has returned 8192 bytes of a larger object then sees `Store`. Neither unfinished pull becomes complete.

An open registration that reaches `complete` before reap is deleted from the live manifest. A batch that still contains an open abort keeps `deletion_success` false. The open row stays `aborted_parts_unverified`.

The deployment directory name must be the operator UUID passed to `create`. `owner.txt` must carry that same UUID. The previous a311 UUID is accepted only when the directory name is that UUID. Uppercase, a mismatched directory, and `..` are refused.

Workspace 24h stays off unless `explicit_workspace` is set. Uploaded source stays the 180 day class. `--now` exits 1. `bearer_url` stays `none`. `gateway_url_revocation` stays `not_implemented`.

## Remaining integration

`required_integration()` returns these seven names. This directory does not mark them done.

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

`filehost_completed_object_delete` and `filehost_quota_release_on_completed_delete` are absent from that list. `parent_complete` is still false on every report, including a report whose `deletion_success` is true.
