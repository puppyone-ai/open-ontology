# Findings

Verdict: **APPROVE_LIMITED** for `59be14baab1d5f30abaa2ce8cf67d607a2e4417f`. Line numbers are `source/blobs/validate-file.rs` unless another file is named. The fresh run is `linux/overlay-3`: 10 passed, 0 failed, `test.exit` 0, `clippy.exit` 0, `hash-compare.exit` 0, `docker-run.exit` 0.

No product defect was observed. The preserved `linux/overlay` and `linux/overlay-2` failures are harness compile and clippy failures. Their logs stay in those directories.

## Success is the finish receipt

`main` writes stdout only when `run` returns a document (`168:181`). `run_delimited` calls `reader.finish` only in the `DelimitedItem::Exhausted` arm (`534:554`). `run_jsonl` does the same for `JsonlItem::Exhausted` (`606:626`). Before that return, the command checks `admits_import`, `published`, the object digest, the row and batch counts, the profile id, the profile digest, the config identity, and the protocol. A mismatch is `parse` / `receipt` or `hash` / `hash_mismatch`, and no summary is rendered.

`render_delimited` (`651:689`) and `render_jsonl` (`691:729`) copy header types and the finish counts into `summary_value` (`778:810`). That object has the result protocol, the four closed flags, the digests, the row and batch counts, and a lineage object of counts and offsets. It has no cell text. `finish_document` (`813:817`) appends one newline.

The overlay pulls the same path, folds `line` and `byte_offset` from each emitted batch, and requires `finish` to agree with that fold. On `next_batch` error it requires `finish` to return the same fact and to withhold a receipt (`726:731` and `795:800` in `overlay/validate_file_a204_review.rs`). CSV, TSV, JSONL, and the empty file each matched a handwritten row, batch, presence, and offset table. `batch_rows` was 2. CSV and JSONL emitted batches of 2 and 2. TSV emitted batches of 2 and 1.

CSV stdout lineage is rows 4, fields 16, value 14, null 1, empty 1, missing 0, first line 2 offset 21, last line 5 offset 116. TSV is rows 3, fields 12, value 11, null 1, empty 0, lines 1 and 3, offsets 0 and 38. JSONL is rows 4, fields 16, value 13, null 1, empty 0, missing 2, lines 1 and 4, offsets 0 and 170. The empty file with `header=absent` completes at the empty SHA-256 with every count 0. The command summary for that file uses profile id `a204-empty`.

Object digests from the harness SHA-256, also equal to `sha256sum` of the saved bytes:

- CSV `e316ece441b03a2ef125c5ac5810d3697c8269b54701aecd63d92ff701536445`
- TSV `5ef62398b756b7b6d2d10b9aa305ec49acf9ac92402c0abbfcd1e1f1fda9d66e`
- JSONL `62049328d535525064945cbf32c2a68ef6c2ac16c85989460e73d791097f8958`

Profile and config identities are in `REPORT.md`. They match `profile_digest` and `config_identity` on the finish receipt and on stdout.

## Closed flags and errors

`closed_delimited` (`572:577`) and `closed_jsonl` (`644:649`) reject a header whose `admits_import`, `published`, `arrow_ipc`, or `executes_native` is true. The observed headers and receipts had all four false. Error stderr is one object with keys `fact`, `outcome`, and `protocol` (`946:958`). The fact is the static token from the reader or the command. The harness checked that stderr stays under 256 bytes and omits the input path, the secret field, and the cell text.

`from_delimited` (`819:829`) and `from_jsonl` (`831:841`) map config to exit 3, budget to exit 6, protocol and encoding to exit 5, hash to exit 7, cancelled to exit 8, and io to exit 4. Exit constants are `38:44`.

Observed facts on the compiled process: `header_required`, `inexact_decimal`, `not_bool`, `not_int64` for CSV `01` and for JSONL `1.0`, `bom_refused`, `max_rows`, `hash_mismatch`, `cancelled`, `open`, and `not_file`. Each left stdout empty. The library `finish` call on those same bytes returned the same fact and no receipt, except `open` and `not_file`, which fail in `open_input` (`283:292`) before a reader exists.

## Request bounds

`read_request` (`259:280`) refuses a metadata length above `1_048_576` with `request_bytes` before filling a body of that size, then reads with `take(REQUEST_MAX_BYTES + 1)`. A file of 1048577 `x` bytes returned `request_bytes`. A file of exactly 1048576 `{` bytes was parsed and returned `request_json`.

`parse_json_value` (`237:238`) is the duplicate-key, depth, node, and trailing-byte gate. A request with `protocol` twice returned `request_json`. `exact_keys` (`864:876`) returns `unknown_field` for an extra key and `request_field` for a missing key. JSON null in `protocol` returned `request_field`. `decode_sha256` (`919:934`) requires 64 lowercase hex digits. Uppercase hex returned `expected_sha256`. Deadline `0` and `120001` returned `deadline` (`498:503`). `format=parquet` returned `format`. A CSV profile whose dialect string is `tsv` returned `dialect`. A comma null token returned `null_token`. Type `float` and JSONL type `decimal` returned `type`. A JSONL profile with `dialect`, and a JSONL budget with `max_field_bytes`, returned `unknown_field`. An extra `password` value `SECRET_a204` returned `unknown_field` and the value was absent from stderr.

`--help` alone returns the usage text (`190:194`, `211:232`) and exits 0. Any other argument list is exit 2, `usage` or `unknown_flag`, with empty stdout. The usage text matched the contract sentences listed in `REPORT.md`.

## Path

`execute` opens the input (`240`) before `run_delimited` or `run_jsonl` can set the cancel flag (`517:518`, `589:590`). A missing path named `PATHTOKEN_a204` with `cancel_before_read` true returned exit 4 fact `open`. The path was not echoed. A directory returned `not_file`. A symlink to the CSV was followed and `finish` reported 4 rows. The command does not claim a jail or an authentication decision. `input_path` is not in the success document (`summary_value` has no such key). The harness also asserts stdout does not contain `input_path`.

## Decimal

`oo-pipeline` has no `arrow-array` dependency. This review did not add one. The command's decimal proof on the fresh files is the summary type (`delimited_type`, `731:745`) plus rejection of `1.251` as `inexact_decimal`. The harness separately checked `oo_core::ExactDecimal`: scale 2 coefficients `120`, `0`, `120`, `1000`, and `1250` for `1.20`, `-0.00`, `1.2`, `10`, and `12.50`, with `-0.00` non-negative, and `1.251` inexact. Arrow `Decimal128Array` cells were not downcast here. That inspection remains the a199 public-reader record.

## Clippy

`cargo clippy -D warnings` on the overlay test exited 0. `sqlx-postgres` vendor `#[expect(dead_code)]` at `crates/oo-connectors/vendor/sqlx-postgres/src/message/flush.rs:11` and `message/mod.rs:76` is unfulfilled. The same two warnings appear in the preoverlay clippy log. They did not fail the build. The preoverlay clippy of `--bin validate-file --test validate_file_cli` also exited 0.
