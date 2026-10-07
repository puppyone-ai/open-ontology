# REPORT — a336 filehost retention review

Verdict: **APPROVE_LIMITED**. `parent_complete` is false. This directory does not close FILE-D03 / E41, SP05-W06, AT-FILE-05, or AT-SEC-03. It does not edit the ledger, does not add a countable unit, and does not accept a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a336`. Reviewed archive `53931278e69d723d0ce38961ded897fd6e41d5d3`. Product ancestor `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Contract ancestor `e6ebf93b5ac261c097a24d145611e0300e4f88b5` (`FILEHOST-DELETE-CONTRACT.md` blob `cb8de1af2712005b481ac2a0a792a9cff0ab9014`). Author baseline `895864a5bf64e50cef84075d20fd7b0c9e41c264`. All three are ancestors of the fixed commit. The author record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete`. The original retention packet is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention`. This directory is the independent review. It did not edit product source.

`linux/preoverlay/repo-head.txt` at launch was `a6ea064d62edd31de568a7685ac6f4fc2e11cc2c`. `linux/review/repo-head.txt` was `0a5c77af1165a4a9fcb2be7f87ecf8c33c891035`. The containers built `git archive` of `53931278`, with `rust-toolchain.toml` moved aside because that commit names channel 1.98.1. Workspace HEAD moved under other actors during the review. A later comparison of `2f6ad2a4a8b1a67cdcbdbf329f36844db439dfbb` still had the same `file_hosting.rs`, `lib.rs`, control manifest, child lock, delete contract, `reaper.rs`, and retention `lib.rs` blobs as `53931278`. Root `Cargo.lock` at that HEAD was `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`. The archive lock the containers checked is blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`, content sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. This review did not write the root lock.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. One builder at a time, 6 GiB, 1 CPU, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, pids 4096, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=512m`, `HOME=/tmp`, private volumes `oo-a336-eef831aa-cargo` and `oo-a336-eef831aa-target`. Network `oo-a336-eef831aa-net`, `internal=false`, because the empty cargo volume and the image still needed crates.io plus rustfmt and clippy. PostgreSQL was not started. The pinned Postgres image was not used. Hash commands are `shasum` and `git hash-object`. Tests, the lock oracle, and the review overlay are Safe Rust. No Python oracle.

Recipe origins are in `linux/preoverlay/recipe-origin.txt` and `linux/review/recipe-origin.txt`:

- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`.
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. Postgres omitted.
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh`, sha256 `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537`.
- a209 smoke shape `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a209/SP05/download-eof-review/pre-edit/inside.sh`, sha256 `b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9`.
- a323 proof-3 launch sha256 `8448a0df6061ac1faf7a17a06de9ca549cc9a413d6c705f65b91d114bb1b2c01` and inside sha256 `15ee95cf7e18ce5965dece4a648641fbef86db56e3d98465e204228f2afd37bf`. The proof-3 exit-0 copy of formatted sources back onto the product tree was not reused.
- a311 lock oracle copied byte-for-byte. sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`.

Preoverlay change: archive `53931278` itself, no `file_hosting.rs` overlay, no copy-back, `rustfmt --check` only, and `cargo fetch --locked` before offline metadata. Review change: copy `review_independent.rs` into the snap only, reuse the preoverlay network and volumes, no copy-back.

## Preoverlay, before the review overlay

`linux/preoverlay/docker-run.exit` is 0. Console ends `preoverlay=pass`. Container `oo-a336-preoverlay`, id `b01bb16a846c73e088b2c26120417aff492cd7fc9a0b14697967b569c1d2afd4`, removed by the launch script. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a336-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=3 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Lock line: `check_ok registry_pins=185 packages=189`.

`cargo test --locked --offline -p oo-control --test file_hosting`: 16 passed in 39.20s. `phases/file-hosting-test.exit` is 0.

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

`cargo test --manifest-path tools/artifact-retention/Cargo.toml --locked --offline`: library 0, binary 0, doctests 0, `tests/live.rs` 13 passed in 0.16s, `tests/lock_subset.rs` 1 passed (`registry_pins_are_a_subset_of_the_root_lock`). Retention clippy `-D warnings` exited 0. `phases/worst.exit` is 0.

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

Phase exits 0 include root lock, `lib.rs`, `file_hosting.rs`, control manifest, child lock, delete contract, and a sampled foreign `oo-builder` lib.

## Attempts kept

| Run | docker-run.exit | Container id | What the console shows |
|---|---|---|---|
| preoverlay-attempt-1 | 101 | `bf8344f2634634f1fe4aeab5e6976ceff7cff32b7374c6e6609c9eb2b7eb9dbc` | Lock check passed, 185/189. `cargo metadata --locked --offline` ran before fetch on an empty private cargo volume. `error: no matching package named tokio found`. |
| preoverlay | 0 | `b01bb16a846c73e088b2c26120417aff492cd7fc9a0b14697967b569c1d2afd4` | Fetch before offline metadata. Author suite and both clippy gates passed. Console ends `preoverlay=pass`. |
| review-attempt-1 | 1 | `49accfbb9a1be5ce96b9384168d33b8f603fe64a2a16f37e1d59ff9d66af6ed8` | `rustfmt --edition 2024 --check` printed a diff of `review_independent.rs` only. Product sources were not rewritten. `phases/fmt-check.exit` is 1. |
| review | 0 | `7a3b3f4db9d9403d465d138c8f31994f314d3831b0f18ab7dd444069b522a782` | Seven independent tests passed. Retention clippy exited 0. Console ends `review=pass`. |

Failed consoles were not rewritten. Named containers were removed by their launch scripts.

## Independent review

`linux/review`. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a336-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Overlay sha256 before the run: `d8ad6b23a4e0d8428beeebf5d7ff5040da76fe4f22e0f2ef4eeb32c7f8f3fdb3`.

`cargo test --locked --offline --test review_independent --test-threads=1`: 7 passed in 0.13s. `phases/test.exit` is 0. Retention clippy `-D warnings` exited 0. `phases/worst.exit` is 0. Source-unchanged phases, including author `live.rs` and `reaper.rs`, are 0. The review did not rerun the 16 file-hosting tests.

- `lifecycle_reap_proves_public_absence_and_keeps_parent_open`
- `quota_release_before_bytes_admits_one_new_reservation`
- `quota_release_then_symlink_is_not_absence`
- `sigkill_at_quota_released_restarts_once`
- `incomplete_pull_does_not_become_complete`
- `drift_symlink_tuple_and_read_error_do_not_delete`
- `open_abort_and_dest_staging_stay_unsupported`

Console line from the quota test:

`quota_window old_bytes=64 reserved=64 max=64 reservation_within_max=true bytes_plus_reservation_exceeds_max=true`

`HostLimits::try_new(64, 64, 10, 64)`. After `delete_completed(..., stop_after_quota_release=true)`, the old quota key is gone, `completed_quota_release_count` is 1, `observe_completed_absence` is not `Ok(true)`, and the object file is still 64 bytes. The same upload config resumes the existing session and does not recreate the quota key. A different display name begins a new session and writes a new quota body of 64. The reservation sum is 64, which is within `max_project_bytes`. The remaining object bytes plus that new reservation are 128. A third begin of the same size is `Quota`. Finishing `delete_completed(..., false)` removes the object, observation is `Ok(true)`, and the count stays 1. The new quota body is still 64, so the original config is `Quota` until that new session is aborted.

That window is the limit of this approval. The reservation sum itself did not exceed `max_project_bytes`. Public absence, one release, and the kill restart passed. Those are the conditions that would withhold this child. They did not fail.

A symlink planted on the object after `quota_released` makes the finishing delete `Corrupt`. The symlink and the aside bytes stay. The count stays 1. Observation is not `Ok(true)`. The new reservation stays 64, so another begin is `Quota`. The released reservation is not written back onto the stuck object.

SIGKILL of this review's own reaper child, after the checkpoint contains `phase quota_released` and `control/waiting` exists, is signal 9. Stdout does not contain `"deletion_success":true`. Restart exits 0 with `"deletion_success":true`, `"restarted_from_partial":true`, `"parent_complete":false`, and `"absence_verified":true`. The count stays 1 on replay and on a second reap. The checkpoint file still has `absence_verified false` and `deletion_success false`.

## Public absence and the seams that stay

`required_integration()` is exactly these seven names. `filehost_completed_object_delete` and `filehost_quota_release_on_completed_delete` are absent from that list. This review does not mark the seven done.

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

A published session-output object is created, split into two parts, completed, and downloaded through FileHost. `abort` of that completed session is `Closed` and the object remains. A wrong-role permit is refused, `bearer_url` is `none`, and `gateway_url_revocation` is `not_implemented`. An analyst pull of the current tuple returns the bytes. A foreign project's uploaded-source registration inside 180 days stays readable, and its quota body is unchanged. After expiry, reap sets `deletion_success` true, `parent_complete` false, observation `Ok(true)`, `read_object` `NotFound`, and release count 1. The tombstone is `phase verified`, `part_count 2`, `release_count 1`. Planted five-digit `00001` is gone. `dest#1` and `notes` remain. `operator-note.txt` and a neighbor file outside the host tree remain. A replay begin gets a new session. A second begin of the same size is `Quota`. A repeated reap stays at count 1.

An open registration that never completes stays `aborted_parts_unverified`. `deletion_success` is false. `parent_complete` is false. Planted `dest#1` remains. The two part and staging seam names stay in `required_integration()`.

A finished download stays `is_complete` after the later delete. A pull that has returned 32 bytes then receives `Store` or `Corrupt`, and `is_complete` stays false. A wrong project and a wrong grant are `Denied`. A wrong digest is `NotFound`. Count stays 0. A flipped object byte makes reap `refused_tamper_or_symlink` and leaves the flipped bytes. A symlink object gets the same refusal and is not unlinked. Replacing the manifest file with a directory keeps `deletion_success` false, `deleted` false, and `absence_verified` false. The object bytes stay. Observation is not `Ok(true)`.

`file_hosting.rs` content sha256 `8804b4eea7463c4853ca159e795eb6edcd2bf53d9bcbb86ece8af98752442627`, git blob `aa16be987070522105b7b4079ed00ef89899b7f3`. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`, git blob `764f182b2a8ccc66347f27fc87a64ba5c7917dca`.
