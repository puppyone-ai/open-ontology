# a080 P5 IPC correction

Correction of a076 `request_changes` against `efb61d62803073b3f98f5fe1885877824f504f80`. The spike stays in `testing/spikes/p05-pg17-parser`. The ADR stays Proposed. This run does not accept P5, Gateway authorization, or a supply-chain approval.

Builder: `oo-a080-msrv-f669cc94`, id `df7bd8a8024798aa9c4f8d55d75e3521c49907c179e949bc94efdb00b63fa84f`. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0 (85eff7c80 2026-01-15)`. `protoc` absent. One builder. No PostgreSQL fixture. No `DROP`, `set_config`, or `pg_read_file` was sent to a database.

## Baseline, before edits

Immutable spike. `Cargo.toml` `528f979780ff278b39e29eec285a9db3257d9f31c7eda93080527ad670dd5695`. `Cargo.lock` `c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7`. `baseline-src-before.sha256` and `baseline-src-after.sha256` were taken around the smoke. Manifest before and after matched. `apt-get update` under `--cap-drop ALL` exited 100 and was left as that failure.

Commands, from `raw/baseline-phases.sh`, with `RUSTUP_TOOLCHAIN=1.94.0`:

| Command | Exit |
|---|---|
| `apt-get update` (capability-dropped; not part of the cargo script) | 100 |
| `cargo fetch --manifest-path /src/Cargo.toml --locked` | 0 |
| `cargo test --manifest-path /src/Cargo.toml --locked --offline -- --test-threads=8` | 0 |
| `cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks -- --test-threads=8` | 0 |
| `cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline -- -D warnings` | 0 |
| `cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline --features test-hooks -- -D warnings` | 0 |

Baseline counts: default lib 9, corpus 4, frame 2. Hooks added ipc 1. Libclang came from the nine Debian trixie arm64 debs in `raw/debs-sha256.txt`. Those blobs were deleted after the hashes were recorded and are not in the commit.

## What changed

The parent screen rejects a contradictory allow shape and clears `executed_sql`. `syntax_allow == false` with code `ALLOW_SYNTAX` becomes `INVALID_OUTPUT` and stays false. A worker OID claim becomes `INVALID_OUTPUT`. Unknown JSON fields, unknown codes, a deny that still carries SQL, and an oversize allow payload clear SQL. The parent does not call `admit_native` to repair worker output. The worker process still parses a request the parent has already accepted as a frame.

The executable trust root is a caller-supplied SHA-256. The path is opened with `O_NOFOLLOW` on every component, including ancestors. The bytes are copied into a sealed memfd and exec'd via `/proc/self/fd`. A symlink, relative path, hash mismatch, or empty or oversize image is `UNTRUSTED_WORKER`. A missing path or a directory is `SPAWN_FAILED`. The nonce, pid, and starttime bind the session to the spawned process. They do not authenticate worker output. Malformed workers run only through `exercise_untrusted` when the supervisor is built with `test-hooks`. A default build rejects `allow-fault`. The test hook's self-hash cannot return `syntax_allow`.

Lifecycle reads are nonblocking. No branch calls `reader.join` or an unbounded `child.wait`. Readiness is 5 s, parse is 500 ms after ready, cleanup is 500 ms. A per-invocation supervisor sets `PR_SET_CHILD_SUBREAPER` on itself only, records pid/starttime/ppid/pgid/sid, SIGKILLs those descendants, and reaps with `waitpid`. `reaped` is true only when that supervisor exited, the proof says `FINAL 0`, the parent's pre-signal snapshot is gone, the workdir was removed, and the workdir byte sum is within 256 KiB. A scan error is not an empty tree. If `prctl` fails, the code is `CONTAINMENT_UNAVAILABLE` and `reaped` stays false. `RLIMIT_FSIZE` is 256 KiB. `RLIMIT_NPROC` is not used, because it is not enforced for uid 0.

The corpus file `corpus/expected.json` is unchanged. `Cargo.lock` is unchanged (`c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7`).

## Corrected commands and exits

| Command | Exit | Log |
|---|---|---|
| `cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks --test ipc -- --test-threads=1 --nocapture` (run 1, before the ancestor-symlink and wall-ceiling fixes) | 101 | `raw/fix-ipc-run1.log` |
| same ipc command (run 2) | 0 | `raw/fix-ipc-run2.log` |
| `cargo test --manifest-path /src/Cargo.toml --locked --offline` | 0 | `raw/fix-focused.log` |
| `cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks -- --test-threads=1` | 0 | `raw/fix-focused.log` |
| `cargo clippy --manifest-path /src/Cargo.toml --locked --offline --all-targets -- -D warnings` (attempt 1) | 101 | `raw/fix-focused.log` |
| same clippy with `--features test-hooks` (attempt 1) | 101 | `raw/fix-focused.log` |
| both clippy commands again after the lint edits | 0 | `raw/fix-clippy-2.log` |
| ipc command with decision lines printed, then the prints were removed | 0 | `raw/fix-ipc-observed.log` |

Final counts from `fix-focused.log`: default lib 10, corpus 4 (83.28 s), frame 7. Hooks lib 10, corpus 4 (87.49 s), frame 7, ipc 11 (106.02 s). The corpus test asserts `containment=proven` and `worker_identity=pinned` for every spawned case, and `pid=0` with containment not proven for `EMPTY_SQL` and `OVERSIZE`.

Run 1 failed on outer wall ceilings (silent wall 7793 ms, bad-magic wall 2853 ms with `ready_wait_ms=1` and `cleanup_wait_ms=18`) and on an ancestor symlink returning `SPAWN_FAILED`. The symlink open now treats `ELOOP` or a nofollow `fstatat` symlink as `UNTRUSTED_WORKER`. Outer walls include the memfd copy of the worker image, about 2.7 s on this builder. The lifecycle clocks stayed separate. Those ceilings were widened only around the copy cost. The 5 s readiness floor and the 500 ms parse floor were not widened.

## Adversarial observations

From `raw/fix-ipc-observed.log`. `sql=` empty means `executed_sql` was empty. Proof lines are `PID pid starttime ppid pgid sid`, then `FINAL n`. The tests also read `/proc/<pid>/stat` after return and failed if that starttime was still present.

| Mode | code | clocks | proof |
|---|---|---|---|
| contradict (`syntax_allow` false, `ALLOW_SYNTAX`, SQL `DROP TABLE analytics.orders`) | `INVALID_OUTPUT` | ready 0, parse 0, cleanup 12, wall 2729 | `PID 8740 1396210 8739 8740 8408` / `FINAL 0` |
| silent-sleep | `SPAWN_FAILED` | ready 5005, parse 0, cleanup 21, wall 7756 | `FINAL 0` |
| partial | `DEADLINE` | ready 1, parse 509, cleanup 21, wall 3292 | `FINAL 0` |
| bad-magic | `INVALID_OUTPUT` | ready 0, parse 0, cleanup 13, wall 2784 | `FINAL 0` |
| stdout-junk, malformed-json, extra-frame | `INVALID_OUTPUT` | ready 0–1, cleanup 12 | `FINAL 0` |
| oversize-frame | `RESPONSE_CAP` | ready 1, parse 0, cleanup 12 | `FINAL 0` |
| early-exit, premature | `SPAWN_FAILED` | ready 1, cleanup 14–18 | `FINAL 0` |
| hold-setsid-after | `INVALID_OUTPUT` | ready 1, parse 0, cleanup 12, wall 2909 | worker `PID 8778 1401343 8777 8778 8408`; setsid grandchild `PID 8779 1401343 8778 8779 8779`; `FINAL 0` |
| hold-samegroup | `INVALID_OUTPUT` | ready 1, parse 0, cleanup 17 | holder `PID 8782 1401621 8781 8781 8408` (pid differs from pgid and sid); `FINAL 0` |
| hold-setsid-noreply | `DEADLINE` | ready 1, parse 500, cleanup 13 | `PID 8785 1401899 8784 8785 8785`; `FINAL 0` |
| fsize | `RESOURCE_BUDGET` | detail `wrote=262144` | `FINAL 0` |
| boundary | `INVALID_OUTPUT` | detail below | `FINAL 0` |
| consistent-allow | `UNTRUSTED_WORKER` | identity `untrusted-test-hook` | `FINAL 0` |
| oid-true, deny-with-sql, unknown-code, unknown-field, oversize-sql, spoof-pid, spoof-start, spoof-nonce | `INVALID_OUTPUT`, SQL empty | ready 0–1, cleanup 12–13 | `FINAL 0` |
| version-server 170010 | `PARSER_MISMATCH`, SQL empty | ready 1, cleanup 18 | `FINAL 0` |

Boundary detail: `netns=net:[4026532700] path=/usr/bin:/bin home=unset passwd=yes cargo=yes socket=yes nnp=1 fds=0,1,2`. The test required that netns string to equal the parent's `/proc/self/ns/net`. Cleared environment and the temp cwd are not network isolation. `/etc/passwd` and `/src/Cargo.toml` were readable. A TCP socket was created and closed. Nothing was connected. File contents were not copied into the log.

`foreign_sleep_survives_cleanup` spawned `/bin/sleep 30`, recorded pid and starttime from `/proc/<pid>/stat`, ran a contradict worker, asserted the sleep starttime was unchanged, then SIGKILLed that pid only if the starttime still matched. The test passed.

`test_hook_cannot_turn_a_real_allow_into_syntax_allow` sent `SELECT 1` through `exercise_worker` and got `UNTRUSTED_WORKER`, `syntax_allow` false, SQL empty, identity `test-hooks-kind`.

## Source provenance

`licenses/protobuf-c-bsd-header.txt` is now SHA256 `142a052f7d22404fb513ad9f8d03f78f27e797352292c3a60c5c2babcef8285e`. The copyright line is `2008-2018`, matching the compiled header. The previous excerpt hash was `e8185daf5bcd355692052fbdc990ea2d8acef9b4c3a41a32f189f272667e9d6b` and said 2008–2015.

Fetched HTTP 200 on 2026-10-02:

| Source | SHA256 |
|---|---|
| pg_query.rs `LICENSE` at `0e382742b36ee9592438c6791b91ce14f32f6233` | `39fac7e7daa7cf3173d96f9e8a167f980b93b90386b83c342de70db3bc7bbb68` (byte-equal to the spike copy) |
| libpg_query `LICENSE` at `7be1aed1f1f968a36cf541319f71e845850f0381` | `f9352c20940619a380a5e9f23cebe34d8f797cfbd747fc236a7a3e5fc8ef9b41` (byte-equal) |
| PostgreSQL `COPYRIGHT` at tag `REL_17_7` | `5ed3ce5c9373dff7f98b1fae7a6c7ccd98df7d734d46d24c1bcebf1240be8307` (byte-equal) |
| libpg_query `vendor/protobuf-c/protobuf-c.h` at `7be1aed1f1f968a36cf541319f71e845850f0381` | `3c06432ca3233a730e7b20afc1bc5750dd3f8c298db9c99b5df03dc6e1a4740c` (byte-equal to the file in the crates.io crate; the spike excerpt is a prefix) |

The GitHub commit API object for `7be1aed1f1f968a36cf541319f71e845850f0381` says message `Release 17-6.2.2 (#326)`, author Lukas Fittl, author date `2026-01-27T05:39:40Z`, `verification.verified=true`, `verification.reason=valid`. The extracted crate contains no `LICENSE`, `COPYING`, or `COPYRIGHT` file. Copies are in `raw/upstream/`. `xxhash` was not re-fetched. The Bison excerpt is unchanged at `e8ecf341ce94fb7ad19de4a4985374eb821ff84fd4dff943b10a022ca93fc7a3`: GPL-2.0-or-later, and the special exception applies only while the larger work that contains the skeleton is not itself a parser generator built from the skeleton. Changing or shipping the skeleton alone drops the exception. That is not a blanket GPL allow.

`cargo audit` and `cargo deny` were not run. The age floor is unchanged. This is not a full supply approval.

## Fixed, and still open

Fixed here: contradictory allow/code/boolean/version/oid/SQL shapes, including false plus `ALLOW_SYNTAX`; OID claims from the syntax worker; substituted paths and the test-only fault hook; bounded cleanup on the malformed, held-stdout, same-group, and setsid cases; separate readiness and parse clocks; descendant proof with a per-invocation subreaper; foreign sleep left alive; file-size cap; protobuf-c year; upstream license bytes and the libpg_query commit check.

Still open, and not claimed by this correction:

- Supervisor death before the reap. Orphans then go to pid 1. The result is defined as not proven. This run did not kill the supervisor mid-reap.
- No child cgroup and no user or pid namespace in this capability set. `unshare(CLONE_NEWUSER)` returned `EPERM`. The cgroup filesystem was read-only.
- Same network namespace as the parent. `passwd=yes`, `cargo=yes`, `socket=yes`. No gVisor or `runsc` result.
- The pinned hash is not a signature. A caller who hashes an untrusted binary and passes that digest has not created an external root. Path replacement after the open does not change the copied bytes. `memfd_noexec` was 0 in the baseline probe.
- R4 function and OID whitelist. `SELECT set_config(...)` remains `ALLOW_SYNTAX` and is not an ACL proof.
- R5 catalog database permissions.
- R9 `WHERE true` and subquery-wrapper star cases. The corpus codes are unchanged.
- Native parser 17.7 versus server 17.10 grammar parity. The 17.10 fixture was not rerun.
- amd64, and Gateway bind/authorize before execute.
- Full supply approval, age-floor changes, and root policy. None of those were changed.
