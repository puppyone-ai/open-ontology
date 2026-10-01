# Harness corrections a058

Run `eef831aa-dba4-4af2-a78b-541bc9e0a058`. Scope is the xtask deadline and Linux fixture correction in `execution/prompts/00h-harness-deadline-linux-fix.md`, with the later local-commit authorization in `execution/prompts/00h-harness-deadline-linux-resume.md`. This report is the engineering record. It is not phase sign-off, release authority, or a full four-recipe matrix.

The parent of the baseline commit is `e271542aa645aa181da7887d416de0698752a805`. The baseline SHA is recorded in `HANDOFF.md` after that commit is created. Product worktree `codex/product-pg-source` at `b2d3cbc` was left untouched.

## What changed

`xtask` captures child stdout and stderr in unique files under a private directory, with a 16 MiB memory cap. The parent poll loop owns the deadline and the cap. There is no reader thread and no join waiting on pipe EOF. A child that keeps an inherited descriptor open cannot hold the parent past the deadline.

Termination uses a validated absolute `kill` from `/bin` or `/usr/bin`. The program must canonicalize to that directory, keep the same file name, be a regular file owned by uid 0, and be owner-executable and not group- or world-writable. The caller `PATH` is not consulted. Owned processes are recorded by pid and start token. A descendant that called `setsid` is signaled by that identity when it was observed while its parent was alive. A same-group signal is sent only when the recorded root is still that group leader. A survivor, or a tree that cannot be listed, is cleanup incomplete, forces a zero exit to -1, and emits `gate.cleanup`. The capture directory is not reused.

Git commands used by the self-test and the candidate commit run with a per-command private `HOME`, `GIT_CONFIG_NOSYSTEM=1`, empty global and system config files, and explicit author and committer environment. Workflow copies are mode `0644` before the candidate commit. Tests place executables on a directory outside the repo where exec works. On the matrix mount that directory is `CARGO_TARGET_DIR` (`/target`). Git repositories and capture files may stay on `/tmp`.

## Red archive, before the fix

The pre-fix snapshot remains at `/tmp/oo-env-eef831aa-dba4-4af2-a78b-541bc9e0a058/snapshot`. Consoles are under `red/`.

On the pinned 1.98.1 arm64 image, `/proc/mounts` showed `tmpfs /tmp` with `noexec`. A mode `0755` script on `/tmp` exited 126. The same script on `/target` printed `RAN` and exited 0. Rust spawn of an absolute path on `/tmp` returned `Permission denied (os error 13)`. `cargo test -p xtask` on that snapshot reported `tests/cli.rs`: 8 passed, 5 failed, in 0.73s. The five failures were `execute_local_shim_is_required_satisfied_and_replay_is_not`, `execute_local_failed_child_cannot_seal_a_pass`, `execute_local_source_drift_keeps_results_and_fails_the_seal`, `execute_local_deadline_reaps_the_group_and_spares_a_foreign_process`, and `fixture_receipt_executes_outside_repo_only`. `evidence_and_gates_self_test` passed on this snapshot in this image. A later workspace run of the same snapshot exited 101 with the same five CLI failures and the same self-test pass (`red/workspace-console.log`).

a057 recorded `evidence_and_gates_self_test` failing with `git-object temp candidate commit failed` on all four recipes, on source bytes that matched this pre-fix snapshot. That failure did not recur in the red container here. The self-test still isolates git identity and keeps the child stderr in the issue if a commit fails again.

## Green proof that was actually read

Both green runs used snapshot `/tmp/oo-env-eef831aa-dba4-4af2-a78b-541bc9e0a058/snapshot-green`, 156 files, tree sha256 `5856931ed4ab0656495f2acaea7df431fc95be57a33704a99cea9a2753a2039f`. Flags: `--pull never`, `--user 0:0`, `--cap-drop ALL`, `no-new-privileges`, memory 8g, swap 8g, cpus 6, pids 4096, readonly `/src`, tmpfs `/tmp:rw,nosuid,nodev,size=2g`. Docker mounted that tmpfs `noexec`. Real `cargo` ran the tests and Clippy. No second builder was started while one of these containers was running.

| Check | Handle | Result read from the log |
| --- | --- | --- |
| Host `cargo test -p xtask --locked --offline` and `cargo clippy -p xtask --all-targets --locked --offline -- -D warnings` | `host/test-clippy.log` | lib 25 passed, cli 16 passed, clippy exit 0, `HOST_PROOF_DONE` |
| Linux 1.98.1 arm64 `sha256:218765d808d35e2e3fa1b12b2e1314e2bfdd427a3530de071398553a95743cba` | container `f26d526b72936f9b25d9aa6ec37d3cc0ad6d049a9b6bdc5caa8085f96cedca1a`, started `2026-10-01T19:42:27Z`, finished `2026-10-01T19:42:55Z`, inspect exit 0, OOM false | lib 25 passed, cli 16 passed in 15.18s, `xtask_test_exit=0`, clippy finished in 6.03s, `xtask_clippy_exit=0`, `GREEN_198_DONE`. `kill_group_syntax_exit=0` |
| Linux 1.94.0 arm64 `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` | container `0902c79268b8`, started `2026-10-01T22:26:04Z`, finished `2026-10-01T22:26:44Z`, inspect exit 0, OOM false, `container.exit` 0 | `rustc 1.94.0`, `RUSTUP_TOOLCHAIN=1.94.0`, `rustup_clippy_exit=0`, lib 25 passed, cli 16 passed in 16.31s, `xtask_test_exit=0`, clippy finished in 7.28s, `xtask_clippy_exit=0`, `GREEN_194_DONE` |

