# HANDOFF — a181 local file hosting

Independent fixed review is next. This slot did not review its own commit as that review.

## Read first

- Source commit `755fb84acc7f80e5ba2fba1c2b80412dab7fab26`, parent `72b5136ba23d134916425132523383947046aeff`.
- `contracts/control/local-file-hosting-v1.md`
- `crates/oo-control/src/file_hosting.rs`
- `crates/oo-control/tests/file_hosting.rs`
- `dependency-delta.md` and `SHA256SUMS` in this directory
- `impl/console.log` for the green run, and `impl/attempt1` through `impl/attempt6` for the failed runs
- `pre-edit/` for the Rust 1.94 control smoke taken before the product files existed

Fixed base for the task was `03512f91324ac33e772e9702fe806e8e04037ec4`. The source commit sits on `72b5136b`, which was HEAD when the green smoke archived the tree. `oo-control`, root `Cargo.toml`, and `Cargo.lock` were unchanged between the fixed base and that parent for this crate's previous contents. The green smoke overlaid the owned files onto `git archive` of `72b5136b`.

## What was checked

Linux/arm64 Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Green phase exits, all 0: fmt, fetch, lock delta, tree, `cargo test --locked --offline -p oo-control --test file_hosting --lib` (6 passed), `cargo clippy --locked --offline -p oo-control --all-targets -- -D warnings`, `cargo run --locked --offline -p oo-arch-check -- --manifest /src/Cargo.toml` (`architecture check: pass`).

No PostgreSQL. No second builder. No push.

## Review focus

The upload state machine in `complete`, `abort`, and `cleanup_open`: which keys are deleted, and that `put_multipart_opts` runs only after the hash matches. The idempotency pointer race that adopts the winner and deletes only the orphan session. Quota that ignores non-hex names and is not linearizable. `Denied` versus `NotFound`. Error display text must stay free of store paths. `FileHost` debug omits the store.

The lock addition is five crates, recorded in `dependency-delta.md`. itertools 0.15.0 is an added version, not a rewrite of the older itertools rows.

## Left open on purpose

S3 and MinIO, part-size rules from S3, the reaper, download and TTL, HTTP and grant authentication, MIME sniffing, formula-safe output, retention, presigned URLs, and the REQ-SRC-05 states after `stored`. Do not treat this commit as closing SP05-W01 or SP05-W06. The ledger was not changed.
