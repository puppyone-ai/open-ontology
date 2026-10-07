# REPORT — a342 filehost retention complete review

Verdict: **APPROVE_LIMITED** for `filehost_part_absence_observation` and `filehost_dest_staging_reap`. **WITHHOLD** the parent FILE-D03 / E41 child, SP05-W06, AT-FILE-05, AT-SEC-03, and these five `required_integration` names: `filehost_object_version`, `filehost_admission_role_and_policy`, `gateway_authenticated_url_revocation`, `formula_safe_csv_excel`, `sessions_outputs_prefix`. `parent_complete` is false. This directory does not edit the ledger, does not add a countable unit, and does not accept a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a342`. Fixed archive `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`, tree `65f7522104dd803732d9f19d076cb53a98fb3701`. Product ancestor `def1f7eea53794449d3975b1987d700727cb7d60`. Addendum ancestor `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81`. Both are ancestors of the fixed commit. `file_hosting.rs` blob `8943ac612ca39be64440d8301dd12b48b5d39976`, sha256 `f37377cb0bc359c84c2efb1bce55a69d429770e334bec9544906daca49e6e5ce`. Addendum blob `3a9de3efdca24ec03b3b4e197a1119f2afa8a2cd`, sha256 `5e09df2722f3590d3761540a9f7400e2dce9658008571065592537cd0f0f8cb1`. Root lock blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`. Child lock blob `764f182b2a8ccc66347f27fc87a64ba5c7917dca`. Those bytes match the a338 post-edit-4 record. This review did not edit product source.

`linux/preoverlay/repo-head.txt` and `linux/review/repo-head.txt` are both `6e7dfd3fbacde02d4dab598dff3b8178622424ef`. The containers built `git archive` of `cd8f754`, with `rust-toolchain.toml` moved aside because that commit names channel 1.98.1. Workspace HEAD is a later commit from other actors. The proof is the archive, not that HEAD.

