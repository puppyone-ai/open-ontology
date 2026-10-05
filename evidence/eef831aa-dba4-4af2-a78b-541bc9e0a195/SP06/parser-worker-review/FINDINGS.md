# Findings

Verdict: **APPROVE LIMITED**

Source `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`, file `crates/oo-gateway/src/parser_worker.rs` unless another path is named. Line numbers are that commit. The independent oracle is `linux/proof/oracle.txt` from container `a138f389de49`.

## F1. Direct child: kill, reap, and the 500 ms clock

`PARSE_DEADLINE` is 500 ms (`parser_worker.rs:46`). `supervise_with` starts that clock immediately before `spawn` (`parser_worker.rs:525` and `:504`). `poll_exchange` checks cancel, then the deadline, then a short receive (`parser_worker.rs:538-555`). A message that arrives at or after the deadline is a timeout (`parser_worker.rs:556`). `finish` kills, waits, then joins the IO thread (`parser_worker.rs:745-748`). `stop_child` calls `Child::kill` on that child and `wait` (`parser_worker.rs:764-768`). There is no process-group signal.

Measured on fault binaries compiled with `rustc --edition 2024 -D warnings`:

- Slow sleep: wall 503 ms, elapsed 503 ms, signal 9, reaped, pid absent, start time gone.
- Crash exit 99: reaped, pid absent, signal empty.
- Oversize declared length `u32::MAX`: elapsed 2 ms, signal 9, reaped. The 30 s sleep did not run out.
- Cancel before spawn: `Cancelled(None)`, pid 0, wall 0.
- Cancel from `on_start`: signal 9, wall 0, reaped, pid absent.
- Blocked write of a 1,513,710 byte request into a child that does not read: wall 508 ms, elapsed 500 ms, signal 9, reaped.
- Child that writes a one-byte refusal body and holds stdout open: wall 501 ms, signal 9, reaped.
- Child that writes four bytes and holds stdout: wall 504 ms, signal 9, reaped.
- Child that sleeps 700 ms and then writes a valid refusal: timeout at elapsed 501 ms, signal 9. The reply was not returned as `Ok`.

Wall time on these cases covers the blocked read or write. The joins returned.

The next call after a timeout used a new pid. A neighboring `/bin/sleep` kept the same `/proc` start time through that timeout and through the following legal plan. `supervise_with` does not signal an earlier pid (`parser_worker.rs:468`).

## F2. Legal SQL matches `guard_select`

The proof catalog is not the author's `logistics.shipment` fixture. Project `depot`, role `oo_r_clerk`, policy `policy-a195-fresh`, relation `depot.parcel`, physical `_oo.pt_parcel`, columns `parcel_id` OID 23 and `zone` OID 25, `count` OID 8801 with `aggregate_star`, operators 8802/8803 on OID 23 and 8804/8805 on OID 25.

Three statements matched the in-process guard, including both SHA-256 fields and `PINNED_PARSER_VERSION`. Star expansion was `parcel_id`, `zone`. `count(*)` bound function 8801 and operator 8802, and the first output was a function. Walls were 1 ms. An identity mismatch on `canary_secret_sql_a195` displayed and debugged without that text (`case=identity-mismatch canary_in_display=0`).

The production binary exited 2 for `--slow` and for `OO_PARSER_WORKER_SLOW` (`parser_worker.rs:677-679`).

## F3. Codec bounds

`decode_request` of a leading `u32::MAX` length returned `Limit` in under 50 ms (`parser_worker.rs:1653-1656` checks the length before `take`). A count of `u32::MAX`, and a count of 129 against `MAX_RELATIONS` 128 (`parser_worker.rs:73` and `:1680-1686`), returned `Limit`. Optional integer tag 2 returned `Tag`. Identity length 128 encoded and 129 did not. Identifier length 63 encoded and 64 did not. Closed catalog, serialize, and construct labels round-tripped. Unknown labels returned `Label`. Refusal tags 0, 17, and 255 returned `Tag`. A frame with a trailing byte returned `Trailing`. A declared body of `MAX_FRAME_BODY + 1` returned `Limit` before a body allocation (`parser_worker.rs:890-891` on the parent read path).

