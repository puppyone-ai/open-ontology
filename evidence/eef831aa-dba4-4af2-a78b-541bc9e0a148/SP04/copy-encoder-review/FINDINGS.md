# Findings — a148 COPY encoder

No product change. The bounded encoder at `d80472d8a99afd66d8d0e71420f7be2bb62e65c8` matches `contracts/pipeline/copy-encoder-v1.md` on the checks below. Line numbers are that commit's `crates/oo-pipeline/src/copy_encoder.rs` unless another file is named.

## Column order and cells

`CopyColumn::SystemPosition` must be index 0 (`open`, lines 218–219). A missing user column is `zero_user_columns` (lines 228–229). User columns resolve through `resolve_user` (lines 864–886): unsupported plans, including money `money_locale`, return `unsupported_column` and do not take a cell slot; lossless plans do, in plan order, which follows attnum. The COPY list then selects a subset. In the fresh fixture, attnum 9 (`extra`) occupies batch slot 7 and is omitted from the COPY list. Slot 0 of the batch is `raw`, because attnum 1 is money. The pulled tuples have 18 fields. PostgreSQL's table attnums are the physical table order and include `pos` as attnum 1 because the system position was loaded as a numeric column. That display order is the table, not the batch field order.

`arrow_dimensions` (lines 140–145) returns `TypedBatch::record_batch` row and column counts for the lossless fields. The probe matched those counts to `batch.rows` and `batch.fields`. For a present cell, the Arrow column equals `cell.arrow`. For the unavailable extra cell, the Arrow column is Null and the cell still holds `skip-secret`. `present` (lines 569–599) encodes SQL NULL as length `-1` only when the column is nullable. Unavailable uses the reject reason, or `unavailable`, and is not encoded as NULL. The masked length stays on `batch.rejects` after a successful pull. The secret and the unselected `not-in-copy` text are absent from the COPY bytes.

## Encoding

`float8_of` (lines 640–655) sends the Arrow `f64` bits. `float_class_matches` (lines 1008–1018) accepts `FloatClass::Zero` for both signs, because `classify_f64` treats `-0.0` as zero. `float4_of` (lines 658–668) requires an exact finite `f32` widening and returns `float4_nan` or `float_precision` otherwise. PostgreSQL `float8send` / `float4send` returned `8000000000000000`, `80000000`, `7ff8000000000001`, and `3fa00000`.

`date_of` (lines 711–729) checks the civil date, the Arrow `Date32` days, and subtracts `PG_UNIX_DAYS` (10_957). `time_of` (lines 732–745) requires microseconds in `[0, 86_400_000_000)`. `timestamp_of` (lines 748–789) checks the civil timestamp against the Arrow microseconds, then subtracts `PG_UNIX_MICROS`. The server showed `2024-02-29`, `1969-12-31`, `01:02:03.000004`, `2024-02-29 23:59:59.000001`, and `1999-12-31 23:59:59.999999` in UTC.

`numeric_layout` / `write_numeric` (lines 1034–1080) write base-10000 `ndigits`, `weight`, sign `0x4000` when negative, and `dscale`. Scale below zero is `numeric_scale` / `negative` at lines 800–801 and 943–944, before any rewrite. The server showed `-10000.5000` and `0.0001` for `numeric(12,4)`, and the 20-digit position `12345678901234567890`. Bytea, uuid, json text, and varchar were the stored bytes. `bpchar` left the encoder as `ab`. The server stored octet length 4 and `rtrim` returned `ab`. That padding is `bpchar` receive behavior. The contract says the encoder does not pad.

## Domain

`plan_domain` in `crates/oo-connectors/src/types/mapping.rs` lines 182–220 copies the base plan, sets `oid` to the domain oid, and leaves `decode_oid` as the base. `kind_of` (lines 903–924) switches on `decode_oid`. A domain over bool encodes as bool. A domain over int4 encodes as int4. A domain over `varchar` uses the domain `typtypmod` character limit (`char_limit`, lines 959–966). The plan metadata records `oo.pg.domain`. It does not record a CHECK expression. PostgreSQL rejected the emitted int4 `-1` for `CHECK (VALUE > 0)` with SQLSTATE `23514`. Emitting the base bytes is not a claim that a domain check passed.

## Budgets, backpressure, poison

`open` rejects a zero value budget, row budget, or row count, and a chunk cap below 19, with `closed_budget` (lines 208–213), before a chunk buffer exists. `pull_rows` measures `row_len` first (lines 318–321). `row_len` calls `payload_len`, and `payload_len` calls `budget_value` (line 436) before the chunk `Vec` is allocated at line 340. A value over `max_value_bytes` or `i32::MAX` is `value_budget` (lines 536–539). A tuple over `max_row_bytes` is `row_budget` (lines 378–381). A tuple that cannot fit an empty chunk is `chunk_budget` (lines 322–328). A row that fits the row budget and not the remaining chunk ends that pull; the next pull continues. `poison` (lines 359–362) stores the error. Later pulls return it. Pieces already returned stay returned.

## Refused source types

`kind_of` returns `type_not_encoded` for a plan element (`array`), nonempty `enum_labels` (`enum`), `jsonb`, `interval`, and any other oid. Money on an `Unsupported` plan is `unsupported_column` / `money_locale` from `resolve_user`, which matches the contract line that a non-lossless plan takes that path. The fresh probe opened each of those and got the contract code. This is the explicit refusal list. It is not a full type matrix.

## Harness notes, not product defects

Attempt 1 did not compile: two `positions()` temporaries were dropped while `CopyEncoder` still borrowed them, and `String::push` received a byte. Attempt 4 expected one reject; `assemble_batch` also records unsupported plans (`batch.rs` lines 222–234), so money added two more. Attempt 5 passed the test and failed clippy on two helpers with eight arguments. Attempt 6 is the green encoder run. Attempts 2 and 3 failed because Docker Desktop did not see the recreated `/tmp/.../oracle-src` directory after an earlier mount of that path; a new directory name mounted. Attempt 6's `docker cp` into the Postgres tmpfs did not create `/tmp/load.sql` for the server. Attempt 7 wrote the same COPY bytes through `docker exec` and the decode matched.
