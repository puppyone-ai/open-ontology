# REPORT — a311 local artifact retention

Verdict: `parent_complete` is false. Linux proof-6 exited 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a311`. Fixed commit `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. Owned paths are `tools/artifact-retention/**` and this evidence directory. Contract `oo.artifact-retention/v1`. FileHost contract `oo.control.local-file-hosting/v1`.

This resume found proof-6 already finished. No second builder and no PostgreSQL were started. At collection, container name `oo-a311` was absent, host compilers `cargo`/`rustc`/`rustdoc`/`rustup`/`clippy-driver` were 0, and the labeled rust-builder list was empty. Network `oo-a311-eef831aa-net` and volumes `oo-a311-eef831aa-cargo` and `oo-a311-eef831aa-target` were still present until `cleanup-done.txt`.

Worktree HEAD at this handoff is `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. The fixed commit is an ancestor. `linux/proof-6/repo-head.txt` records `773c461104729d6c289eae8640c7c2a28207c447` at the proof launch. `git diff --stat cab5e8e219d1924070df7336f5ab5d4ce7cf40dc HEAD -- Cargo.toml Cargo.lock crates/oo-control crates/oo-core crates/oo-jobs` is `Cargo.toml` +7. Those lines are the unapproved workspace pins for axum 0.8.9, tower 0.5.3, and tower-http 0.6.11 from `773c4611`. `Cargo.lock` and `crates/oo-control/src/file_hosting.rs` match the fixed commit.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder, 6 GiB, 1 CPU, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, pids 4096, tmpfs `/tmp` `nosuid,nodev,noexec,size=512m`, `HOME=/tmp`, private `CARGO_HOME` and `CARGO_TARGET_DIR`. PostgreSQL was not started. Hash oracle is `shasum` and `git hash-object`. Product tests and the lock oracle are Safe Rust. No Python oracle.

Recipe origins, with the hashes recorded in `linux/pre-edit/recipe-origin.txt`:

- a204 launch `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. Memory 8g to 6g, CPUs 4 to 1, global cap 6 to 10, owner a311, package `oo-control` `file_hosting`.
- a200 launch `a587031c719a484830d78e493605f48e2056cae4`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. Postgres omitted.
- a199 launch `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh`, sha256 `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537`. No Python hashlib.
- a209 smoke shape `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a209/SP05/download-eof-review/pre-edit/inside.sh`, sha256 `b5cf6e162266a3365bd7512378cc43de36b005b16c557b1a29d7d65f00245aa9`.

The fixed-archive build moves `rust-toolchain.toml` aside so the image compiler 1.94.0 is the one that runs. The file at the fixed commit names channel 1.98.1.

## Smoke, before the overlay

`linux/pre-edit/docker-run.exit` is 0. `phases/worst.exit` is 0. Console ends `smoke=pass`. Clippy on `oo-control` finished in 20.52s. This resume did not rerun it.

## Attempts kept

| Run | docker-run.exit | What the console shows |
|---|---|---|
| proof-1 | 126 | `/inside.sh: line 95: /tmp/lock_from_baseline: Permission denied`. `/tmp` is `noexec`. |
| proof-2 | 101 | Lock gate passed (`lock_method=metadata-offline`, `registry_pins=185`, `packages=189`). Compile failed: `control_dir` is private (`E0624`) and `GenericArray::as_slice` is deprecated. `phases/test.exit` 101. Clippy did not run. |
| proof-3 | 101 | `lock_method=prebuilt-locked`. Tests failed to compile: `CARGO_BIN_EXE_artifact_retention` unset, `Deployment` has no `Debug`, borrow of moved `parent`. |
| proof-4 | 101 | 8 live tests and 1 lock-subset test passed. Clippy `-D warnings` reported 6 errors. `phases/test.exit` 0, `phases/clippy.exit` 101. |
| proof-5 | 101 | Same 9 tests passed. Clippy failed on one `collapsible_if` in `tests/live.rs`. `launch.sh` lines 39–43 have an unclosed quote; `recipe-origin.txt` kept the leaked `echo` lines. The container still wrote `console.log`. |
| proof-6 | 0 | Tests and clippy passed. `phases/worst.exit` 0. Console ends `proof=pass`. |

Root `Cargo.lock` and `file_hosting.rs` phase checks are 0 on proof-2 through proof-6. Failed consoles were not rewritten.

## Green proof

