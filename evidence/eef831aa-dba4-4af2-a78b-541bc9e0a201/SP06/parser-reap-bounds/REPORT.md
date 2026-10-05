# a201 parser reap bounds

Repair of the one-shot parser worker after review `ea2c16df` / a195. Source commit `c27d6349ffa7cf8629a5a79c62015ca5c255e18f` (`gateway: bound parser teardown to the process group`) on parent `90dfac17534983e3f3f93d5f437806193c24095b`. The fixed tree was `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`.

`PARSE_DEADLINE` is still 500 ms from immediately before `spawn`. It now includes the request write, the child's read of the trailing end of file, the reply read, the process-group signal, the direct `wait`, and the join of the IO thread. `RESTRICTION_CAPABILITY` stays `Unsupported`. `UNTRUSTED_ADMISSION` stays `Blocked`. This is not a kernel network, filesystem, or memory sandbox. SP06-R8, the public gateway, and release stay open.

`query_executor.rs` was not edited. Its stale sentence that a 500 ms worker is not implemented is still there. `Cargo.toml` and `Cargo.lock` were not edited. The lock digest stayed `1e9b9377cb2bdf951ab22f7f0a4b68649cb70e563f6ef13adad5852e1c72c657`.

## What ran

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No pull. No PostgreSQL. Private network `oo-a201-eef831aa-net`, volumes `oo-a201-eef831aa-cargo` and `oo-a201-eef831aa-target`. Each container: `--cap-drop ALL`, no-new-privileges, memory 8g, cpus 4, pids-limit 4096, `HOME=/tmp`, tmpfs `/tmp` `noexec`. No docker socket. No host network. No host cargo cache.

The compile tree was a `git archive` of `c22cf08` at `/tmp/oo-a201-eef831aa-dba4-4af2-a78b-541bc9e0a201/archive`. Product edits were copied onto that archive for the proof. The dirty worktree copies of `query_executor.rs`, policy, and `Cargo.toml` were not mounted.

## Pre-edit smoke

`linux/smoke/` on the unfixed archive:

| Phase | Exit |
|---|---|
| libclang extract and load, fetch | 0 |
| `cargo test -p oo-gateway --lib --test parser_worker` | 0 |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` | 0 |
| red harness compile | 101 |

Library tests: 11 passed. `tests/parser_worker.rs`: 27 passed in 1.46s. Container `oo-a201-smoke` then removed. `docker-run.exit` is 101 because the red file indexed a `&str` by `usize` (`E0277` at `parser_worker_a201_red.rs:135`). That is a harness bug. It is not a measurement that `finish` returns.

## Pre-edit hang

`linux/smoke/red-attempt-2/` is the preserved failed probe. Container `oo-a201-red2` (`4305b6899c8b`) exited 143, not OOM, not privileged. The std probe compiled and stopped after `std_io_pipe=ok`. The red harness did not start. The outer 600s wait had killed the launch script. That container was left exited until cleanup.

`linux/smoke/red-attempt-3/` is the next harness failure, also preserved. The probe did not compile (`format` argument mismatch). The fault binary did not compile (`unused import Write` under `-D warnings`).

`linux/smoke/red-attempt-4/` measured the unfixed supervisor. `docker-run.exit` 0. Oracle:

```
case=grandchild-stdout-red hung=1 at_ms=3000 child=170 grandchild=171 alive=1 comm=sleep
case=grandchild-stdout-red unblocked=1 wall_ms=3002 elapsed_ms=3002 signal=Some(9) reaped=1 pid_absent=1 direct=170
```

A grandchild holding the child's stdout blocked `finish` for 3.002s. The direct child was reaped with signal 9 only after the test killed that grandchild. Container `oo-a201-red4` removed after the logs were saved.

## Rust 1.94 std probe

`linux/smoke/red-attempt-4/std-api-probe.txt`, probe `std-api-3`, exit 0:

- `Stdio` has no `From<UnixStream>`. `OwnedFd::from` then `Stdio::from` carried `/bin/cat` bytes `probe-bytes` and the child exited 0.
- `set_read_timeout(Some(Duration::ZERO))` is `InvalidInput`. An 80 ms timeout returned `ErrorKind::WouldBlock` at 94 ms. A full write buffer returned `WouldBlock` after 233152 bytes.
- `/bin/kill` is a regular file, procps-ng 4.0.4. `Child::kill` left the same-group grandchild alive (`grand_alive=1`).
- `/bin/kill -KILL -- -<pgid>` exited 0 and the grandchild was gone. `/bin/kill -s KILL -- -<pgid>` exited 1 and both stayed alive. `/bin/kill -KILL -<pgid>` exited 1.
- `/usr/bin/setsid /bin/sleep` landed in its own process group and was still `sleep` after the group signal.

The product signal is that measured argv: absolute `/bin/kill`, arguments `-KILL`, `--`, and `-<group>`. No shell. The IO thread uses `UnixStream` timeouts of at most 20 ms, refreshed until the deadline, then the parent joins it. The parent does not drop the `JoinHandle` and does not call `abort`. `std::thread` has no abort.

## Proof

`linux/proof/` tests passed (11 lib, 30 parser_worker in 3.44s) and clippy did not run: a fresh container does not keep the smoke container's rustup clippy component. `linux/proof-clippy/` installed clippy and failed on `clippy::needless_return` at `parser_worker.rs` in `spawn_worker`. That `return` was removed. Both failed logs are kept.

`linux/proof2/` is the final tree, blob `59c1518d5d2941f04e2c51ec1585de1e9e57c283`:

| Phase | Exit |
|---|---|
| clippy component add | 0 |
| `cargo test -p oo-gateway --lib --test parser_worker --locked --offline` | 0 |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` | 0 |
| docker | 0 |

