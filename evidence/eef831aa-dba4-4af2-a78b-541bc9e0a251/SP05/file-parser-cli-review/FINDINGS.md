# Findings

Verdict: **APPROVE_LIMITED** for `0a30fcda7fb586bd8d6955e03ae306888374b915`. Line numbers are `source/blobs/parse-file.rs` unless another file is named. The fresh run is `linux/overlay-2`: 12 passed, 0 failed, `test.exit` 0, `clippy.exit` 0, `hash-compare.exit` 0, `docker-run.exit` 0.

No product defect was observed. The preserved `linux/overlay` failure is a review-harness Clippy failure. Its logs stay in that directory.

## Success is ParentParseComplete

`main` writes stdout only when `run` returns a document (`184:197`). With no arguments, `run` calls `execute` (`200:204`). `execute` (`237:266`) reads at most `1_048_576` bytes, rejects credential key names, parses the request, checks `parent_deadline_ms` is `1..=120000`, then calls `ParentConfig::try_new`. It hashes the worker after `try_new` and before `ChildRequest::try_new` and `start`. It pulls until `next_batch` returns `Ok(None)`, then calls `finish`. `summary` is the only success return.

`summary` (`395:488`) reads `KERNEL_RESTRICTION`. The only arm is `Unsupported`, rendered as `unsupported`. A true `admits_import`, `published`, or `pg_run_complete` is fact `admission` and is not printed. `arrow_ipc` must equal `batches > 0`. Reader protocol, profile id, profile digest, config identity, object digest, column count, row count, and batch count must match the receipt and the fold. Field count must be `rows * columns`. A zero-row receipt must have every lineage counter and both byte sums at 0. The JSON object copies those receipt methods and the fold. It has no cell text, no input path, and no executable path. A trailing newline is appended.

The overlay starts a second `ParentConfig` on the same production worker, folds `line`, `byte_offset`, `raw_bytes`, `arrow_bytes`, and presence from `VerifiedBatch`, and requires the CLI document to match that receipt. CSV, TSV, JSONL, and the empty file each matched a handwritten row, batch, and presence table. The harness does not downcast Arrow cells.

CSV stdout lineage is rows 4, fields 16, value 12, null 3, empty 1, missing 0, first line 2 offset 18, last line 5 offset 66. TSV is rows 2, fields 4, value 3, null 1, empty 0, lines 2 and 3, offsets 7 and 14. JSONL is rows 3, fields 9, value 7, null 1, empty 0, missing 1, lines 1 and 3, offsets 0 and 62. The empty file completes at the empty SHA-256 with every count 0. Profile id on that file is `a251empty`.

`lineage_preimage` (`555:585`) is the contract text. The saved CSV preimage begins `oo.connectors.parse-file-lineage/v1` and ends with `arrow_bytes=3288` plus a newline. `sha256sum` of that file equals the stdout `lineage_sha256` and the harness digest (`linux/overlay-2/hash-compare.txt`).

Object digests, also equal to `sha256sum` of the saved bytes:

- CSV `a82155e70d4ccd1787149cde689b4b9acdf9e911e16da7f3fd15dd8a3c746cc1`
- TSV `17765af692fdf1f83732808353e7866dc6bca2c2212815512e5dcdd689b2dd1c`
- JSONL `f31e2f56091c6936ebf31cfec1d8426ee7080046b8f8b84241d6c04e15cd3244`
- Empty `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`

## Closed flags and the worker

The four success documents have `admits_import`, `published`, and `pg_run_complete` false and `kernel_restriction` `unsupported`. `parent_protocol` is `oo.source.file-worker-parent/v1`. CSV and TSV use `oo.source.delimited/v1`. JSONL and the empty file use `oo.source.jsonl/v1`. The command source does not call `DelimitedReader` or `JsonlReader` and does not build an Arrow IPC stream. The batches the parent returned are what made `arrow_ipc` true when `batches > 0`.

