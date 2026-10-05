# Findings

Verdict: **APPROVE_LIMITED** for `47743688d45ec7a001e796725febc5357b7e20bd`. Line numbers are `source/blobs/jsonl.rs` unless another file is named. The fresh run is `linux/overlay-4`: 8 passed, 0 failed, `test.exit` 0, `clippy.exit` 0.

## Completion token

`JsonlComplete` is constructed only in `finish` (`754:781`). The seal field is private (`523:539`). `finish` returns `not_exhausted` unless `exhausted_delivered` is set, the cancel flag is clear, and `object_sha256` equals the expected digest (`754:762`).

`note_hash` returns `truncated` when `eof` is false, and compares the digest only after that (`824:840`). `pull_bytes` sets `eof` when `read` returns 0 (`1062:1064`). Bytes are hashed as they are read (`1066:1068`). Past `max_total_bytes`, one extra byte is probed and not hashed (`1042:1049`).

`linux/overlay-4/test.stdout` line `complete_sha256=d988fc1a3142a23b8cd89fe4483f7159ec6525808bcf6879769b7e2a1d75ef11` equals `sha256sum` of the temp file and the host digest of `linux/overlay-4/oracle/fixture.jsonl`. `prefix_and_mismatch_do_not_mint` feeds the SHA-256 of the first record and a flipped full digest. Both end at `hash_mismatch`. `finish` returns that error. `partial_poison_and_cancel_do_not_mint` calls `finish` after one row of a three-row file and gets `not_exhausted`.

A hash mismatch can still deliver the last batch. `finish_stream` stores the error and emits the pending rows (`795:803`). The next pull and `finish` return `hash_mismatch`. The batch is not `JsonlComplete`. `admits_import` and `published` on the token are false (`766:779`).

## Projection

`build_batch` uses series 60 `StringArray`, `BooleanArray`, and `Int64Array` (`898:936`). `json_text` is UTF-8 with `oo.jsonl.logical=json_text` and `oo.jsonl.encoding=raw-json-utf8` (`908:931`). `capture_raw` copies the window bytes of the value (`1271:1282`). The test observed `{"z":1, "b":[true, null]}` and `"kept"` with its quotes.

`parse_int64` calls `ExactDecimal::parse_json_number`, rejects `.`, `e`, and `E` as `not_int64`, and then parses `i64` (`1253:1268`). There is no `f64` conversion in this file. The test's integer `9007199254740993` landed in `Int64Array`. Its `f64` rendering differs, which the harness asserts before accepting the cell. `i64::MIN` and `i64::MAX` matched. `-0` became 0. `1.0`, `1.5`, and `1e2` are `not_int64`. `+1` is `type_mismatch` because `+` never enters `parse_int64` (`1242:1246`). `01` is `invalid_number` (`1573:1577`). `9223372036854775808` and `-9223372036854775809` are `int64_overflow`.

Nested objects on `utf8`, `bool`, and `int64` are `nested_refused` (`1217:1221`). A number on `utf8` is `type_mismatch` (`1224:1228`). The reader does not invent a column type from a sample.

## Lineage and CRLF

`bump` increments the line on `\n` only (`1015:1023`). `skip_ws` treats space, tab, CR, and LF as whitespace (`978:985`). The fixture places CRLF between row 0 and row 1. The test requires the second `{` to be line 2 and the bytes immediately before it to be `\r\n`. Trailing CRLF after a finished object completes as one row (`eof_crlf_and_unterminated_final_record`).

A present or null field records the key's opening quote. A missing nullable field records the `{` and `FieldPresence::Missing` (`1161:1180`). The third row has `name: null` (`Null`) and no `j` (`Missing`). Those presences differ. A missing non-null key is `missing_key`. JSON null on a non-null `int64` is `null_not_allowed`.

An unknown key is `unknown_key` (`1125:1126`). A duplicate decoded key is `duplicate_key` (`1128:1129`), including `"n"` and `"\u006e"`. `scan_object` applies the same rule inside `json_text` (`1318:1319`). The error display for `{"SHOULD_NOT_LEAK":2}` does not contain that label.

`{"n":1}` with no newline completes. `{"n":1}\n{"n":` returns `truncated` after the good row. `finish` returns `truncated`. A file that is only `{"n":1` does the same with zero batches.

## Bounds

`open` checks profile width, field cap, and the deadline, then allocates `max_record_bytes` (`621:636`). It does not read. The past-deadline and narrow-column cases observed 0 reads. The test does not trace the allocator. The source order is the allocation proof.

`next_batch` emits the current batch and rewinds a row that does not fit (`714:733`). Two records of 7 bytes with `batch_bytes` 7 produced two batches, then a token. `max_rows` 1 on a two-row file returns `max_rows` and `finish` does not mint. A 7-byte record with `max_record` 4 returns `max_record`. A 16-byte file with `max_total` 8 returns `max_total`. `queue_batches` is 1 (`426:428`).

## Cancel and the blocked read

`read_into` retries `ErrorKind::Interrupted` and otherwise returns `io` (`304:311`). It does not look at the cancel flag or the deadline. `guard` checks both before a pull (`942:949`, and `pull_bytes` calls `guard` at `1027`).

`blocking_read_is_not_preempted_by_cancel_or_deadline` holds `Read::read` until the harness releases it. Cancel was requested during the hold. The read slept 411 ms and then the pull returned `cancelled`. A 400 ms deadline with a 1500 ms sleep slept 1507 ms and then returned `deadline`. `finish` returned the same fact. This shows the flag is observed after `read` returns. It does not show that a blocked read is interrupted, and it does not bound every future read by the deadline.

`cancel` stores the flag and returns `JsonlCancelled` (`785:793`). `Drop` zeroizes the window (`612:615`). The drop case performed 0 reads. There is no `into_inner`.

## Profile

`JsonlProfile` stores an id and columns (`220:223`). `try_new` rejects an empty list and a duplicate name (`228:241`). There is no PII or semantic-class field. The harness Debug check found neither word. Column names reject leading whitespace (`269:276`, fact `column_name`). Header flags are constants false (`661:681`). The same flags are false on a successful token. `JSONL_PROTOCOL` is separate from `oo.source.pgsql/v1` (`lib.rs` lines 14–15 and 60–68). The crate forbids `unsafe_code` (`lib.rs` line 18).

## Preserved harness failures

`linux/overlay` called `JsonlBudget::try_new` with `max_record` 32 and `max_total` 8. `try_new` returns `max_total` (`369:371`). That is the contract. The harness was wrong. The replacement uses `max_record` 8 and `max_total` 8.

`linux/overlay-2` ran the eight tests to exit 0 in a new container. Clippy is not in the image layer. `clippy.stderr` says `cargo-clippy` is not installed. `linux/overlay-3` installed clippy and then failed `needless_lifetimes` at the old helper signatures. The helpers now elide those lifetimes. `linux/overlay-4` is the run that combines the corrected harness, the tests, and clippy.

## Not signed

Do not mark SP05-W02, AT-FILE-02, or the development ledger. Do not treat this parse as an import, a publication, an Arrow IPC write, a file-hosting adapter, or a killable parser process. CSV, TSV, Parquet, DuckDB sniff, GB18030, and decimal columns stay open. The blocked-read limitation above stays in force.
