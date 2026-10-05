# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a215`. Reviewed source `c27d6349ffa7cf8629a5a79c62015ca5c255e18f`, tree `c53545c3ba03dbc0e9da5ddb1ea84916228ec947`, parent `90dfac17534983e3f3f93d5f437806193c24095b`. Author repair record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a201/SP06/parser-reap-bounds/`. Prior review is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a195/SP06/parser-worker-review/` (verdict APPROVE LIMITED on `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`, findings F5 and F6). This directory is the independent review of the repair commit. No product file was edited. The parent ledger was not edited.

The approval covers the 500 ms parser-worker deadline, including the process-group signal, the direct wait, and the IO join, on the sealed Linux arm64 proof below. `RESTRICTION_CAPABILITY` stays `Unsupported`. `UNTRUSTED_ADMISSION` stays `Blocked`. A `setsid` descendant stays outside the group signal. Pid reuse when `/proc` no longer names the leader stays the residual in contract line 29. Contract line 15 names a `query_executor.rs` sentence this tree does not contain. M1-D23, SP06-R8, the public gateway, and release stay open.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/COMMIT` and `source/blobs/`
- `overlay/parser_worker_a215_review.rs`
- `linux/overlay/` for the decisive run, including `test.stdout`, `clippy.stderr`, and `oracle/oracle.txt`
- `linux/preoverlay/` for the author-test smoke before the harness existed
- `linux/overlay-attempt-1/` and `linux/overlay-attempt-2/` for preserved harness failures
- `linux/RECIPE.txt` for the a204, a201, and a199 origins
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`

## Decisive exits

`linux/overlay`: `blob-cmp.exit` 0, `fetch.exit` 0, `clippy-component.exit` 0, `test.exit` 0, `clippy.exit` 0, `lock-hash.exit` 0, `overlay.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.30s`.

Largest timeout elapsed time 509 ms (`setsid-stdout`). Ceiling 530 ms. Crash elapsed 2 ms, exit 99. Container `5d5d5700621af76e815031d324f71e2759b28fe4de13bfc63020a7918f6654d5`.

`linux/preoverlay`, before the harness file existed: offline fetch 101, online fetch 0, libclang 0, author `parser_worker` tests 0 (`30 passed` in 6.56 s), docker 0. Container `ec2a20b5d578cb76f3e737e2bbdf0a6b1e97dd74926eb9877a300c6fcd4d576a`. Rust `1.94.0` on `linux/arm64`. Memory 6g, one CPU.

## Preserved attempts

- `linux/overlay-attempt-1`: harness `E0277` on `str::get(usize)`. `test.exit` 101, `clippy.exit` `not_run`, `docker-run.exit` 101. Container `9e287464be3d6cfde3327c8bcc6c825c7fc375d0d2a6ab6b2e64e6549d141581`.
- `linux/overlay-attempt-2`: 5 tests passed in 4.26 s, elapsed times inside 530 ms. Clippy `clone_on_copy` on `ProcessReport`. `clippy.exit` 101, `docker-run.exit` 101. Container `bd6791bc739b04cef58136d832eef5050c7489af934c20f44fc0947518b7160d`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `c27d6349`, private cargo and target volumes, private network, `HOME=/tmp`, `TMPDIR=/cargo-home/tmp`. Copy `overlay/parser_worker_a215_review.rs` onto that archive as `crates/oo-gateway/tests/parser_worker_a215_review.rs`. Install clippy in the same container. `OO_A215_ORACLE_DIR` receives the oracle. No PostgreSQL. One builder, 6g, one CPU.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-gateway --test parser_worker_a215_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --test parser_worker_a215_review -- -D warnings
```

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust. It checks the empty digest `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc` digest `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` in the pinned image. `/tmp` is `noexec`, so the binary is not placed on a tmpfs. The first container stdout is copied to `linux/manifest/console.log` after that process exits. `linux/manifest/followup.sh` rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, the exit files, and `CLEANUP.txt` are closed. That later stdout stays at `/tmp/oo-a215-eef831aa-manifest-followup.log`, outside this directory.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a215-eef831aa-net` (`network_rm_exit=0`, 0 containers attached), volumes `oo-a215-eef831aa-cargo` and `oo-a215-eef831aa-target` (`cargo_volume_rm_exit=0`, `target_volume_rm_exit=0`), and `/tmp/oo-a215-eef831aa-dba4-4af2-a78b-541bc9e0a215` (`tmp_rm_exit=0`, absent afterward). Launch scripts had already removed this task's containers (`container_rm_exit=none`). The pinned Rust image stays. `docker prune` was not run. Foreign containers still present afterward include `oo-a226-chain`, `oo-a226-pg`, and `oo-a229-pg`. The follow-up console path above stays outside the removed tmp directory.

The evidence commit contains only this directory.
