# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a251`. Reviewed source `0a30fcda7fb586bd8d6955e03ae306888374b915`, tree `ee69dec5a05f66f136e2e7cd72ac0bce65fd918d`, parent `9907d40bc0d2fd684a786cd8f98473d52403fc3e`. Author evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a245/SP05/file-parser-cli/` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers `parse-file` for one explicit CSV, TSV, or JSONL profile. The success document is a bounded summary emitted only after `ParentParseComplete`. Import, publication, PostgreSQL, HTTP, a public authentication decision, dialect sniffing, GB18030, Parquet, Excel, the worker review, and the parent review stay open. `admits_import`, `published`, and `pg_run_complete` stay false. `kernel_restriction` `unsupported` stays the honest parent state. SP05-W02, AT-FILE-02, and the phase ledger stay open.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/parse-file.rs` and `source/blobs/parse-file-cli-v1.md` (content SHA-256 matches `0a30fcd`)
- `overlay/parse_file_a251_review.rs`
- `linux/overlay-2/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the `parse_file_cli` smoke before the overlay existed
- `linux/overlay/` for the preserved harness Clippy failure
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`
- `recipes/ORIGIN.md`

## Decisive exits

`linux/overlay-2`: `blob-cmp.exit` 0, `fetch-offline.exit` 0, `fetch.exit` 0, `clippy-component.exit` 0, `test.exit` 0, `hash-compare.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 224.09s`.

CSV object `a82155e70d4ccd1787149cde689b4b9acdf9e911e16da7f3fd15dd8a3c746cc1`. TSV object `17765af692fdf1f83732808353e7866dc6bca2c2212815512e5dcdd689b2dd1c`. JSONL object `f31e2f56091c6936ebf31cfec1d8426ee7080046b8f8b84241d6c04e15cd3244`. Empty object `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. `linux/overlay-2/hash-compare.txt` shows `sha256sum` of the saved fixtures and of the lineage preimages equal to the Rust digests. Container `4e9cc7cee1c1096e7193c4de3c2cdbe4a041ea7cc0439a0f25ccad8e817c9b67`.

Deadline on that run: `empty_ms=8929`, `busy_ms=74438`, `kill_ms=32754`, `kill_elapsed_ms=41599`, `term_signal=Some(9)`, `exit_code=None`, `teardown=signal9`. The next empty JSONL call completed. The closed-pipe exit-7 branch was not taken on this run or on the preserved overlay run.

Clippy for overlay-2 is `cargo clippy --locked --offline -p oo-connectors --test parse_file_a251_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.29s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/preoverlay`, before any overlay file was copied: offline fetch `101` on a cold volume, then `fetch.exit` 0, `test.exit` 0 (`12 passed`, `205.59s`, the existing `parse_file_cli` suite), `clippy.exit` 0, `docker-run.exit` 0. Rust `1.94.0` on `linux/arm64`. Container `fa515ca94e48de15b9fad0c4253e97f4c442c6bf9d0ffd4dab1619dae5c7e6d8`.

## Preserved attempt

`linux/overlay`: 12 tests passed (`223.32s`) and the hash compare matched. Clippy rejected `document` for `too_many_arguments` (8/7) at harness line 518. `clippy.exit` 101, `docker-run.exit` 101. Container `e6eed3cccf5682ffdcd4c20e015b98c8b5b257c06593103572e34f6324daa624`. The product sources were not changed. The fixed harness is `overlay/parse_file_a251_review.rs`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, `CARGO_BUILD_JOBS=1`, archive of `0a30fcd`, private cargo and target volumes, private network, `HOME=/tmp`, 6 GiB, 1 CPU. Copy `overlay/parse_file_a251_review.rs` onto that archive as `crates/oo-connectors/tests/parse_file_a251_review.rs`. Install clippy in the same container. `sha256sum` must be on `PATH` for the cross-check. `OO_A251_ORACLE_DIR` receives the fixtures. No PostgreSQL. The preoverlay container is the existing `parse_file_cli` pass. The overlay container is the independent harness.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-connectors --test parse_file_a251_review -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-connectors --test parse_file_a251_review -- -D warnings
```

Recipe origins, with the commit paths and hashes this review copied from, are in `recipes/ORIGIN.md`. The changes are the owner, this fixed commit, 6 GiB, 1 CPU, the global builder cap of 10, and the `parse-file` test names. The a199 manifest tool is unchanged (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`).

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust. It checks the empty digest and `abc`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` in the pinned image, network none, 6 GiB, 1 CPU. `/tmp` is `noexec`, so the binary is not placed on a tmpfs. The first container stdout is copied to `linux/manifest/console.log` after that process exits. `linux/manifest/rehash.sh` runs the same tool again after `console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` exist, and rewrites `MANIFEST.sha256`. That later stdout stays outside this directory and is not copied in. The host does not rewrite `hostconfig.txt` or `console.log` on the second run. A file written into this directory after the second walk is outside the inventory.

## Cleanup

`CLEANUP.txt` records removal of volumes `oo-a251-eef831aa-cargo` and `oo-a251-eef831aa-target`, network `oo-a251-eef831aa-net` (id `975d812f34c3eaa83d7bdf094adec5b248f0c63f69169ae921d4ce27cd829d92`, 0 containers), and `/tmp/oo-a251-eef831aa-dba4-4af2-a78b-541bc9e0a251`. Each removal exit was 0. The pinned Rust image stays. `docker prune` was not run. PostgreSQL was not started. The manifest builder is a later container and is removed by its own launch script.

The evidence commit contains only this directory.
