# a199 fixed bounded CSV and TSV reader review

Verdict: **APPROVE_LIMITED**.

Fixed source `671507b229ad7d1bc6a2e948b5b74d6d7252393f`, tree `d85bc4363875c707a2ec582a44a3a53e69e1a278`, parent `65169f374c665d8efdd62a8bcd47317c38a1359e`. Author evidence `123383c090a3848b81d2240a826423c76ce8a112` and `b487f0a171aee739a76dcc8b605ad1246e48dfc5` is the implementation record. Reviewer `eef831aa-dba4-4af2-a78b-541bc9e0a199`. Source archive commit `39d2851cd70b2591768d85025550dde0924bf81a`. No product file was edited.

This approval covers the `oo.source.delimited/v1` pull over `std::io::Read`: an explicit CSV or TSV dialect, series-60 `RecordBatch` values for `utf8`, `bool`, `int64`, and `decimal128`, and `DelimitedComplete` only after exhaustion, profile coverage, and the whole-object SHA-256. `admits_import` and `published` are false. Arrow IPC, a file loader, process isolation, publication, PostgreSQL `RunComplete`, GB18030, sniffing, DuckDB, and SP05-W02 as a phase stay open.

## What was executed

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`. Private network `oo-a199-eef831aa-net`, cargo volume `oo-a199-eef831aa-cargo`, target volume `oo-a199-eef831aa-target`. Cap drop `ALL`, no new privileges, memory 8g, no host network, no PostgreSQL, no privileged mode. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. Each archive is `git archive` of `671507b2` for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. The overlay test was copied onto that archive only.

| Run | Result |
|---|---|
| `linux/preoverlay` | Before the overlay. `fetch.exit` 0, `clippy-component.exit` 0, `lib.exit` 0 (64 passed, 0.01s), `test-delimited.exit` 0 (25 passed, 0.06s), `clippy.exit` 0 (`Finished dev profile` in 19.22s), `docker-run.exit` 0. Container `90525eebe3c3`. |
| `linux/overlay` | Harness bug. `budget(4, 1024, 8, 8, 4, 10)` rejects `max_total` because 4 is below `max_record` 8. `test.exit` 101. Five other tests passed. `clippy.exit` `not_run`. Container `ee158927af74`. Log kept. |
| `linux/overlay-2` | Harness bug. Eliding the lifetime on `field` failed to compile (`E0106`, two input references). `test.exit` 101. `clippy.exit` `not_run`. Container `db7ea899c18f`. Log kept. |
| `linux/overlay-3` | Decisive. `blob-cmp.exit` 0, `fetch.exit` 0 (offline), `clippy-component.exit` 0, `test.exit` 0 (6 passed, 0 failed, 2.11s), `hash-compare.exit` 0, `clippy.exit` 0 (`Finished dev profile` in 0.25s), `docker-run.exit` 0. Container `361b944ccb40`. |

`sqlx-postgres` still prints two `unfulfilled_lint_expectations` warnings from `message/flush.rs:11` and `message/mod.rs:76`. Those files are vendor code. The oo-connectors clippy command still exited 0.

## Oracle

The harness hashes with a Safe Rust FIPS 180-4 SHA-256 in `overlay/delimited_reader_a199_review.rs`. It checks the empty digest and `abc` before trusting a result. It does not call the product hasher. `sha256sum` on the saved fixtures agrees with that digest (`linux/overlay-3/hash-compare.txt`).

CSV fixture `linux/overlay-3/oracle/positive.csv`, 146 bytes:

`b28a3f046f008a002d875f236a4d1a797abfad9d2dc02bb5c0c0463bb82b0a60`

TSV fixture `linux/overlay-3/oracle/positive.tsv`, 68 bytes:

`09028e8ceb769f47c9e86873c8150db9a62fc2e76869c53c3a001c2e7195d7fe`

Those digests are the Rust oracle, `sha256sum` of the saved files, and `DelimitedComplete.object_sha256` (`complete_sha256` and `tsv_complete_sha256` in `linux/overlay-3/test.stdout`).

CSV profile preimage SHA-256 `77ce0318c38552c2ecd213138d0015a9b9d7061dfc45ef04ef9c59a297596dc8`. CSV config preimage SHA-256 `47744bd6ee4674d49a5934a18a19bcf8809da7e08e5955a493e1bc778efeaeaf`. Both match `DelimitedProfile::profile_digest` and `config_identity` for this fixture. The preimage text is in `linux/overlay-3/oracle/`. Budgets are absent from both preimages. Two budgets on the same bytes produced the same `config_identity`. `queue_batches` on the budget value is 1.

## Requirement results

The decisive test is `crates/oo-connectors/tests/delimited_reader_a199_review.rs` on the archive copy. Product lines below are `source/blobs/delimited.rs`, which matches the fixed blob (`12583452df440852b8979d0dfa966966eebbbc53a04433d1c7bc41aa60cd4587`).

| Check | Result | Product lines |
|---|---|---|
| CSV temp file, 3-byte reads, exact header, BOM strip, UTF-8 Chinese, quoted comma, quoted quotes, quoted LF, CRLF, EOF without a trailing newline, int64 min and max, `-0`, decimal `1200.50` / `-0.00` / `12.5` | Passed in `positive_csv_matches_the_hand_fixture_only_at_eof`. 50 reads, max buffer 256, 146 bytes. Coefficients 120050, 0, 1250. | `844:888`, `1168:1214`, `1216:1240`, `1690:1728` |
| TSV, one-byte reads, BOM refuse, header absent, quoted tab, quoted CRLF, int64 extrema, EOF without a trailing newline | Passed. 68 bytes, max buffer at most 256. Row 1 field `n` is line 3 because the quoted field contains LF. | `1216:1240`, `1313:1318`, `1441:1470` |
| Lineage and whole-object hash | Passed. CSV rows are line 2 offset 25, line 4 offset 88, line 5 offset 128. Note presences are Value, Empty, Empty. City presences are Value, Value, Null at offset 128. | `1091:1107`, `1514:1594` |
| Profile and config preimage | Passed. The preimage has no budget fields. A comma null token is refused for CSV and accepted for TSV. Duplicate column names are `duplicate_column`. Separators other than comma and tab are `ambiguous_dialect`. | `489:505`, `557:606` |
| Malformed quotes, width, budgets, hash | Passed. Unclosed and junk-after-close quotes are `malformed_quote` and the display omits `SHOULD_NOT_LEAK_a199`. `max_record`, `max_field`, `max_total`, `batch_bytes`, `max_rows`, and `max_fields` refuse `DelimitedComplete`. A hash mismatch and a prefix digest emit the pending batch once, then `hash_mismatch`. | `523:530`, `956:988`, `1056:1107`, `1321:1362`, `1441:1507` |
| Early stop, cancel, drop, blocking `Read` | Passed. `finish` before exhaustion is `not_exhausted`. Cancel before a pull performs 0 reads. A past deadline fails in `open` with 0 reads. Drop sets the reader-release flag and Debug omits `熊`. | `523:530`, `838:841`, `956:999`, `1253:1260` |
| Blocking read limitation | Passed as an observation of the limit. Cancel during `read` waited 80 ms while the pull had not returned, then `cancelled`, with `reads=1`. An 800 ms deadline with a 2000 ms sleep returned after 2009 ms with `deadline`. | `523:530`, `1253:1260`, `1321:1353` |

`DelimitedComplete` is constructed only in `finish` (`972:987`). Header flags `admits_import`, `published`, `arrow_ipc`, and `executes_native` are false (`904:907`). The same import and publication flags are false on the token (`984:985`). `lib.rs` lines 16–17 name `DELIMITED_PROTOCOL` as a separate file pull and say it does not admit an import. The crate forbids `unsafe_code` (`lib.rs` line 20).

## Source blobs

| File | SHA-256 |
|---|---|
| `source/blobs/delimited.rs` | `12583452df440852b8979d0dfa966966eebbbc53a04433d1c7bc41aa60cd4587` |
| `source/blobs/lib.rs` | `5ca17a403e2e8ee01bf074bc81af8a261ffd639a2aa660610cf0c240aaf0007d` |
| `source/blobs/tests-delimited.rs` | `b33c4b136c1eb0bc772885e34889f6311c67a1ba39225edbcfa38a594f332f0c` |
| `source/blobs/delimited-reader-v1.md` | `3641c6b750f1684c2116e08ec6092fd1ac7ed1df7bce2bc5e613613345c71623` |

`linux/overlay-3/source-sha256.txt` records the same four digests inside the container. The overlay harness digest there is `02d786ea0a43889d3176b662935c98b02973a62839eeb0b751280202f3caa63d`. `diff/parent.diff` is the four-file diff from parent `65169f37` to `671507b2`.

## Scope left open

SP05-W02 still includes Parquet, encoding sniff, GB18030, DuckDB isolation, and a restricted parser process. This reader is an in-process `Read` pull for comma and tab only. A stalled `Read::read` is not interrupted (`523:530`). The deadline and the cancel flag are checked in `guard` before `read`, not during it (`1253:1260`). `DelimitedComplete` is the parse receipt for this pull.

Cleanup of the owned network, volumes, and `/tmp` archive is in `CLEANUP.txt`. The pinned Rust image stays. `docker prune` was not used.
