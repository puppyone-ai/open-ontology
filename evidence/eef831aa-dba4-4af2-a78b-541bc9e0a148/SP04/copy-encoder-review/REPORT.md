# COPY encoder review

Verdict: **APPROVE LIMITED** for the bounded `copy-encoder/v1` primitive at `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`.

The primitive pulls PostgreSQL binary COPY pieces from a `TypedBatch`: a 19-byte header, row chunks, a trailer, and `Done`. This approval covers that encoder. It leaves the Loader, source-type certification, SP04-W02, advisory fences, shadows, publications, and receipts untouched. The ledger stays unchecked.

## Source

Reviewed commit `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`, tree `a3b10827acfac28c4abf986831cbcb26823477dc`, parent `8e0d1da658d65878be96f6f2a7a4478dd3222b19`. Workspace HEAD when the archive was taken was `3df150451cd038ee21d1bd73973afde8eb402c84`. The encoder paths were clean against the fixed commit. This review did not move HEAD and did not edit production, the manifest, the lockfile, the spec, or the ledger.

Smoke used `git archive` of that commit with no overlay. The oracle used the same archive plus `crates/oo-pipeline/tests/a148_copy_oracle.rs`. Git blobs at the fixed commit:

| path | blob |
|---|---|
| `contracts/pipeline/copy-encoder-v1.md` | `9b1d8f12be86675f35604bbb2e31d5fa3db57248` |
| `crates/oo-pipeline/src/copy_encoder.rs` | `8eb2224204ea3f6e57d8407ce218bab790a8059d` |
| `crates/oo-pipeline/src/lib.rs` | `2e0fa8e0fe4de6f2d72ae2e7b73cd4881805c556` |
| `crates/oo-pipeline/tests/copy_encoder_live.rs` | `2021732e21dac0ff7a280336c3a5e44fb44238af` |
| `crates/oo-pipeline/src/physical_mapping.rs` | `56e00c301bd1676e680c031c2e80bb5e419263a5` |
| `crates/oo-pipeline/Cargo.toml` | `d8b180d705ef04641498504565264e48998ae499` |
| `contracts/source/pgsql/type-conversion-v1.md` | `a86eb4595a3d71743eb2a7fd81e9e48dcfcf20ae` |

Content SHA-256 of those seven files, computed from the archive, is in `linux/smoke/expect-*.sha256`. The container compared them before the package test.

## Images and runs

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. Both were already local. Pull was never. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo `1.94.0 (85eff7c80 2026-01-15)`. Server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num=170010`.

Capacity before smoke: `running_rust_builders=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. One Rust builder at a time, on private volumes `oo-a148-eef831aa-cargo` and `oo-a148-eef831aa-target`, network `oo-a148-eef831aa-net`. Rust flags: cap-drop `ALL`, `no-new-privileges`, memory 8g, `HOME=/tmp`, tmpfs `/tmp`, no published ports, no host network, no socket, no host home, no shared cargo cache. Postgres flags, captured on the decode run: privileged false, cap-drop `ALL`, cap-add `CHOWN DAC_OVERRIDE FOWNER SETGID SETUID`, memory 1g, tmpfs `/tmp`, empty port bindings, private network. Auth for that postmaster was trust. No host auth environment was passed.

| attempt | where | result | exit |
|---|---|---|---|
| smoke, pure archive | `linux/smoke` | fetch, package test, clippy | 0 |
| oracle 1 | `linux/oracle` and `linux/oracle-attempt-1` | harness compile: position temporaries and hex digits | test 101 |
| oracle 2 | `linux/oracle-attempt-2` | Docker could not see a recreated `/tmp` snapshot | 125 |
| oracle 3 | `linux/oracle-attempt-3` | same `/tmp` path, same mount error | 125 |
| oracle 4 | `linux/oracle-attempt-4` | test ran; `rejects.len()` was 3 | test 101 |
| oracle 5 | `linux/oracle-attempt-5` | test passed; clippy `too_many_arguments` | clippy 101 |
| oracle 6 | `linux/oracle-attempt-6` | test 0, clippy 0, container 0; `docker cp` into tmpfs hid `load.sql` | host 1 |
| psql 7 | `linux/oracle-attempt-7` | decode, schema, count, rollback, domain `23514` | 0 |

Smoke package results, ignored live tests left ignored: `oo-pipeline` lib 7 passed; `copy_encoder_live` 5 passed, 1 ignored; `physical_mapping_live` 1 passed, 1 ignored; `segment_null_correction` 1 passed, 1 ignored; `segment_preflight_correction` 2 passed, 1 ignored; `target_fence_contract` 1 passed. `sqlx-postgres` printed two unfulfilled `#[expect(dead_code)]` warnings. Clippy still exited 0. Those files were not edited.

