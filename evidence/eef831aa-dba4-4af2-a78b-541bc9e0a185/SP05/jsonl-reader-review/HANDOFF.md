# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a185`. Reviewed source `47743688d45ec7a001e796725febc5357b7e20bd`, tree `14b54c719c69d51fd87784dd9cc98f0c41b24668`, parent `ce040593f6af6a61a5c3e29abd494b1a863f32c5`. Author evidence `753423b7` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers the `oo.source.jsonl/v1` pull over `std::io::Read`. Arrow IPC, a file loader, process isolation, CSV, TSV, Parquet, DuckDB, GB18030, and SP05-W02 as a phase stay open. `admits_import` and `published` stay false. A blocked `Read` ran to the harness release (411 ms on cancel, 1507 ms on deadline). That observation is the honest limitation. It is not a universal deadline proof.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/jsonl.rs` and `source/blobs/jsonl-reader-v1.md` (content SHA-256 matches `47743688`)
- `overlay/jsonl_reader_a185_review.rs`
- `linux/overlay-4/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the connectors library smoke before the overlay existed
- `linux/overlay/`, `linux/overlay-2/`, and `linux/overlay-3/` for preserved harness failures
- `diff/parent.diff` for the four-file change from `ce040593` to `47743688`
- `CLEANUP.txt` and `MANIFEST.sha256`

## Decisive exits

`linux/overlay-4`: `blob-cmp.exit` 0, `fetch.exit` 0 (`cargo fetch --locked --offline`), `clippy-component.exit` 0, `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.92s`.

`oracle_file_sha256` and `complete_sha256` are `d988fc1a3142a23b8cd89fe4483f7159ec6525808bcf6879769b7e2a1d75ef11`. Profile preimage `c99e1d7d9e07979e671057a0169ad1dec3b448bcc2482167b2e2c6a6842b15a5`. Config preimage `63133140d481e6b446f4301bd043d3144f4b41982a1d8deaa789fdc3645163b9`. A host `shasum -a 256` of the three saved oracle files matches those digests.

Clippy for that run is `cargo clippy --locked --offline -p oo-connectors --test jsonl_reader_a185_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.22s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/preoverlay`, before any overlay file was copied: `fetch.exit` 0, `lib.exit` 0 (`64 passed; 0 failed`, 0.01s), `clippy.exit` 0 on `--all-targets`, `overlay-absent.exit` 0, `docker-run.exit` 0. Rust `1.94.0` on `linux/arm64`.

## Preserved attempts

- `linux/overlay`: harness called `budget` with `max_total` 8 below `max_record` 32. `test.exit` 101, `docker-run.exit` 101, `clippy.exit` `not_run`.
- `linux/overlay-2`: 8 tests passed, `test.exit` 0. Fresh container had no clippy component. `clippy.exit` 1, `docker-run.exit` 1.
- `linux/overlay-3`: clippy installed, 8 tests passed, `clippy.exit` 101 on `needless_lifetimes` in the harness, `docker-run.exit` 101.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `47743688`, private cargo and target volumes, private network, `HOME=/tmp`. Copy `overlay/jsonl_reader_a185_review.rs` onto that archive as `crates/oo-connectors/tests/jsonl_reader_a185_review.rs`. Install clippy in the same container. `sha256sum` must be on `PATH`. `OO_JSONL_REVIEW_ORACLE_DIR` receives the fixture and preimages. No PostgreSQL.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-connectors --test jsonl_reader_a185_review -- --nocapture
cargo clippy --locked --offline -p oo-connectors --test jsonl_reader_a185_review -- -D warnings
```

## Cleanup

`CLEANUP.txt` records removal of network `oo-a185-eef831aa-net` (`network_rm_exit=0`, 0 containers attached, owner `a185`), volumes `oo-a185-eef831aa-cargo` and `oo-a185-eef831aa-target` (`volume_rm_exit=0`, 0 users), and `/tmp/oo-a185-eef831aa-dba4-4af2-a78b-541bc9e0a185` (`tmp_rm_exit=0`, absent afterward). The empty `oracle/` directory at the review root was removed. The pinned Rust image stays. `docker prune` was not run. Foreign containers were left in place. PostgreSQL was not started.

`MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. The evidence commit contains only this directory.