Author record: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete`. Historical review: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review`.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder at a time, 6 GiB, 1 CPU, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, pids 4096, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=512m` on the proof containers, `HOME=/tmp`. Private volumes `oo-a342-eef831aa-cargo` and `oo-a342-eef831aa-target`. Network `oo-a342-eef831aa-net`, `internal=false`, because the empty cargo volume and the image still needed crates.io plus rustfmt and clippy. PostgreSQL was not started. The pinned Postgres image was not used. Hash commands are `shasum` and `git hash-object`. Tests, the lock oracle, and the review overlay are Safe Rust. No Python oracle.

Before the first launch, Docker Desktop's linuxkit VM had already crashed and the engine socket was gone. The app was quit and reopened. Factory reset was not used. The pinned image was still local. Both proof containers ran after that restart.

Recipe origins are in `linux/preoverlay/recipe-origin.txt` and `linux/review/recipe-origin.txt`:

- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`.
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. Postgres omitted.
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, sha256 `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537`.
- a209 smoke shape sha256 `b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9`.
- a323 proof-3 launch sha256 `8448a0df6061ac1faf7a17a06de9ca549cc9a413d6c705f65b91d114bb1b2c01`. The proof-3 copy-back of formatted sources was not reused.
- a336 preoverlay launch sha256 `a9a50886591cfbcf4d63ac30587f4a3b0c9bc9dd266d38f2fd1da16bc17c39f9`. a336 review launch sha256 `f8b12c53b217f86f5686917976e07013e0c25b94ce2d8f3682002acff17f26d2`.
- a342 preoverlay launch sha256 `f343d63d2c0776530b4166ee09285cbb8c3a0120032bf36a68f44790c296e5b4`, recorded by the review launch.
- a311 lock oracle copied byte-for-byte. sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`.

Preoverlay change: archive `cd8f754` itself, no review overlay, no copy-back, full `oo-control` lib, extra unchanged checks for `file_hosting.rs`, both contracts, the addendum, `live.rs`, `reaper.rs`, and the retention lib. Review change: copy `review_independent.rs` into the snap only, reuse the preoverlay network and volumes, no copy-back.

## Preoverlay, before the review overlay

`linux/preoverlay/docker-run.exit` is 0. Console ends `preoverlay=pass`. Container `oo-a342-preoverlay`, id `54c759446cfe31e070472b0149ffc040cff725a270f52a819ccf9fac75f86637`, removed by the launch script. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a342-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Lock line: `check_ok registry_pins=185 packages=189`.

`cargo test --locked --offline -p oo-control --test file_hosting`: 16 passed in 36.71s. `phases/file-hosting-test.exit` is 0.

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

`cargo test --locked --offline -p oo-control --lib`: 6 passed in 0.03s. `oo-control` clippy `-D warnings` exited 0.

- `file_hosting::delete_without_lock::new_returns_unsupported_before_store_io`
- `file_hosting::retention_completion::abort_reaps_platform_multipart_staging`
- `file_hosting::retention_completion::legacy_quota_released_reconciles_without_a_second_release`
- `file_hosting::retention_completion::mismatched_quota_body_writes_nothing`
- `file_hosting::retention_completion::quota_stays_until_bytes_are_absent`
- `file_hosting::retention_completion::symlink_staging_keeps_the_quota_and_writes_no_tombstone`

`cargo test --manifest-path tools/artifact-retention/Cargo.toml --locked --offline`: library 0, binary 0, doctests 0, `tests/live.rs` 14 passed in 0.24s, `tests/lock_subset.rs` 1 passed (`registry_pins_are_a_subset_of_the_root_lock`). Retention clippy `-D warnings` exited 0. `phases/worst.exit` is 0. The snap refused `review_independent.rs` before those tests. Unchanged checks for the root lock, `lib.rs`, `file_hosting.rs`, the control manifest, the child lock, both contracts, the addendum, `CONTRACT.md`, `live.rs`, `reaper.rs`, the retention lib, and a sampled foreign `oo-builder` lib are 0.

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

## Attempts kept

| Run | docker-run.exit | Container id | What the console shows |
|---|---|---|---|
| preoverlay | 0 | `54c759446cfe31e070472b0149ffc040cff725a270f52a819ccf9fac75f86637` | 16 + 6 + 14 + lock subset, both clippy gates 0. Console ends `preoverlay=pass`. |
| review-attempt-1 | 1 | `7800385e62478060eeb67652582ada1bcea7f8a0d3f778d8c44cabfe563bbd6c` | `rustfmt --edition 2024 --check` printed a diff of `review_independent.rs` only. `phases/fmt-check.exit` is 1. Product sources were not rewritten. |
| review-rustfmt | 0 | removed; `hostconfig.txt` records memory and CPU | Pinned `rustfmt --edition 2024` rewrote the oracle. See `note.txt` for the earlier PATH miss. |
| review | 0 | `ee43c955b3fd793ba3cd4afbff442bbf765158d43fb77f1a9e1aca6228bc200d` | Four independent tests passed in 0.19s. Retention clippy exited 0. Console ends `review=pass`. |

Failed consoles were not rewritten. Named containers were removed by their launch scripts or by the format helper.

## Independent review

`linux/review`. `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a342-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Overlay sha256 at the passing run: `2373a5e20a86fcac2c8d89135982310f4bf6ab512cbb8ced2ff7e72c9f4901b1`. The unformatted overlay sha256 is `8159d29c85df73cbfd7fd83b05c222b9ced9d34a88c0c4afc8d6fda636c37dd2`.

`cargo test --locked --offline --test review_independent -- --nocapture --test-threads=1`: 4 passed in 0.19s. `phases/test.exit` is 0. Retention clippy `-D warnings` exited 0. `phases/worst.exit` is 0. Source-unchanged phases, including `file_hosting.rs`, author `live.rs`, `reaper.rs`, both contracts, and the addendum, are 0. The review did not rerun the 16 + 6 + 14 suite.

- `completed_kills_hold_quota_until_one_release_and_reuse_the_pointer`
- `open_reap_drops_canonical_staging_and_checks_the_receipt_grant`
- `open_receipt_window_restarts_once_and_rejects_a_forged_grant`
- `symlink_store_error_and_wrong_tuple_keep_the_reservation`

Console lines from the receipt-window test:

```
receipt_window observe_while_quota_held=true
notfound_replant observe=true quota_still_present=true
```