Oracle 4 and 5 failed in the review test, which is not product code. `assemble_batch` already stores one `money_locale` reject per row for the unsupported plan, so the fresh batch has three rejects, not one masked reject. The later assertion keeps the masked extra-column reject and the money rejects, and checks that `pull` leaves the vector unchanged. Clippy's argument-count lint is allowed on two test helpers. Oracle 6 is the green in-process run. Oracle 7 feeds those same `fresh.copy` and `domain-violation.copy` bytes to PostgreSQL through `docker exec`, because `docker cp` into the tmpfs was not visible to the server.

## Commands

Smoke, inside the container, after the archive hash check:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
```

Oracle 6, offline, on the warmed private target volume:

```
cargo test --manifest-path Cargo.toml --locked --offline --test a148_copy_oracle -- --nocapture
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
```

Exits for smoke: toolchain 0, snapshot 0, clippy-component 0, fetch 0, test 0, clippy 0, docker-run 0. Exits for oracle 6: toolchain 0, snapshot 0, clippy-component 0, test 0, clippy 0, docker-run 0. Exits for the decode: psql-main 0, schema diff 0, rows diff 0, psql-domain 3, and the stderr contains SQLSTATE `23514`.

## Independent oracle

The fixture is schema `oo_copy_a148`, table `fresh`, dropped by rollback. It is a different shape from the author sample `oo_copy_a145`. The public encoder built the COPY bytes. `psql \copy ... FORMAT binary` loaded them inside a transaction that rolled back. `to_regclass('oo_copy_a148.fresh')`, `to_regclass('oo_copy_a148_chk.checked')`, and `to_regclass('oo_copy_a148_chk.pos_int')` were null afterward. Count was 2.

COPY list order, with the system position first: `pos, raw, note, flag, n, small, wide, amount, day, frac, narrow, clock, ts, tstz, doc, token, label, code`. Plan attnum order is different. `cash` (`money`, attnum 1) is unsupported and has no cell slot. `extra` (text, attnum 9) is lossless, sits between `amount` and `day` in the batch, and is absent from the COPY list. The wire has 18 fields. The strings `not-in-copy` and `skip-secret` are absent from the COPY bytes. The Arrow value for that unavailable cell is Null. The cell still holds the text. `batch.rejects` still has the masked reject for attnum 9 after `pull`.

PostgreSQL returned the rows in `linux/oracle-attempt-7/pg-rows.txt`. They match `linux/oracle-attempt-6/expected-rows.txt`. Decoded values include bytea `0001ff4109`, the note with a newline, a quote, and a tab, booleans `true` and `false`, `i64::MIN`, `i16::MIN`, `i32::MIN`, numeric `-10000.5000` and `0.0001`, dates `2024-02-29` and `1969-12-31`, float8 sign bit `8000000000000000`, float4 sign bit `80000000`, NaN payload `7ff8000000000001`, float4 `3fa00000`, time `01:02:03.000004`, timestamps `2024-02-29 23:59:59.000001` and `1999-12-31 23:59:59.999999`, json `{"a": 1}`, uuid `00010203-0405-0607-0809-0a0b0c0d0e0f`, varchar `éé` at 4 octets, and `character(4)` displayed as `ab` with stored `octet_length` 4. The encoder's own field for that `bpchar` is the unpadded bytes `ab`. The server pads on receive. Empty varchar is octet length 0.

A second script created domain `pos_int AS integer CHECK (VALUE > 0)` and COPY'd the encoder's int4 `-1` payload. PostgreSQL returned `23514` from `domain_check_input`. The encoder had already emitted the base int4 bytes. A domain check is not applied by this primitive.

The in-process probe also refused `jsonb`, `interval`, arrays, enums, and numeric scale `-2` at `open`, refused inexact float4 and float4 NaN on pull, refused a domain `varchar(2)` value of three scalars, and accepted `éé` for that domain. Zero budgets fail at `open` with `closed_budget`. A 1_000_000-byte text fails with `value_budget` and no row piece. A row over 24 bytes fails with `row_budget`. A chunk cap of 19 returns the header and then `chunk_budget`, and the next pull repeats that error. `max_rows_per_chunk = 1` splits the two rows, and the concatenated row bytes match the single-chunk pull. SQL NULL on non-null `n` is `null_not_allowed`. A bool whose Arrow bit disagrees with the logical value is `value_mismatch`. A list that does not start with the system position is `system_position_order`.

`Vec::with_capacity` for the chunk is `copy_encoder.rs` line 340, after `row_len` and the chunk cap check at lines 318–335. `payload_len` calls `budget_value` at line 436 before that allocation. The workspace forbids `unsafe_code`, so this review did not install a global allocator. The budget failures return before a `CopyPiece::Rows`.

## Boundary

`lib.rs` says `copy_encoder` is not the Loader. `CopyEncoder::open` takes a batch, a column list, positions, and budgets. It opens no connection. The `psql` session is test administration of a private postmaster: trust, no published port, rollback, and a null `to_regclass`. It is not an advisory-fenced production sink, and it writes no shadow, publication, or receipt.

Author evidence `9f95d072c09eed4e332d36156d0b31b6b277401c` was read. Its green live fixture was not reused as this oracle.
