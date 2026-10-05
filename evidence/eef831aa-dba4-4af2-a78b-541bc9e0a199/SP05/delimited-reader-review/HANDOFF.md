# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a199`. Reviewed source `671507b229ad7d1bc6a2e948b5b74d6d7252393f`, tree `d85bc4363875c707a2ec582a44a3a53e69e1a278`, parent `65169f374c665d8efdd62a8bcd47317c38a1359e`. Author evidence `123383c090a3848b81d2240a826423c76ce8a112` and `b487f0a171aee739a76dcc8b605ad1246e48dfc5` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers the `oo.source.delimited/v1` pull over `std::io::Read` for an explicit comma or tab dialect. Arrow IPC, a file loader, process isolation, publication, PostgreSQL `RunComplete`, GB18030, sniffing, DuckDB, and SP05-W02 as a phase stay open. `admits_import` and `published` stay false. A blocked `Read` ran to the harness release (80 ms still blocked after cancel, then `cancelled` with one read; 2009 ms on an 800 ms deadline). That observation is the honest limitation.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/delimited.rs` and `source/blobs/delimited-reader-v1.md` (content SHA-256 matches `671507b2`)
- `overlay/delimited_reader_a199_review.rs`
- `linux/overlay-3/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the connectors library smoke before the overlay existed
- `linux/overlay/` and `linux/overlay-2/` for preserved harness failures
- `diff/parent.diff` for the four-file change from `65169f37` to `671507b2`
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`

## Decisive exits

`linux/overlay-3`: `blob-cmp.exit` 0, `fetch.exit` 0 (`cargo fetch --locked --offline`), `clippy-component.exit` 0, `test.exit` 0, `hash-compare.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.11s`.

`complete_sha256` is `b28a3f046f008a002d875f236a4d1a797abfad9d2dc02bb5c0c0463bb82b0a60`. `tsv_complete_sha256` is `09028e8ceb769f47c9e86873c8150db9a62fc2e76869c53c3a001c2e7195d7fe`. Profile preimage `77ce0318c38552c2ecd213138d0015a9b9d7061dfc45ef04ef9c59a297596dc8`. Config preimage `47744bd6ee4674d49a5934a18a19bcf8809da7e08e5955a493e1bc778efeaeaf`. `linux/overlay-3/hash-compare.txt` shows `sha256sum` of the saved fixtures equal to the Rust digests.

Clippy for that run is `cargo clippy --locked --offline -p oo-connectors --test delimited_reader_a199_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.25s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/preoverlay`, before any overlay file was copied: `fetch.exit` 0, `lib.exit` 0 (`64 passed; 0 failed`, 0.01s), `test-delimited.exit` 0 (`25 passed`, 0.06s), `clippy.exit` 0 on `--all-targets` (19.22s), `docker-run.exit` 0. Rust `1.94.0` on `linux/arm64`. Container `90525eebe3c3`.

## Preserved attempts

- `linux/overlay`: harness called `budget` with `max_total` 4 below `max_record` 8. `test.exit` 101, `docker-run.exit` 101, `clippy.exit` `not_run`. Container `ee158927af74`. Five tests passed.
- `linux/overlay-2`: `field` dropped its named lifetime and failed `E0106` at compile. `test.exit` 101, `docker-run.exit` 101, `clippy.exit` `not_run`. Container `db7ea899c18f`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `671507b2`, private cargo and target volumes, private network, `HOME=/tmp`. Copy `overlay/delimited_reader_a199_review.rs` onto that archive as `crates/oo-connectors/tests/delimited_reader_a199_review.rs`. Install clippy in the same container. `sha256sum` must be on `PATH`. `OO_A199_ORACLE_DIR` receives the fixtures and preimages. No PostgreSQL.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-connectors --test delimited_reader_a199_review -- --nocapture
cargo clippy --locked --offline -p oo-connectors --test delimited_reader_a199_review -- -D warnings
```

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust. It checks the empty digest and `abc`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` in the pinned image. `linux/manifest-1/` is the preserved first attempt: `bash -lc` reset `PATH` and `rustc` was not found (`docker-run.exit` 127, container `a240d3df5259`). `linux/manifest-2/` is the preserved second attempt: `rustc -D warnings` rejected an unused `Read` import (`docker-run.exit` 1, container `4446d2aa7fc7`). `linux/manifest-3/` is the preserved third attempt: `/tmp` is mounted `noexec`, so the compiled binary returned `Permission denied` (`docker-run.exit` 126, container `52ef347364eb`). `linux/manifest-4/` is the preserved fourth attempt: a `/build` tmpfs was also `noexec` (`docker-run.exit` 126, container `ce155b9f44d4`). The corrected launch compiles to `/usr/local/bin/sha256_manifest` on the container root filesystem. `linux/manifest/console.log` is the stdout of that container. A later run of the same binary rewrote `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, and the exit files were closed. That later stdout stayed outside this directory.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a199-eef831aa-net` (`network_rm_exit=0`, 0 containers attached, owner `a199`), volumes `oo-a199-eef831aa-cargo` and `oo-a199-eef831aa-target` (`volume_rm_exit=0`, 0 users), and `/tmp/oo-a199-eef831aa-dba4-4af2-a78b-541bc9e0a199` (`tmp_rm_exit=0`, absent afterward). The pinned Rust image stays. `docker prune` was not run. `oo-a201-red2`, `oo-a200-pg`, and `oo-a198-pg` were still present afterward. PostgreSQL was not started.

The evidence commit contains only this directory.