`HostLimits::try_new(64, 64, 10, 64)`. SIGKILL of this review's own reaper child at checkpoint `phase data_deleting` is signal 9. Stdout does not contain `"deletion_success":true`. The object bytes and quota body `64` remain, the release count is 0, and observation is not true. Canonical `original#1` and `00001#1` are still present. A second 64-byte begin is `Quota`. Restart exits 0 with `"deletion_success":true`, `"restarted_from_partial":true`, `"parent_complete":false`, and `"absence_verified":true`. The count is 1. The host removed the canonical staging files. `original#01`, foreign `dest#1`, and a neighbor outside the deployment root remain. A repeated reap stays at count 1. The same upload config begins a new session and rewrites the same idempotency pointer. Another 64-byte name is `Quota`. Abort of that open session removes the one quota file.

A second published object is then SIGKILL'd at `phase data_absent`. The object read is `NotFound`, the quota body is still `64`, the count is 0, and observation is false. The tombstone contains `phase data_absent`, `release_count 0`, and `order bytes_then_quota`. Another begin is `Quota`. Restart releases once. The first object's count stays 1. A repeated reap stays at 1. The same config begins again, and a further 64-byte begin is `Quota`.

The open reap's first abort path returns a row with status `aborted`, seam `none`, `deleted` true, and `absence_verified` true. `deletion_success` is true and `parent_complete` is false. Canonical `original#1`, `00001#1`, and the five-digit part are gone. `original#01`, `dest#1`, `notes`, and the outside neighbor remain. The receipt contains the session, project, principal, and `grant_hex` equal to both `hex(grant bytes)` and `row.grant_hex`. A wrong grant and a wrong project are `Denied`. A second `abort` is `NotFound`. A second reap is still `aborted` with the same receipt bytes. `required_integration()` is exactly the five names above, length 5.

The receipt window has no pause hook. The test copies the host-written quota, session, and pointer, lets `abort` return `Ok`, then restores those three files. Observation while the quota body is held is `Ok(true)`. A different 64-byte name is `Quota`. The same config resumes the same session and does not create a second quota file. The following `abort` returns `Ok` and removes that one key. The next `abort` is `NotFound`. A wrong grant is `Denied`. The receipt bytes are unchanged.

After the session is already gone, planting only the quota file again leaves `abort` as `NotFound` and the quota body in place, while observation stays `Ok(true)`. That is a limit of the `NotFound` arm. The addendum's open success condition is `abort` returning `Ok` together with observation `Ok(true)`. This review saw that `Ok` on the first abort and on the restored-session abort.

A forged receipt planted before abort makes the reap `deletion_success` false, `deleted` false, and `absence_verified` false. The quota body stays `64`. Canonical staging and the five-digit part are already gone when `write_open_absence` returns `Conflict`. Foreign `dest#1` remains. Observation with the real grant is `Denied`. Observation with the forged grant is `Conflict`. Another begin of the same size is `Quota`.

A wrong digest is `NotFound` and a wrong grant is `Denied`. Both leave the bytes and count 0. A symlink planted on the object makes reap `refused_tamper_or_symlink`. The host leaves the symlink. The aside bytes and the quota body stay. Replacing the manifest with a directory keeps `absence_verified` false, `deleted` false, and `deletion_success` false. The object bytes and the quota body stay. The test removes only the symlink it created after that refusal is recorded, and renames the aside back. That restore is not the reap proof.

A wrong role and policy on the published object returns `allowed` false, reason `role_or_policy_mismatch`, `bearer_url` `none`, and `gateway_url_revocation` `not_implemented`. The object key starts with `uploads/` and ends with `/original`.

## Seams that stay

`required_integration()` on this archive returns exactly the five names listed in the verdict. `filehost_part_absence_observation` and `filehost_dest_staging_reap` are absent from that list. Removing them is not parent acceptance. Plan SP05-W06 still requires the authenticated download proxy, the current permission tuple, unified delete of TTL/version/cache/multipart/temp, CSV/Excel formula escaping, and old URL revocation within 5 seconds. AT-FILE-05 and AT-SEC-03 stay open. Object version, outputs prefix, download URL, and export seams were not exercised as closed work. This review does not shrink the original child.
