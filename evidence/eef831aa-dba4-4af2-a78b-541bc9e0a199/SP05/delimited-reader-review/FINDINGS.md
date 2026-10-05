# Findings

Verdict: **APPROVE_LIMITED** for `671507b229ad7d1bc6a2e948b5b74d6d7252393f`. Line numbers are `source/blobs/delimited.rs` unless another file is named. The fresh run is `linux/overlay-3`: 6 passed, 0 failed, `test.exit` 0, `clippy.exit` 0, `hash-compare.exit` 0.

## Completion token

`DelimitedComplete` is constructed only in `finish` (`956:987`). The seal field is private (`747:759`). `finish` returns the stored error when one is set (`957:959`). It returns `cancelled` when the flag is set (`960:962`). It returns `not_exhausted` unless exhaustion was delivered, `hash_ok` is set, names are covered, and `object_sha256` equals the expected digest (`963:969`).

`note_hash` returns `truncated` when `eof` is false, and compares the digest only after that (`1091:1107`). `pull_bytes` sets `eof` when `read` returns 0 (`1355:1357`). Bytes are hashed as they are read (`1359:1361`). Past `max_total_bytes`, one extra byte is probed and not hashed (`1335:1342`).

`linux/overlay-3/test.stdout` line `complete_sha256=b28a3f046f008a002d875f236a4d1a797abfad9d2dc02bb5c0c0463bb82b0a60` equals `sha256sum` of `linux/overlay-3/oracle/positive.csv`. The TSV line `tsv_complete_sha256=09028e8ceb769f47c9e86873c8150db9a62fc2e76869c53c3a001c2e7195d7fe` equals `sha256sum` of `positive.tsv`. `malformed_width_budget_and_hash_do_not_complete` feeds a flipped digest and the digest of the prefix `a` for the file `a\n`. Both emit one batch, then `hash_mismatch`. `finish` returns that error. `early_stop_cancel_and_blocking_read_do_not_complete` calls `finish` before any pull, and again after one row of a two-row file, and gets `not_exhausted`.

A hash mismatch can still deliver the pending batch. `finish_stream` stores the error and emits the pending rows (`1063:1070`). The next pull and `finish` return `hash_mismatch`. The batch is not `DelimitedComplete`. `admits_import` and `published` on the token are false (`984:985`).

An empty object with `header=exact` returns `header_required` before the hash compare (`1056:1058`). An empty object with `header=absent` and a wrong digest returns `hash_mismatch`. The same empty object with the empty SHA-256 completes with 0 rows.

## Projection

`build_batch` uses series 60 `StringArray`, `BooleanArray`, `Int64Array`, and `Decimal128Array` (`1168:1214`). Decimal metadata is `oo.delimited.logical=decimal` plus precision and scale. The CSV batch schema field `amt` is `Decimal128(8, 2)`.

`parse_i64` requires `int_lexical`, then `ExactDecimal::parse_sql`, then a zero scale, then `i128`, then `i64` (`1690:1704`, `1730:1740`). There is no `f64` conversion on this path. The fixture's `i64::MIN` and `i64::MAX` matched. `-0` became 0. `+1`, `01`, and `1.0` are `not_int64`. `9223372036854775808` and `-9223372036854775809` are `int64_overflow`.

`parse_decimal` requires `decimal_lexical`, then `parse_sql`, then `rescale_exact`, then `fits_decimal128`, then the coefficient length against precision (`1706:1728`, `1742:1765`). `1200.50`, `-0.00`, and `12.5` at scale 2 are coefficients 120050, 0, and 1250. `1.20`, `1.2`, and `-1.20` at scale 2 are 120, 120, and -120. `1200` at scale -2 is 12. `12` at scale -2 is `inexact_decimal`. `+1.20`, a leading space, `1.`, and `1e2` are `not_decimal`. `1.251` at scale 2 is `inexact_decimal`. `1000000.00` at precision 8 scale 2 is `decimal_precision`. Thirty-eight nines at precision 38 scale 0 round-trip. Thirty-nine nines are `decimal_storage`.

Quoted text keeps spaces, a bare CR inside quotes, and a quoted comma, tab, quote, or newline. An unquoted CR at end of input is `bare_cr`. Invalid UTF-8 is `utf8`. A NUL is `control_char`. An unclosed quote and a quote with trailing junk are `malformed_quote`. The error display is `delimited error: <kind> fact=<fact>` (`144:151`). The harness checks that Display and Debug omit the poisoned cell bytes and the OS string `SECRET_OS_a199`. An `ErrorKind::PermissionDenied` read becomes `io` / `read` (`523:529`).

## Lineage and dialect

