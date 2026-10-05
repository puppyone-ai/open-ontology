# Findings

Reviewed the frozen parent scope against the worker contract and the build06 proof. This is not an acceptance review and it does not close a separate review slot.

## Observed

- Pre-overlay `cargo test -p oo-connectors --test file_parser_worker` on `c2db9c430b822a824a1062b6c3c7345924df7dd8` passed, 12 tests, before `file_worker_parent.rs` existed in the extract.
- After the overlay, 8 parent tests passed: CSV, TSV, and JSONL exact values; empty JSONL; deadline and path refusal; early `finish`; hash mismatch, bad quote, and int overflow; fixture malformed, trailer, and early complete; 800 ms stall then a later empty JSONL call; 1 ms stall.
- The 800 ms stall was `TimedOut`, reaped, pid not alive, `group=Sent`, termination signal 9, elapsed 801 ms. The following real-worker call completed.
- Clippy `--lib --test file_worker_parent -- -D warnings` exited 0 on Rust 1.94.
- `lib.rs` only gained the module declaration, the export, and the crate-doc sentence.
- No manifest, lock, or worker-binary edit. No new dependency. No PostgreSQL.

## Arrow memory

Build04 printed `a220-debug arrow lineage=2800 decoded=1928 raw=102` for the enterprise CSV and no other mismatch. IPC decode changes buffer lengths, so `get_array_memory_size` on the decoded batch is not the worker's pre-encode sum. The parent now requires both numbers to be inside `batch_bytes + 65536`, and both to be non-zero when the batch has rows. The contract states that. Equality would reject this worker. The debug print was removed before build05.

## Limits

- `KernelRestriction::Unsupported` is the implemented state. Group kill plus `Child::kill` is not network, filesystem, or memory isolation. `setsid` descendants are not killed. Inherited file descriptors are not closed beyond the three stdio pipes. No containment certificate is issued.
- The identity check is Linux `/proc`. This proof did not run on another OS.
- The parent does not re-parse the source file to recompute every lineage offset. The tests check those offsets on the real worker output.
- A hash that is still incomplete when the deadline expires returns `Config("deadline")` and does not spawn. The 1 ms test spawned and returned `TimedOut`.
- Display text has no path and no cell bytes. The stall numbers are test output, not the error Display.
- Failed attempts build01 through build04 are kept. Build01 did not compile (`send_timeout` is unstable on this toolchain, and `Stdio` does not convert from `UnixStream` directly). Build02 and build03 compiled far enough to fail Clippy or the CSV test.
