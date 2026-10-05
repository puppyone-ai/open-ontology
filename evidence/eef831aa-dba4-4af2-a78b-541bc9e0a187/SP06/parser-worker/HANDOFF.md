# Handoff

Source commit: `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`

Message: `gateway: isolate one SQL guard call in a one-shot child`

Owned product paths in that commit:

- `crates/oo-gateway/src/parser_worker.rs`
- `crates/oo-gateway/src/lib.rs`
- `crates/oo-gateway/src/bin/oo-sql-parser-worker.rs`
- `crates/oo-gateway/tests/parser_worker.rs`
- `contracts/gateway/parser-worker-v1.md`

Blobs of those files are under `source/`. `source/COMMIT.txt` repeats the hash. `SHA256SUMS` covers this evidence directory.

## For the next reviewer

Review the source commit above. The green evidence is `linux/focused-6`: `docker-run.exit` 0, `focused.exit` 0, `phases/test.exit` 0, `phases/clippy.exit` 0. Raw logs are `linux/focused-6/test.log`, `clippy.log`, `console.log`, `hostconfig.txt`, `mounts.txt`, and `forbidden-compare.txt`. Earlier attempts stay in `linux/focused` through `linux/focused-5`.

Check that the parent never parses SQL, that a late or failed child is killed and reaped, and that the codec round-trips the catalog and the plan the guard already returns. The contract is the field layout.

Do not treat this commit as permission to accept untrusted SQL. `UNTRUSTED_ADMISSION` is `Blocked`. `RESTRICTION_CAPABILITY` is `Unsupported`. Authentication, durable audit, and row-level security are absent. A production `runsc` or other launcher proof is a separate gate. SP06-R8, the public gateway, and the full goal stay open.

The archive `query_executor.rs` still says that no 500 ms worker is implemented. Updating that comment belongs to a task that owns `query_executor.rs`. This task left the file untouched. The same rule applies to `sql_guard.rs`, `policy.rs`, manifests, and the lock.

## Reproduce the focused command

Inside the pinned 1.94 image, with `LIBCLANG_PATH` pointed at the extracted LLVM tree on the private cargo volume, and with only the five owned files overlaid on archive `523951fd9bea54e38c9e6c14a2df109cf0b1b196`:

```
cargo test --locked --offline -p oo-gateway --lib --test parser_worker --test sql_guard -- --test-threads=4
cargo clippy --locked --offline -p oo-gateway --all-targets -- -D warnings
```

`RUSTUP_TOOLCHAIN=1.94.0`. Do not point that build at a moving worktree copy of `policy.rs` or `query_executor.rs`.

## Cleanup already recorded

Focused containers were removed by their launch scripts (`container-removed.txt`). This handoff does not remove other tasks' containers, volumes, or networks.