The 1.94 `/proc/mounts` line for `/tmp` is `tmpfs /tmp tmpfs rw,nosuid,nodev,noexec,relatime,size=2097152k`. `/target` is ext4 without `noexec`. `/src` is read-only. The 1.98 console's `findmnt` lines were empty, so that console does not itself prove the mount flags. The 1.98 run used the same tmpfs invocation and the same passing CLI set, including the tests that exec a shim.

`kill` in the 1.94 image is procps-ng 4.0.4 at `/bin/kill` and `/usr/bin/kill`. `/bin/kill -KILL -1999999999` exited 0 with empty stderr. `/bin/kill -KILL -- -1999999999` exited 1 with `No such process`. A live `/usr/bin/setsid /bin/sleep 60` whose pgrp equaled its pid was removed by `/bin/kill -KILL -<pgrp>` (`live_after_A=gone`). A concurrent `/bin/sleep 60` outside that group was still present (`foreign_after_A=present`). The product argv is `kill -KILL -PGID`, the form that stopped the live group in this probe. The missing-group exit 0 is a procps quirk. Cleanup still fails closed when a recorded process remains.

These tests passed in the host log and in both Linux consoles:

- `timeout_reaps_a_setsid_descendant_and_spares_a_foreign_process`
- `timeout_reaps_the_owned_group_and_leaves_a_foreign_process`
- `output_over_the_limit_is_truncated_and_fails`
- `failed_child_keeps_streams_and_exit`
- `execute_local_setsid_descendant_returns_bounded_and_spares_a_foreign_process`
- `execute_local_path_kill_is_not_used`
- `execute_local_deadline_reaps_the_group_and_spares_a_foreign_process`
- `execute_local_output_limit_cannot_seal_a_pass`
- `execute_local_failed_child_cannot_seal_a_pass`
- `execute_local_source_drift_keeps_results_and_fails_the_seal`
- `execute_local_shim_is_required_satisfied_and_replay_is_not`
- `fixture_receipt_executes_outside_repo_only`
- `evidence_and_gates_self_test`
- `gate_deadline_rejects_out_of_range_without_spawning`
- `isolated_git_commits_without_the_caller_home`
- `trusted_kill_and_ps_are_fixed_absolute_programs`

The 1.98 image's first `rustc` invocation downloaded two toolchain components because `rust-toolchain.toml` requests rustfmt and clippy. The 1.94 run downloaded the clippy component for 1.94.0. `cargo test` and `cargo clippy` then ran with `--offline` and real cargo. Neither run is an airgap proof.

## Cleanup

Removed only this run's resources after the logs and inspect files were on disk: container `0902c79268b8`, the three labelled volumes `oo-eef831aa-dba4-4af2-a78b-541bc9e0a058-cargo-1_98_1-arm64`, `oo-eef831aa-dba4-4af2-a78b-541bc9e0a058-target-1_98_1-arm64-red`, and `oo-eef831aa-dba4-4af2-a78b-541bc9e0a058-target-1_94_0-arm64-green`, and network `oo-eef831aa-dba4-4af2-a78b-541bc9e0a058-net-build`. The first volume delete of all three names together returned Docker `404 page not found`; each volume then deleted alone with exit 0. A following `docker volume ls` for this run id was empty, and `docker ps -aq` for this run id was empty.

The 1.98 green container `f26d526b7293` had already been removed by the 1.94 launcher after its inspect and console were saved. The red diagnose container had already been removed by the 1.98 launcher. Red consoles and the red snapshot directory are still present. The eleven volume ids in `raw/volumes-before.txt` were still present after cleanup (`foreign_missing=0`). Container `00a1d97d9034` (`oo-3c91753b-5703-4d60-b1ee-917714ec882c-build-1_94_0-amd64`) was still running. No prune.

## Source hashes

| Path | sha256 |
| --- | --- |
| `crates/xtask/src/lib.rs` | `d07585d636be6554328b3215dd0399c9e516dd01a8e48421f18d64f30a9bf98f` |
| `crates/xtask/src/owned_process.rs` | `1f2617120b684de17e8e2d2dc24ed3739644957e4c2b9ad38d892a325762d6db` |
| `crates/xtask/src/runner_capture.rs` | `a37e162c059b3711e205ed7503bf77899bbf046f23881485b676f3dbe17d9abb` |
| `crates/xtask/src/gates.rs` | `d40b544c7aa79f774de45d2bb345505851f60a52c2fa92e23c07eea80be89dea` |
| `crates/xtask/src/suites.rs` | `d10cf64f54d401803af6e9bb4fde02e2237695c4f9a6b2ff511cccec5a23fba8` |
| `crates/xtask/src/util.rs` | `d8734ef3ba64a068f2b6a3cfa0d51fef2e4ab201d6dcc397ec8e275f51b23413` |
| `crates/xtask/src/evidence.rs` | `4738ae94ac1dc16e6b34f08d5b5d603f1a4935511c87863c161cd1cdee40f221` |
| `crates/xtask/tests/cli.rs` | `b082cb6a483370388dc652be73cf3f80a9f56fff6742eecc4c11811b3c62e118` |
| `testing/harness/README.md` | `40259dc5e055a9f23495de92bb8d473b0ed0cb6a45125359ceb60f05df69b2e4` |
| `Cargo.lock` | `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1` |

## Still pending

The secret gate stays red. Full W05 and CI are unimplemented. `phase_signoff` stays `in_review`. `protected_github_policy`, `release_authority`, and `ci_passed` stay false. The focused proof is host plus linux/arm64 1.98.1 and 1.94.0. linux/amd64 and the other two matrix recipes were not run. a049 on its own snapshot stays a recorded failure. No tag, push, or release was made.
