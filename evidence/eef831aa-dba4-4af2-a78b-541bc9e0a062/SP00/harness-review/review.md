# a062 fixed-commit foundation review

Decision: `request_changes` for this harness foundation slice.

Reviewed tree only: commit `f61de8d83c3923fee8d98915eac290471841b83f`, tree `968704d558304fc49a1e450db6816363468eecc2`, parent `e271542aa645aa181da7887d416de0698752a805`. The archive was `git archive` of that commit into `/tmp/oo-a062-eef831aa-dba4-4af2-a78b-541bc9e0a062/archive` (283 entries, missing=0, mismatch=0, extra=0). Metadata handoff `9c2ed1d8ea6fc20adf437c694d96e0a4e5a57f90` adds only `HANDOFF.md` and is not the reviewed tree. Moving main, `oo-builder`, and `Cargo.lock` were not signed. No product source was edited.

`phase_signoff` stays `in_review`. This review does not sign SP00, the secret gate, protected CI, release authority, linux/amd64, or the rest of the matrix.

## Linux arm64 Rust 1.94.0

Pinned image `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--platform linux/arm64`, `--user 0:0`, `--cap-drop ALL`, `no-new-privileges`, memory 8g, swap 8g, cpus 6, pids 4096, `--init`, default bridge, `/src` read-only, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=2g`. One owned builder. No a059/a061/a063 Rust builder was running. `RUSTUP_TOOLCHAIN=1.94.0`. `rust-toolchain.toml` in the commit still says channel `1.98.1`.

`rustup component add clippy --toolchain 1.94.0` exited 0. That download is recorded and is not an airgap claim. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. gcc 14.2.0 was already in the image.

`cargo fetch --locked` exited 0. Then:

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test -p xtask --locked --offline -- --test-threads=8` | 0 | lib 25 passed, cli 16 passed, doc 0 |
| `cargo clippy -p xtask --all-targets --locked --offline -- -D warnings` | 0 | finished, no warning output |

Suite container `696b92f412d9` exited 0, OOM false. Logs: `linux/suite-pass/`. sha256 `xtask-test.log` `545a1e7569a1bb10ab7e897a3c010bc5db1912cf2867bb39ac0fe055f71bc19b`, `xtask-clippy.log` `630939c3ba90d34cb40aa18d5269ef3edfe68834abc4d8d758b06486e152593e`.

The author CLI tests that failed under a057 (`execute_local_shim_is_required_satisfied_and_replay_is_not`, `execute_local_failed_child_cannot_seal_a_pass`, `execute_local_source_drift_keeps_results_and_fails_the_seal`, `execute_local_deadline_reaps_the_group_and_spares_a_foreign_process`, `fixture_receipt_executes_outside_repo_only`, `evidence_and_gates_self_test`) passed in this run, along with `execute_local_path_kill_is_not_used`, `execute_local_setsid_descendant_returns_bounded_and_spares_a_foreign_process`, and `execute_local_output_limit_cannot_seal_a_pass`.

Attempt 1 is preserved under `linux/attempt-1/`. `cargo metadata --locked --offline --no-deps` exited 0 on an empty registry, the script skipped fetch, and test and clippy both exited 101 with `no matching package named chrono-tz` before any test ran. That is a reviewer script error, not a product failure.

The first probe build is preserved: `linux/probe-build.log` sha256 `aeb72660876fbe1b64d56e23c6cbc541b4894fa67006360aff99e88fc8b15300`, exit 101, Rust 2024 `set_var`/`remove_var` are unsafe (`E0133`, 26 errors). The rerun probe does not mutate the process environment. It passes adversarial environment with `Command.env` on a child process. `linux/probe-build-rerun.exit` is 0.

## Blocking finding

An unobserved `setsid` descendant stays alive, `cleanup_incomplete` stays false, and `gates --execute-local` still reaches `required_satisfied`.

`execute_command` records a descendant only while the direct child is alive and the descendant's parent is already a member, or while the root is still that process-group leader and the descendant shares the group. `kill_recorded` runs only when the direct child has not already exited. A double-fork that calls `setsid` and whose parent exits before the 20 ms poll is not a member. `confirm` then sees an empty live set and reports cleanup complete. Exit 0 is kept.

Minimal repro, probe exit 0, stdout sha256 `317f1f8260cdfb541769f40aea6b49b65d2b5e41c72a1f06b6909b6b30cf5a18` (`probes/seal-escape.stdout`):