`hash_executable` (`295:333`) uses `symlink_metadata`, refuses a non-regular file, caps the file at `2_147_483_648` bytes, and reads 8192-byte chunks under a 120000 ms clock. A digest mismatch returns `Fail::hash("executable_sha256")` at `248:249`, before `start`. The observed stderr has outcome `hash`, fact `executable_sha256`, `reaped` false, `group_signal` `not_needed`, and null `exit_code`, `term_signal`, and `worker_token`. That is the document the contract requires when `start` is not called. This review did not attach a syscall tracer.

A relative worker path fails inside `ParentConfig::try_new` at line 246, which is before `hash_executable`. The observed fact is `worker_path`, outcome `config`, exit 3, `reaped` false. `PATHTOKEN_a251` is absent from stdout and stderr. `from_parent` (`860:871`) maps `worker_path` to `Fail::config`.

A wrong input digest is still passed to `start`. The observed child exit is 6, fact `hash_mismatch`, worker token `hash_mismatch`, `reaped` true, `group_signal` `not_needed`. The paired parent call returned `FailureKind::HashMismatch`. No success document was written.

A malformed quote and an exact header on an empty file are missing completion frames. Both CLI documents have outcome `parent`, fact `ended`, child exit 4, `reaped` true. Worker tokens are `protocol` and `header_required`. The paired parent calls returned `FailureKind::Ended` with those tokens.

## Request bounds that this run exercised

`read_stdin` (`268:293`) stops above `REQUEST_MAX` with `request_bytes`. `reject_credentials` (`837:858`) walks objects and arrays and rejects the key names in `CREDENTIAL_KEYS` (`40:54`), including `password`, before `parse_request`. The observed extra `password` value `SECRET_a251_CRED` returned exit 3, fact `credentials`, `reaped` false. The value and the key name `password` were absent. The fact string itself is `credentials`.

`help_text` (`215:234`) is the `--help` stdout. The test required the protocol, `1048576`, CSV, TSV, JSONL, the closed import, publish, and database sentences, `ParentParseComplete`, `no raw rows`, `clean EOF`, `unsupported`, and `parent_deadline_ms`. The same function lists exits 0 and 2 through 7 and says the two deadline clocks are not added. `--help` plus another argument is `unknown_flag`, exit 2 (`205:208`).

`emit` (`999:1016`) writes one object whose keys are `exit_code`, `fact`, `group_signal`, `outcome`, `protocol`, `reaped`, `term_signal`, and `worker_token`. The harness checked that set on every error document.

## Deadline

`z_deadline_kills_then_the_next_call_completes` calibrates an empty JSONL file against 40000 rows of eight bytes, `batch_rows` 16. Overlay-2 recorded `empty_ms=8929`, `busy_ms=74438`, `kill_ms=32754`, `kill_elapsed_ms=41599`. The killed document is `timed_out` / `deadline` / exit 7, `reaped` true, `group_signal` `sent`, `term_signal` 9, `exit_code` null. The harness accepts either signal 9 with a null child exit, or a null signal with child exit 7. This run, and the preserved overlay run, both took signal 9. The contract says `sent` is the parent's report that `/bin/kill -KILL -- -<group>` returned success. It is not a certificate that every descendant died.

The following call is a new `parse-file` process on the empty file, with a new `ParentConfig::start` inside the harness. It matched the empty receipt. The command source does not reuse a child (`258` is the only `start` on that `ParentConfig`).

The busy summary asserted `admits_import` false and `kernel_restriction` `unsupported`, with lineage value 40000 and last line 40000. That file is the deadline boundary. It is not an extra accepted format.

## Clippy

`cargo clippy -D warnings` on the overlay-2 test exited 0. `sqlx-postgres` vendor `#[expect(dead_code)]` at `crates/oo-connectors/vendor/sqlx-postgres/src/message/flush.rs:11` and `message/mod.rs:76` is unfulfilled. The same two warnings appear in the preoverlay clippy log and in the preserved overlay log. They did not fail the build. The preoverlay clippy of `--bin parse-file --test parse_file_cli` also exited 0.

The preserved overlay Clippy error is `too_many_arguments` on the review harness `document` function, 8 arguments against a threshold of 7. The product package was not the source of that error. The later harness has 7 arguments because the executable digest and path share `ExeOverride`.
