# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a204`. Reviewed source `59be14baab1d5f30abaa2ce8cf67d607a2e4417f`, tree `d41c64a880ba4ef9f11775f5142bed1a2e7d76e9`, parent `ef6c4fc7da03342f900146e90ff692fe742d1ba4`. Author evidence `73d772c5cc7bac52c9e95754444f4c839a3ec646` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers `validate-file` for one explicit CSV, TSV, or JSONL profile. Import, publication, PostgreSQL, HTTP, Arrow IPC, worker isolation, dialect sniffing, GB18030, Parquet, Excel, and a public authentication decision stay open. `admits_import` and `published` stay false. A blocking read is not interrupted during the read. The path is a trusted local file. `File::open` follows a symlink. That observation is the honest limitation, not a jail.

CSV library `671507b229ad7d1bc6a2e948b5b74d6d7252393f` stays the a199 approval. JSONL `47743688` stays the a185 approval. Decimal cell coefficients in Arrow are the a199 public-reader proof. This command's summary carries precision and scale. `1.251` at scale 2 is `inexact_decimal` and produces no success receipt.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/validate-file.rs` and `source/blobs/validate-file-cli-v1.md` (content SHA-256 matches `59be14ba`)
- `overlay/validate_file_a204_review.rs`
- `linux/overlay-3/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the pipeline smoke before the overlay existed
- `linux/overlay/` and `linux/overlay-2/` for preserved harness failures
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`

## Decisive exits

`linux/overlay-3`: `blob-cmp.exit` 0, `fetch.exit` 0 (`cargo fetch --locked --offline`), `clippy-component.exit` 0, `test.exit` 0, `hash-compare.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s`.

CSV object `e316ece441b03a2ef125c5ac5810d3697c8269b54701aecd63d92ff701536445`. TSV object `5ef62398b756b7b6d2d10b9aa305ec49acf9ac92402c0abbfcd1e1f1fda9d66e`. JSONL object `62049328d535525064945cbf32c2a68ef6c2ac16c85989460e73d791097f8958`. Empty object `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. `linux/overlay-3/hash-compare.txt` shows `sha256sum` of the saved fixtures equal to the Rust digests. The deciding digest is the Safe Rust function in the harness. Container `483124ad01e5938972d03e928a550526218cdbed0a3664db199c656464f1d01e`.

Clippy for that run is `cargo clippy --locked --offline -p oo-pipeline --test validate_file_a204_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.22s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/preoverlay`, before any overlay file was copied: offline fetch `101` on a cold volume, then `fetch.exit` 0, `lib.exit` 0 (`11 passed`), `cli.exit` 0 (`11 passed`, the one `validate_file_cli` pass), `clippy.exit` 0, `docker-run.exit` 0. Rust `1.94.0` on `linux/arm64`. Container `f4edb34736e80c706c479c86e6043bf1b9245551eb939fc5ff57635ae27fbdcd`.

## Preserved attempts

- `linux/overlay`: `expect_err` required `Debug` on `DelimitedSeen` and `JsonlSeen`. `test.exit` 101, `clippy.exit` `not_run`, `docker-run.exit` 101. Container `627672b8c14dbba61b49a245fe219f2caa0991b7c275a1d31429c4d332341ae7`.
- `linux/overlay-2`: 10 tests passed and the hash compare matched. Clippy rejected unread closed-flag fields. `clippy.exit` 101, `docker-run.exit` 101. Container `9c6e1b6f224dd8618bb8693888eb4a1661e4a271d39004d97b19ad8855a6e8b1`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `59be14ba`, private cargo and target volumes, private network, `HOME=/tmp`. Copy `overlay/validate_file_a204_review.rs` onto that archive as `crates/oo-pipeline/tests/validate_file_a204_review.rs`. Install clippy in the same container. `sha256sum` must be on `PATH` for the cross-check. `OO_A204_ORACLE_DIR` receives the fixtures. Do not set `OO_A197_ORACLE_DIR`. No PostgreSQL. Do not re-run `validate_file_cli` in the overlay container. The preoverlay container is the one existing CLI pass.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-pipeline --test validate_file_a204_review -- --nocapture
cargo clippy --locked --offline -p oo-pipeline --test validate_file_a204_review -- -D warnings
```

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust. It checks the empty digest and `abc`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` in the pinned image. `/tmp` is `noexec`, so the binary is not placed on a tmpfs. The first container stdout is copied to `linux/manifest/console.log` after that process exits. A later run of the same tool rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, and the exit files are closed. That later stdout stays outside this directory.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a204-eef831aa-net` (`network_rm_exit=0`, 0 containers attached, owner `a204`), volumes `oo-a204-eef831aa-cargo` and `oo-a204-eef831aa-target` (`cargo_volume_rm_exit=0`, `target_volume_rm_exit=0`), and `/tmp/oo-a204-eef831aa-dba4-4af2-a78b-541bc9e0a204` (`tmp_rm_exit=0`, absent afterward). The pinned Rust image stays. `docker prune` was not run. `oo-a203-pg` was still present afterward. PostgreSQL was not started.

The evidence commit contains only this directory.