The allowlisted `cargo` was a symlink to a helper that forks, calls `setsid`, execs `/bin/sleep 120`, and whose parent `_exit(0)`. Nine foundation commands ran with `OO_XTASK_GATE_DEADLINE_SECS=30`.

```json
{"alive":9,"cleanup_code":false,"deadline_code":false,"elapsed_ms":60,"execute_exit":0,"foreign_state":"S (sleeping)","recorded":9,"required_satisfied":true,"sample_pid":1200,"sample_ppid":"1","sample_state":"S (sleeping)"}
```

The same helper through `execute_command` alone (`probes/early-setsid.stdout`, sha256 `965bb0254922697407e8f1462ffc5bd4697a67d1d3953b382225e6afb61ae67a`): exit code 0, `cleanup_incomplete` false, `timed_out` false, elapsed 4 ms, descendant 1040 still `S (sleeping)`, ppid 1, NSpgid 1040. Fifteen repeats (`probes/early-repeat.stdout`, sha256 `4ca22be6dfb6479beefe227d5d9789d865d5c18485d0563cde2d54694216aeac`) were 15/15 alive and 15/15 clean exit 0, max elapsed 22 ms. `setsid` from a shell that then `exit 0` (`probes/shell-setsid-early.stdout`) matched: alive, exit 0, cleanup complete, elapsed 2 ms, ppid 1.

The probe killed those sleeps with a direct `kill` syscall after recording them. The container sweep then showed only docker-init, the runner, and `ps`.

This is the hole a055's author tests do not cover. Those tests, and this run's `observed-setsid`, keep the parent in `wait`, so the child is observed and reaped. `observed-setsid` returned in 1019 ms with the descendant absent, the foreign sleep still `S`, `timed_out` true, and exit code -1. `group-early` (`sleep 120 & exit 0`, same group, no `setsid`) left the sleeper absent after 44 ms and exit 0. Same-group reaping of a child that has been seen, and reaping of a `setsid` child whose parent is still waiting, are what this tree actually does. They are not a guarantee that every descendant of the command is dead before a pass can seal.

`testing/harness/README.md` says recorded descendants are signaled by pid and start token, and that survivors are `gate.cleanup`. The seal above has no survivor in the recorded set, so `gate.cleanup` does not fire.

Related listing behavior on a waiting parent (timeout, so exit code -1 and these particular commands would not seal):

- Every `/proc/pid/stat` replaced with `1 (x) S\n` (`listing-malformed`): `cleanup_incomplete` true, descendant still sleeping. Empty parse fails closed.
- Stat lines containing `(sleep)` replaced with a short line (`listing-hide-sleep`): descendant still sleeping, `cleanup_incomplete` false. Omission is not a list failure.
- Second read of a sleep's start token increased by 1000000 (`listing-flip-sleep`): descendant still sleeping, `cleanup_incomplete` false. `live_pids` drops a pid whose start token changed. That drop is the PID-reuse guard. On a live process it also drops the member, so the process is not a survivor.
- `listing-missing` (stat of `/proc/1/stat` and exec of `ps` forced `ENOENT`) did not reproduce an unavailable list. The descendant was absent and `cleanup_incomplete` was false. This run does not claim that a missing process listing fails closed.

`hold-file` is the same early `setsid` exit, with the grandchild still writing the inherited stdout descriptor. Return was 2 ms, exit code 0, cleanup complete, and the process still sleeping. `/proc/<pid>/fd/1` size was 1048576 at return and 9437184 about 2.5 s later. The capture directory is removed before return. The open inode keeps growing. Deadline return is bounded. The 16 MiB cap does not stop that growth.

## What else the probes measured

Inherited-fd case (`elapsed_ms` 1021, descendant absent, foreign sleeping, exit -1). The `ls` of `/proc/self/fd/1` was redirected to a file the probe created, so those listings are not the harness capture descriptor. The file-backed descriptor is the `hold-file` size growth above. The a055 inherited-pipe hang (25 s, descendant still `Ss`) did not recur on the observed-parent path.

PATH no-op `kill` was first on `PATH` (`/target/a062-noop-kill`). `command -v kill` printed `kill`. The no-op log is empty. The same-group sleeper was absent after 1035 ms, exit -1, foreign still sleeping. Absolute kill was used. `/bin/kill` and `/usr/bin/kill` are mode 755, uid 0. `trusted_tool` returned canonical `/usr/bin/kill` and `/usr/bin/ps`, uid 0, mode 755, regular files, not group or world writable, owner executable. Five non-names (`../kill`, `kill/extra`, empty, `-1`, `not-a-real-tool`) were rejected.

