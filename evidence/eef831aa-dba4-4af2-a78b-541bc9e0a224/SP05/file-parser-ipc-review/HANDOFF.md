# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a224`. Reviewed source `6bd2455849da2527973c136968d4549ecd98d770`, tree `99efda0344ffff4b0b52851c31e495daf2e1cdc5`, parent `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`. Author evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a207/SP05/file-parser-worker` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers the file-parser child for one explicit CSV, TSV, or JSONL profile, framed as Arrow IPC and decoded with `StreamReader`. Import, publication, the parent exporter, the loader, and a public endpoint stay open. `admits_import` and `published` stay false. Kernel network, filesystem, and memory stay `unsupported`. `parent_kill` and `parent_reap` stay `caller`. A blocked read ends when the parent kills the process. The path check accepts one regular file and refuses a directory, a symlink, and a fifo. It does not jail the path. The IPC ceiling binds the sink. Resident memory was not measured.

CSV library approval stays a199. JSONL library approval stays a185. This review's fixtures are independent of those suites.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/oo-file-parser-worker.rs` and `source/blobs/file-parser-worker-v1.md`
- `overlay/file_parser_worker_a224_review.rs`
- `linux/overlay-4/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the author smoke before the overlay compiled
- `linux/supply/` for the package audit
- `linux/overlay/`, `linux/overlay-2/`, and `linux/overlay-3/` for preserved harness failures
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`, `tools/sha256_files.rs`, and `tools/supply_audit.rs`
- `recipes/ORIGIN.txt`

## Decisive exits

`linux/overlay-4`: `blob-cmp.exit` 0, `parent-absent.exit` 0, `fetch.exit` 0 (`cargo fetch --locked --offline`), `clippy-component.exit` 0, `test.exit` 0, `hash-compare.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s`.

CSV object `cd9c70c60ce4cf93b1f5715862e89e21cb5ff55d424971df80a99953dd808c2c`. TSV object `db3814f9a7869aae9a5b8406e2204543ae8321c1cf75b423a63e6dd9c9f399cd`. JSONL object `132ce226d1f4f90db4967e63831fca2c2eb561864c65c6074a25c8ba90cf25af`. Empty object `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. `linux/overlay-4/hash-compare.txt` shows `sha256sum` of the saved fixtures equal to the Rust digests. The deciding digest is the Safe Rust function in the harness. Container `ed26c6696c84d82da500f19808c29f8323c08ff8c33fadf6ac113bd8ae0c6edf`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`.

Clippy for that run is `cargo clippy --locked --offline -p oo-connectors --test file_parser_worker_a224_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.26s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/supply`: `supply-rustc.exit` 0, `supply-curl.exit` 0, `supply.exit` 0, `docker-run.exit` 0. Container `3ea1d1ae0987238c0a615e5009a78c709d45c54a62641d52c582e1a8b59777ce`. `supply.stdout` ends `supply_audit=ok`. Four additions, Apache-2.0, MSRV at most 1.94.0, checksums matched, age at least 7 days at `2026-10-05T21:49:34Z`.

`linux/preoverlay`, before the review harness was in the archive: offline fetch `101` on a cold volume, then `fetch.exit` 0, author `test.exit` 0 (`12 passed`, 0.42s), `clippy.exit` 0, `supply-rustc.exit` 1, `docker-run.exit` 1. Rust `1.94.0` on `linux/arm64`. Container `7343c8e6a86d3c79afbb304cffd5c1e8c8884502caefcd91a86fe68c637d27a2`.

## Preserved attempts

- `linux/preoverlay` supply compile `E0106`. `supply.exit` is `not_run`. Container `7343c8e6a86d`.
- `linux/overlay`: `find_field` lifetime. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `4bc7a6b02333`.
- `linux/overlay-2`: schema temporary. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `b749080d8fb0`.
- `linux/overlay-3`: 9 tests passed and the hash compare matched. Clippy `useless_format` exited 101. Container `9543062601a2`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `6bd24558`, private cargo and target volumes, private network, `HOME=/tmp`, memory 6g, 1 CPU. Copy `overlay/file_parser_worker_a224_review.rs` onto that archive as `crates/oo-connectors/tests/file_parser_worker_a224_review.rs`. Install clippy in the same container. `sha256sum` must be on `PATH` for the cross-check. `OO_A224_ORACLE_DIR` receives the fixtures. The child environment stays empty. No PostgreSQL. Do not re-run `file_parser_worker` in the overlay container. The preoverlay container is the author-suite pass.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-connectors --test file_parser_worker_a224_review -- --nocapture --test-threads=8
cargo clippy --locked --offline -p oo-connectors --test file_parser_worker_a224_review -- -D warnings
```

The supply rerun uses the same image and the warm cargo volume, `network` not internal, and `tools/supply_audit.rs` against `/src/Cargo.lock`, `/locks/parent.lock`, and the crate files under the cargo home. It re-fetches the four index and API documents. `cargo update` stays the a207 record and is not part of this rerun.

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust. It checks the empty digest and `abc`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` in the pinned image. `/tmp` is `noexec`, so the binary is not placed on a tmpfs. The first container stdout is copied to `linux/manifest/console.log` after that process exits. Container `d5c6cdf7ca369b2837c3d1f73ec09006f8fb52a679802db3d9f4130d5a1a4296` exited 0. The rewrite container `a691407791a6bf78c5682f8536dd03dce9679a1d09b38fc68fdcfd2ce5e12f13` wrote `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, and the exit files were closed, then its extra `cmp` exited 1 because the next walk included `MANIFEST.sha256`. `linux/manifest/REWRITE.txt` records that. One later container writes `MANIFEST.sha256` again after this paragraph and does not write another file into this directory. Its stdout stays outside this directory.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a224-eef831aa-net` (`network_rm_exit=0`, 0 containers attached, owner `a224`), volumes `oo-a224-eef831aa-cargo` and `oo-a224-eef831aa-target` (`cargo_volume_rm_exit=0`, `target_volume_rm_exit=0`), `/tmp/oo-a224-eef831aa-dba4-4af2-a78b-541bc9e0a224` (`tmp_rm_exit=0`, absent afterward), and `/tmp/oo-a224-meta` (`meta_rm_exit=0`, absent afterward). The launch scripts had already removed the containers. The pinned Rust image stays. `docker prune` was not run. PostgreSQL was not started. `oo-a064-rust194-contract`, `oo-a064-rust194-contract2`, `oo-a064-rust194-smoke`, and `oo-a064-pg1710-jobs` were still present. `oo-a223-pg` and `oo-a218-pg` were already absent. This cleanup did not remove them.

The evidence commit contains only this directory.
