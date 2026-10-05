# Findings

Verdict: **APPROVE_LIMITED**

Source `c27d6349ffa7cf8629a5a79c62015ca5c255e18f`. Line numbers are that commit. The decisive oracle is `linux/overlay/oracle/oracle.txt` from container `5d5d5700621af76e815031d324f71e2759b28fe4de13bfc63020a7918f6654d5`. An earlier passing oracle with the same assertions is preserved in `linux/overlay-attempt-2/oracle/oracle.txt`.

## F1. Contract line 15 describes a comment this tree does not contain

`contracts/gateway/parser-worker-v1.md:15` says `query_executor.rs` is unchanged and that its `UntrustedAdmission::Blocked` comment still says a 500 ms worker is not implemented. The constant, it says, stays `Blocked`.

`query_executor.rs` is outside this commit's diff. At this commit the module docs at lines 7-9 say the parser worker exists, its kernel sandbox is unsupported, and this crate has no authentication gate, so `UNTRUSTED_ADMISSION` is `Blocked`. The enum docs at line 105 say the same. The constant at line 110 is `Blocked`. A search of that file for `No 500 ms process-isolated parser worker is implemented.` finds no match.

The decisive oracle records `contract_claims_stale=1`, `executor_has_stale_sentence=0`, `executor_names_unsupported=1`. The admission test asserts `RESTRICTION_CAPABILITY` is `Unsupported` and `UNTRUSTED_ADMISSION` is `Blocked`.

The behavior under review matches the deadline, the group signal, and the closed admission constant. The cross-reference in the contract is stale. That is why this approval is limited. The constant stays the public gate. SP06-R8 and the public gateway stay open.

## F2. The 500 ms clock includes the group signal and the IO join

`PARSE_DEADLINE` is 500 ms (`parser_worker.rs:57`). `spawn_worker` stores `Instant::now()` immediately before `spawn` (`parser_worker.rs:550`). `supervise_with` runs `on_start`, then sets `deadline = watch.started + PARSE_DEADLINE` (`parser_worker.rs:518-520`). `poll_exchange` checks cancel, then the deadline, then `recv_timeout` of at most 10 ms (`parser_worker.rs:573-590`). `finish` calls `stop_child`, then joins the IO thread, then builds `ProcessReport.elapsed` from `watch.started` (`parser_worker.rs:774-787` and `:869`). `stop_child` calls `kill_owned_group` and then `Child::kill` and `wait` (`parser_worker.rs:802-808`).

`IO_SLICE` is 20 ms (`parser_worker.rs:60`). `arm_read` caps the socket timeout at `IO_SLICE` and at least 1 ms (`parser_worker.rs:1102`). The parent keeps the `JoinHandle` and joins it. There is no `abort`.

The review ceiling is 530 ms: the 500 ms deadline, one 20 ms slice, and one 10 ms poll. Fresh decisive measurements, all `hung=0`, signal 9, reaped, pid absent, `pid_reused=0`, `join_returned=1`:

- Blocked stdin, encoded request longer than 300000 bytes: elapsed 501 ms, wall 503 ms.
- Hold-stdout after a one-byte body: elapsed 503 ms, wall 506 ms.
- Partial reply of four bytes `OOSW`: elapsed 507 ms, wall 508 ms.
- Late writer sleeps 700 ms and then writes: elapsed 502 ms, wall 503 ms, under 690 ms.
- Same-group `/bin/sleep` with inherited stdout: elapsed 505 ms, wall 505 ms. Grandchild pid recorded, absent afterward, group empty.
- Same-group `/bin/sleep` with null stdout: elapsed 503 ms, wall 505 ms. Same absence check.
- Crash `exit(99)`: elapsed 2 ms, wall 2 ms, exit code 99, empty signal, `direct_wait=1`.

The largest decisive elapsed time is 509 ms, on the `setsid` row in F5. Attempt 2's largest elapsed time was 507 ms. Both are inside 530 ms and far inside the 2 s watchdog. The join is bounded.

## F3. Legal codec, trailer, and the production flags

