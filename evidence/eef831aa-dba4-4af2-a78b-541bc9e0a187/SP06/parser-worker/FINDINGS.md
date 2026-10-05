# Findings

Source commit `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`. Green run `linux/focused-6`.

## Isolation

`RestrictionCapability::Unsupported` is the published capability. `std::process::Command` with `env_clear`, a private empty working directory, stderr set to null, and a new process group is the implementation. The parent calls `Child::kill` (`SIGKILL`) and then `wait`. That deadline is 500 ms from immediately before `spawn` through the read of the reply. The slow-worker test observed signal 9, a reaped status, elapsed time at least 500 ms and under 3 s, and the recorded `/proc` start time gone.

This is not a kernel network, filesystem, or memory sandbox. The crate does not call `runsc`. It does not add a dependency. It has no `unsafe` block for namespaces, landlock, seccomp, or rlimits. Inherited file descriptors other than stdin, stdout, and stderr are not closed. A grandchild that leaves the process group is not killed by the direct `SIGKILL`. Those controls were not verified. A future production launcher remains its own gate.

## Admission

`oo_gateway::query_executor::UNTRUSTED_ADMISSION` is `Blocked`. The focused test `admission_stays_blocked_and_the_sandbox_is_unsupported` passed against the archive copy of `query_executor.rs`. That archive copy still documents the variant with the sentence "No 500 ms process-isolated parser worker is implemented." This task did not edit `query_executor.rs`, `sql_guard.rs`, or `policy.rs`. The worktree copies of `query_executor.rs` and `policy.rs` differed from the archive at launch and were not the bytes compiled in focused-6.

The worker is a library and a binary. It is not an HTTP route, an authentication check, an audit write, a row-level security check, or a database session. The child environment is cleared, so it does not inherit a database URL, a model token, or `HOME`. Public untrusted admission stays closed until the worker, authentication, audit, and row-level security are all present.

SP06-R8, the public gateway, and the full goal stay open.

## Codec and process

The wire format is the length-prefixed layout in `contracts/gateway/parser-worker-v1.md`. Magic `OOSW`, version `1`. Counts and string lengths are checked before the buffer is reserved. Static labels are accepted only from the allowlists, including the predicate labels the guard emits (`LIKE predicate`, `IN predicate`, `BETWEEN predicate`, `ANY predicate`). Refusal bodies carry no SQL text and no privilege bit. `ParserResponse::Plan` holds `Box<GuardedPlan>` so the refusal variant does not reserve the plan's stack size. The box is a Rust representation. The frame layout is unchanged.

Each `supervise` call starts a new child and waits for that child before it returns. The test `a_timeout_does_not_kill_the_next_child` passed. The production binary exits 2 on any extra argument and when `OO_PARSER_WORKER_SLOW` is set, and it does not sleep. Fault programs (slow, crash, oversize, bad version, bad magic, bad kind, trailer, echo, delayed) are separate `rustc` binaries built by the integration test.

The oracle for legal SQL is `guard_select` in the test process. The supervisor library does not call the guard. The child does.

## Build constraints

No manifest, lock, or dependency change. Existing `std`, `oo-core`, `pg_query`, and `sha2` only. `lib.rs` exports `pub mod parser_worker` and does not re-export the module's items at the crate root. Cargo discovered `src/bin/oo-sql-parser-worker.rs` without a `Cargo.toml` edit. The integration test locates it with `env!("CARGO_BIN_EXE_oo-sql-parser-worker")`.

The pre-edit smoke compiled the archive and did not compile these files. Focused-6 is the run that compiled them.