Output cap is per stream and is strict greater-than 16 MiB (`16777216`):

| Probe | File high-water | Retained stdout | `output_limited` | Exit code |
| --- | --- | --- | --- | --- |
| `dd` count 16 | 16777216 | 16777216 | false | 0 |
| 16 MiB plus 1 byte | 16777217 | 16777216 | true | -1 |
| `dd` count 32 | 33554432 | 16777216 | true | -1 |

Exactly 16 MiB can seal. One extra byte cannot. The capture file can pass 16 MiB between polls. The retained prefix stops at 16 MiB. This is not a disk quota.

Noexec tmpfs: executing `/tmp/a062-noexec.sh` exited 126, `Permission denied`. `/bin/sh` on that script exited 0. The same script on `/target` exited 0 and printed `RAN`. The product shim test passed with `CARGO_TARGET_DIR=/target`.

Git: a raw `git` child with `Command.env` `HOME=/tmp/a062-poison-home` and `GIT_CONFIG_COUNT=1` / `user.name=COUNTPOISON` committed as `COUNTPOISON poison@example.invalid`. `isolated_git` committed as `xtask xtask@localhost`, created `oo-xtask-git-home-*` directories, left the poison file hash `715f5791537231c46bae881557b8023331966334d09dcd3389d43953647a75a5` unchanged, and `git config --show-origin` did not contain the poison path or name. The probe process `HOME` was the poison directory for the whole case and was not the host home. Host `HOME` was not mounted.

Capture provenance: two `/bin/echo` runs kept argv `alpha-token` and `beta-token` and left no `oo-xtask-capture-*` directory behind.

Imported capture: positive shim `execute-local` exited 0 with `required_satisfied` and `runner_authority` local-engineering. Replay of that capture with `--baseline-source bootstrap` exited 3, contained `gate.unverified`, did not contain `required_satisfied`, and reported `runner_authority` absent. `a062-forge` `serde_json::from_str::<ExecutedCommand>` failed to compile, `E0277` missing `Deserialize`, exit 101, log sha256 `8beecea5d7cd0b5fb0ca53da3490206f5200350a4fdf8aa0b8196e340322a401`. `a062-ctor` struct literal failed, `E0451` private fields, exit 101, log sha256 `e362d442ff8d9c34a76e47591b0946090037e0fea20654226a8b8060b8d7cd22`. Those logs were kept. A private `ExecutedCommand` cannot be deserialized or built outside the crate.

Kill-failure injection did not leave the recorded sleeps alive. `seal-cleanup-enoent` and `seal-cleanup-rc1` each recorded 9 sleeps, finished with `alive` 0, `gate.deadline` set, `gate.cleanup` unset, `required_satisfied` false, foreign sleep still `S`, elapsed about 9.3 s. The hook log prefix in those JSON files is the previous case's `/tmp/a062-hook.log` (it starts with `flip-sleep`), so this run does not prove the `execve` interposition ran. Source `signal_target` still ignores the kill program's exit status. A failed kill of a recorded survivor was not reproduced here. Deadline rejection of this waiting command was reproduced. Foreign processes were spared in every case that started one.

## Still pending

Secret gate, external protected CI, production integration suites, full SP00, linux/amd64, and the other matrix recipes were not run. They stay pending. Unrelated builder work is not blocked on those release conditions. `authenticated_provenance` stays pending. `protected_github_policy`, `release_authority`, and `ci_passed` stay false.

Probe-only container `0008268098ae` exited 0, OOM false, same posture, `A062_SKIP_TESTS=1`. `linux/probe-inspect.txt`. `A062_RESULT test=0 clippy=0 probe_build=0 forge=101 ctor=101 rustup_clippy=0`. Console sha256 `f319caff558aee5b66ffe33a3a9d67f1b006141ef2d58eca1aaa420ec48ce29d`. Per-case stdout sha256 files are `probes/*.sha256`.

After this review was written, cleanup is only this run's container and the UUID `cargo-home` and `target` directories. The archive and probe sources under the UUID temp directory, and this evidence directory, stay. No foreign container, volume, or host home was removed.