The proof catalog is project `yard`, role `oo_r_clerk`, policy `policy-a215-yard`, relation `yard.bin` to `_oo.pt_bin`, columns `bin_id` OID 23 and `zone` OID 25, `count` OID 9101 with `aggregate_star`, operators 9102/9103 on OID 23 and 9104/9105 on OID 25.

Three statements matched in-process `guard_select`, including both SHA-256 fields and `PINNED_PARSER_VERSION` 170007. Executed SQL contains `pt_bin`. Star expansion is `bin_id`, `zone`. `count(*)` binds function 9101 and operator 9102. Walls were 2 ms, 1 ms, and 1 ms.

`SELECT bin_id FROM yard.bin WHERE zone = 'canary_secret_sql_a215'` with `identity.project` set to `other-yard` returns `ParserResponse::Refusal` equal to `GuardError::IdentityMismatch`. Display and debug omit the canary. Wall 1 ms.

`read_frame_from` reads one frame and then `read_following_eof` (`parser_worker.rs:1176-1210`). `Ok(0)` and `UnexpectedEof` accept the frame. Any other byte is `CodecError::Trailing`. `decode_frame` of a request frame plus `0x7f` is `Trailing` (`parser_worker.rs:1167`). The production binary `oo-sql-parser-worker`, environment cleared, writes a decoded reply and exits 0 on a clean frame. The same frame plus `0x7f` exits 1 with empty stdout. Wall 7 ms. `worker_main` returns 2 when any argument is present or `OO_PARSER_WORKER_SLOW` is set (`parser_worker.rs:714-718`). Both paths exited 2 with empty stdout.

## F4. Process identity on the trusted group signal

`GROUP_KILL_BIN` is `"/bin/kill"` (`parser_worker.rs:64`). `kill_owned_group` runs that program with `-KILL`, `--`, and `-{leader}` (`parser_worker.rs:821-824`). The image file `/bin/kill` is a regular file, ELF (`7f 45 4c 46`), length 67824, and `symlink_metadata` says it is a file.

`group_is_owned` (`parser_worker.rs:832-846`) refuses a leader at or below 1, a leader equal to the parent's own group, a `/proc` pgrp at or below 1, a pgrp equal to the parent's group, and a leader whose `/proc` pgrp differs from its pid. The live fault children used for the slow, hold, partial, late, orphan, and `setsid` cases were checked while alive: process group equal to the pid, empty `environ`, `exe` equal to the approved path, and cwd name starting with `oo-parser-worker-`. The fast production plans and the crash child skip that `/proc` race. The crash report still carries `start_ticks`.

A neighboring `/bin/sleep 40` was started in the test's own process group. Its start time, pid 1's start time, and the test process start time were unchanged after the timeouts and after the direct signal of the escaped pid. The neighbor's `Drop` calls `Child::kill` on that pid only.

The `_ => true` arm at `parser_worker.rs:844` still signals the recorded group when `/proc` no longer names the leader. Contract line 29 states that reuse of that id is not prevented. The oracle records `pid_reuse_residual=recorded_group_signaled_when_proc_leader_absent`. The measured rows have `pid_reused=0`. This review did not build a pid-reuse collision.

## F5. Same-group grandchild and the escaped `setsid` session

Same-group grandchildren are killed with the recorded group. The decisive rows name grandchild pids 283 and 288. `wait_absent` then sees no start time, and `group_members` of the leader is empty. The neighbor start time is unchanged. Elapsed times are 505 ms and 503 ms.

`setsid` is required. The fault child runs `/usr/bin/setsid` or `/bin/setsid` on `/bin/sleep 25` with stdout inherited, and records that child pid. The decisive row is escaped pid 275, child pid 274, elapsed 509 ms, wall 509 ms, `hung=0`, signal 9 on the direct child. The escaped comm is `sleep` or `setsid`, and its process group differs from the fault child's pid. It was alive after `supervise_with` returned. The test then signaled that positive pid only. `wait_absent` then saw it gone. The neighbor, pid 1, and the test process kept their start times.

Contract lines 11 and 31 state this limitation: a descendant that has left the group is not killed, and the socket timeout still lets the parent return. The measurement matches that sentence. It is an unsupported capability, and it is the boundary of this approval.