## F4. Unsupported is the true capability

`RESTRICTION_CAPABILITY` is `Unsupported` (`parser_worker.rs:233-239`). The contract says `Command` plus `env_clear` is not a network, filesystem, or memory sandbox, and that a grandchild which leaves the child is not reaped by the direct `SIGKILL` (`contracts/gateway/parser-worker-v1.md:11`).

The probe measured that description:

- `net`, `mnt`, `pid`, and `user` namespaces matched `/proc/self`.
- `/etc/passwd` opened. `listen=ok:42149` on `127.0.0.1`.
- Environment length 0. Process group id equaled the pid (`process_group(0)` at `parser_worker.rs:523`).
- File descriptors: `0->pipe`, `1->pipe`, `2->/dev/null`. `hosts_fd=false` for a parent file opened on `/etc/hosts`.

`spawn_worker` (`parser_worker.rs:512-528`) sets the command, clears the environment, points stdio at pipes and null, and sets the process group. It has no loop that closes other descriptors. The three fds in this run are the assigned stdio. A `CLOEXEC` parent file stayed out. That observation does not add a closer. Public admission stays closed.

`query_executor.rs:7-9` says a 500 ms process-isolated parser worker does not exist in that crate. `query_executor.rs:98-103` defines `UntrustedAdmission::Blocked` as "No 500 ms process-isolated parser worker is implemented" and sets `UNTRUSTED_ADMISSION` to `Blocked`. The proof asserted both the constant and that sentence. The comment is stale relative to `parser_worker`. The constant is still the admission gate. The file was not changed. SP06-R8 and the public gateway stay open.

## F5. A grandchild that holds stdout blocks the join

`g_direct_kill_leaves_a_grandchild`: the fault child spawned `/bin/sleep`, recorded its pid, and slept. After the supervisor timeout, `/proc/<grandchild>/comm` was `sleep` and the start time was still present. The direct child's signal was 9. The test then killed the grandchild.

`h_grandchild_stdout_is_not_a_sandbox`: the same grandchild inherited the fault child's stdout. `exchange` reads one extra byte until EOF (`parser_worker.rs:900-909`). `finish` joins that thread only after `Child::kill` of the direct pid (`parser_worker.rs:745-748`). The call did not return within 3 s (`hung=1`, grandchild 379, cwd `/tmp/oo-parser-worker-136-19`). Killing the grandchild unblocked the join. The result was `TimedOut` for direct pid 378, signal 9, reaped, pid absent, elapsed 3.012911835 s. That elapsed time is the test's watchdog plus the unblock, not the 500 ms deadline. The direct kill did not remove the grandchild. This is the unsupported case. It is not a passing sandbox test.

## F6. The child reader ignores request trailer bytes

`read_frame_from` (`parser_worker.rs:980-1000`) reads a header and the declared body and returns. It does not require the following read to be EOF. `decode_frame` does (`CodecError::Trailing`).

The review appended `0x7f` to a valid request and spawned `oo-sql-parser-worker` outside `supervise_with`. `decode_frame` returned `Trailing`. The child exited 0 and wrote 266 bytes. The supervisor's reply path still rejects a trailing reply byte. Callers that use `supervise_with` send one encoded frame. A parent that writes extra request bytes still gets a guard reply from this binary.

## F7. Malformed faults that exit on their own

Magic, kind, and trailer workers wrote a bad frame and returned. The reports are `Malformed`, reaped, pid absent, exit `Some(0)`, signal empty, elapsed 0. `stop_child` still calls `kill` and `wait`. `wait` returned the exit status the process already had. The pid and start time were gone. The same reap held for the crash (exit 99) and for signal 9 on the sleeps.

## Tests that were not repeated

The pre-overlay smoke ran `oo-gateway` `--lib` and `--test parser_worker` plus clippy. It did not run the `sql_guard` integration campaign. The proof ran only `parser_worker_a195_review`. No authentication test, no HTTP gateway test, and no release check was run.
