# Findings

Source `c27d6349ffa7cf8629a5a79c62015ca5c255e18f`. Base `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`. Review repaired: a195 full reports, commit `ea2c16df`.

## F1. The unfixed join hung

a195 F5. `finish` signaled only the direct pid and then joined the IO thread. A grandchild that inherited stdout kept that read blocked.

Measured again on the unfixed archive before this edit (`linux/smoke/red-attempt-4/oracle.txt`): hung at 3000 ms, grandchild 171 comm `sleep` alive, then after the test killed that pid the supervisor returned `TimedOut`, signal 9, elapsed 3002 ms, direct pid 170 reaped.

Repaired in `stop_child` (`parser_worker.rs` `kill_owned_group`, around line 817) and `exchange` (around line 957). The group signal is `/bin/kill -KILL -- -<group>` from `GROUP_KILL_BIN`. The IO thread arms `UnixStream` timeouts of at most `IO_SLICE` (20 ms) and returns `IoOutcome::Deadline` when the clock has fired. `finish` still joins that thread. It does not detach it.

`linux/proof2` `same_group_grandchild_is_reaped_inside_one_second` passed: wall under 1s, signal 9, grandchild gone, group empty, neighbor sleep unchanged.

## F2. Request trailer was accepted by the child

a195 F6. `decode_frame` rejected a long buffer. `read_frame_from` did not require the next read to be end of file. A direct spawn with a trailing `0x7f` could exit 0.

`read_following_eof` (around line 1200) now requires `Ok(0)` or `UnexpectedEof`. Any other byte is `CodecError::Trailing`. `worker_main` returns 1 and writes no guard reply. `direct_binary_matches_the_guard_and_rejects_a_non_request` passed on that input.

The parent's write uses the same deadline. A request that does not finish, or a child blocked before it sees end of file, becomes `TimedOut` rather than an unbounded read. The blocked-stdin case in `deadline_covers_blocked_stdin_stdout_partial_and_late` passed with elapsed under 1s and at least 500 ms.

## F3. Group teardown is not a sandbox

`RESTRICTION_CAPABILITY` remains `Unsupported`. `UNTRUSTED_ADMISSION` remains `Blocked`. The admission test passed.

The std probe and `escaped_session_is_not_reaped_and_the_parent_still_returns` agree: `/usr/bin/setsid /bin/sleep` leaves the grandchild's group, the group signal does not kill it, and the parent still returns inside the deadline because the socket wait ends. The test then signals only that pid. A neighbor sleep keeps its start time.

`group_is_owned` refuses group 0 and 1, the parent's own group, and a leader whose `/proc` process-group id is not its pid. If `/proc` no longer names the leader, the recorded id is still signaled. That does not make pid reuse safe.

No `unsafe`. No new crate. `Cargo.lock` unchanged. `query_executor.rs` unchanged, including the stale "500 ms worker is not implemented" comment.

## F4. Harness failures kept

| Attempt | Result |
|---|---|
| `linux/smoke/` red phase | harness `str` index, exit 101. Smoke tests and clippy had already passed. |
| `linux/smoke/red-attempt-2/` | container `4305b6899c8b` exit 143. Probe hung after `std_io_pipe=ok`. Red test did not start. |
| `linux/smoke/red-attempt-3/` | probe format error and unused import in the fault binary. |
| `linux/proof/` | tests passed, clippy component absent in a fresh container. |
| `linux/proof-clippy/` | `needless_return`, fixed before `linux/proof2/`. |

`linux/proof2/` is the passing test and clippy record for the committed blobs.