11 library tests passed. 30 `parser_worker` tests passed in 3.34s, including `deadline_covers_blocked_stdin_stdout_partial_and_late`, `same_group_grandchild_is_reaped_inside_one_second`, and `escaped_session_is_not_reaped_and_the_parent_still_returns`. `Cargo.lock` hash matched. A scan for the token `unsafe ` in the three Rust files was empty. Comments still say this crate does not use `unsafe`. Container `oo-a201-proof2` exit 0, not OOM, not privileged, `CapDrop ["ALL"]`, memory 8589934592, then removed.

The new tests assert the bounds. They do not print each elapsed time. What they require, and what passed:

- Same-group grandchild holding stdout: `TimedOut`, signal 9, direct pid reaped, grandchild start time gone, no `/proc` entry left in that group, wall under 1s, neighbor `/bin/sleep` start time unchanged.
- Same-group grandchild with stdout null: same reap.
- `setsid` sleep: parent `TimedOut` in under 1s, escaped pid still alive in a different group, then the test signals only that pid. Neighbor start time unchanged.
- Blocked stdin (encoded request longer than 300000 bytes), hold-stdout, and a 4-byte partial reply: `TimedOut`, signal 9, elapsed under 1s.
- Late frame (sleep 700 ms, then write): `TimedOut`, signal 9, elapsed at least 500 ms and under 690 ms.
- Direct `oo-sql-parser-worker` with a request frame plus `0x7f`: exit 1, empty stdout. `decode_frame` returns `Trailing`.
- Existing slow, crash, oversize, malformed, echo, cancel, and next-child cases still pass. `admission_stays_blocked_and_the_sandbox_is_unsupported` still expects `Blocked` and `Unsupported`.

## Limits

A descendant that has left the child's process group is not killed. The socket timeout is what lets the parent return. Inherited descriptors other than stdin, stdout, and stderr are not closed. `/proc` gone for the leader still signals the recorded group id, so pid reuse is not prevented. No namespace, landlock, seccomp, rlimit, or `runsc` is installed. There is no new dependency.