`linux/proof-6`. Container `oo-a311-proof-6`, id `edca8ca56246a3f0467fdd8d792f4f8a280f304dc0188d3da4be6afce12f596d`, removed by the launch script (`container-removed.txt`). `hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a311-eef831aa-net`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1`. Network `internal=false` because the image still downloads the rustfmt and clippy components. `lock-method.txt` is `prebuilt-locked`. Check line: `check_ok registry_pins=185 packages=189`.

`cargo test --locked --offline` finished the test profile in 0.56s. Library unit tests 0, binary unit tests 0, doctests 0. `tests/live.rs` 8 passed in 0.11s:

- `current_tuple_refuses_the_pull_without_a_url`
- `guessed_paths_and_symlink_roots_are_refused`
- `kill_between_metadata_and_object_step_stays_partial`
- `open_abort_does_not_claim_part_absence`
- `production_clock_matches_the_process_clock`
- `published_reap_does_not_fake_absence_and_keeps_quota`
- `tamper_drift_symlink_and_read_error_do_not_delete`
- `ttl_classes_stay_distinct_and_workspace_defaults_off`

`tests/lock_subset.rs`: `registry_pins_are_a_subset_of_the_root_lock` passed. `phases/test.exit` is 0.

`cargo clippy --locked --offline --all-targets -- -D warnings` finished in 0.36s. `phases/clippy.exit` is 0.

Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70` matches `linux/proof-6/Cargo.lock.final`. Root lock file sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`, git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. `file_hosting.rs` git blob `8c32a4feaf69d9a445a5839ba2a01982c994936b`, content sha256 `05c9d4d7e2888c40d9de848021f3452968671ae0a3ea04ca58a13f147c49b38b`. The proof-6 snap under `/tmp/oo-a311-eef831aa-dba4-4af2-a78b-541bc9e0a311/proof-archive-6/tools/artifact-retention` matched `tools/artifact-retention` before cleanup.

## What the live tests measured

The reaper registers objects only from `FileHost` receipts. A guessed path or id is refused. The deployment root's final component must be the owner UUID, and a symlink root is refused. Uploaded-source, session-output, transcript, quarantine, and eval caps stay distinct. Workspace snapshot 24h stays off unless the caller sets the explicit flag.

A published object past expiry calls `abort`, receives `Closed`, and stays readable through `read_object`. Status is `unsupported_completed_object_delete`. `deleted`, `absence_verified`, `deletion_success`, and `parent_complete` stay false. Replay `begin` returns the same session. A second `begin` on the filled project is `Quota`. An unregistered object in another project survives. The crate does not unlink store files or edit quota rows.

A current-tuple mismatch refuses the pull. `bearer_url` is `none`. `gateway_url_revocation` is `not_implemented`. Hash, version, symlink, and directory-manifest drift do not call `abort`. An underlying read error is not reported as absence.

An open session can `abort`. The public API still cannot prove part absence, so the status stays `aborted_parts_unverified` and success stays false. Killing the process after checkpoint `metadata_api_done` and before the object step leaves a partial run. The restart does not report success. The CLI has no `--now` flag; that argument exits 1. The library test clock is `Now::Explicit`. The CLI uses `Now::Production`.

## Remaining integration

`required_integration()` returns these seams. They are the host and gateway work still required. This directory does not mark them done.

- `filehost_completed_object_delete`
- `filehost_quota_release_on_completed_delete`
- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

## Pins and Safe Rust

Standalone workspace. Path dependency `oo-control` `=0.1.0`. Registry pins `sha2` `=0.10.9` and `tokio` `=1.53.1`. The child lock is a checksum subset of the fixed root lock: 185 registry checksums, 189 packages. Proof-2 produced it with one `cargo metadata --offline` after a full-pin superset, then the checksum gate. Later proofs use `--locked` only. Root `Cargo.toml` and `Cargo.lock` were not edited by this task. a313 remains the root-lock writer. a207 remains the connector-manifest writer.

`source/rust-inventory.txt` lists every owned `.rs` file. `forbid(unsafe_code)` is on the library crate, the binary, both integration tests, and `lock_from_baseline.rs`. The single `unsafe` hit in each of those files is that attribute. The library modules have zero `unsafe` hits. `Cargo.toml` also sets `unsafe_code = "forbid"`.

Filesystem mtimes: `CONTRACT.md` `2026-10-06T12:23:26Z`, `src/lib.rs` `2026-10-07T10:46:53Z`, `tests/live.rs` `2026-10-07T10:51:29Z`. The contract file on disk is older than the implementation files the proofs compiled. The git history splits that file into its own commit before the implementation commit.

## Cleanup

`cleanup-done.txt` is the command log for network `oo-a311-eef831aa-net`, volumes `oo-a311-eef831aa-cargo` and `oo-a311-eef831aa-target`, and `/tmp/oo-a311-eef831aa-dba4-4af2-a78b-541bc9e0a311`. Nothing is pruned. Foreign containers stay. Evidence directories, including proof-1 through proof-5, stay.