`bump` increments the line on `\n` only (`1313:1318`). The CSV fixture's quoted note contains one LF, so the next record starts on line 4. The TSV fixture's quoted field contains CRLF, so its following `n` field is line 3 at offset 49. Field offsets are absolute from the first read byte. A stripped BOM leaves the first record at offset 3 (`1216:1234`). A later `EF BB BF` is the character U+FEFF. `bom=require` on bytes without a BOM is `bom_required`. `bom=refuse` on a leading BOM is `bom_refused`.

`NullPolicy::Empty` treats an unquoted empty field as null and a quoted empty field as an empty string (`1586:1593`). `NullPolicy::Token` matches only the unquoted token. The CSV null token `\N` is Null at offset 128. The quoted empty note is Empty at offset 144. Every emitted row has one field entry per profile column (`oracle/lineage-csv.txt`).

Width is checked before header name equality (`1001:1009`). `a,b` against one column is `width_mismatch`. A header name that differs is `header_mismatch`. `try_new` rejects a null token that contains the dialect separator (`489:505` in the profile constructor region; the fact observed is `null_token`). A comma token is legal for TSV.

## Bounds

`open` checks profile width, field cap, and the deadline, then allocates one window of `max_record_bytes` (`844:888`). It does not read. The past-deadline case observed 0 reads. The CSV pull saw `Read` buffers of at most 256 bytes, which is that window, across 50 reads of a 146-byte file sliced 3 bytes at a time.

`try_new` rejects `max_total_bytes` below one record (`583:585`). A legal probe is `max_record` 4 and `max_total` 4 on `ab\nXY`. The pull returns `max_total` and `finish` returns the same error. The row parsed before the probe is not emitted.

`consider` emits the current batch and rewinds a row that does not fit (`1015:1053`). `aa\nb\n` with `batch_bytes` 3 produced two batches, each of at most 3 raw bytes, then a token. `aa\n` with `batch_bytes` 1 is `batch_bytes`. `max_rows` 1 on `a\nb\n` returns one batch, then `max_rows`, and `finish` does not mint. `"abcdef"\n` with `max_record` 8 is `max_record`. `abcdef\n` with `max_field` 4 is `max_field`. `max_fields` 1 on `a,b\n` is `max_fields`. `queue_batches` is 1 (`646:648`).

## Cancel and the blocked read

`read_into` retries `ErrorKind::Interrupted` and otherwise returns `io` (`523:530`). It does not look at the cancel flag or the deadline. `guard` checks both before a pull (`1253:1260`). `pull_bytes` calls `guard` at `1322` before `read_into` at `1351`.

`early_stop_cancel_and_blocking_read_do_not_complete` holds `Read::read` until the harness releases it. Cancel was requested during the hold. The pull was still blocked 80 ms later. After release it returned `cancelled`, and `finish` returned `cancelled`, with one read. A deadline 800 ms ahead, with a 2000 ms sleep inside `read`, returned after 2009 ms with `deadline`. `finish` returned `deadline`. This shows the flag is observed after `read` returns. It does not show that a blocked read is interrupted.

`cancel` stores the flag and returns `DelimitedCancelled` (`991:998`). It does not mint `DelimitedComplete`. `Drop` zeroizes the window (`838:841`). The drop case set the harness flag, and Debug of the reader did not contain the fixture text. Cancel before `next_batch` performed 0 reads.

## Profile

The CSV preimage starts `oo-delimited-profile/v1\n` and names dialect, BOM, header, null token, quote, record ending, escape, and the five columns. The config preimage starts `oo-delimited-config-identity/v1\n`, appends that profile text, and appends `expected_sha256=` plus 64 lowercase hex digits. The harness compared both to `profile_preimage` and `config_preimage`. Debug of the profile contained neither `pii` nor `semantic`.

## Preserved harness failures

`linux/overlay` called `DelimitedBudget::try_new` with `max_record` 8 and `max_total` 4. `try_new` returns `Config` / `max_total` (`583:585`). That is the contract. The harness was wrong. The replacement uses `max_record` 4 and `max_total` 4, matching the author probe in `source/blobs/tests-delimited.rs` around the `ab\nXY` case. Five tests had already passed, including both positive fixtures, so the hand digests were already confirmed.

`linux/overlay-2` restored a helper signature that dropped the named lifetime. The function borrows `row` and also takes `name`, so the return needs an explicit lifetime. Compile failed with `E0106` before any test ran. `linux/overlay-3` puts that lifetime back on the row borrow. That is the run that combines the corrected harness, the tests, the `sha256sum` agreement check, and clippy.

## Not signed

Do not mark SP05-W02, AT-FILE-02, or the development ledger from this directory. `DelimitedComplete` does not admit an import or a publication. Arrow IPC, a file loader, a killable parser process, GB18030, encoding sniff, and DuckDB stay open. The blocked-read limitation above stays in force.
