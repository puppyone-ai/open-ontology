# Findings

Reviewed the frozen child `oo-file-parser-worker` at `6bd2455849da2527973c136968d4549ecd98d770`. The decisive process is `linux/overlay-4`. The supply decision is `linux/supply`.

## Child IPC

- The harness spawns the compiled binary with an empty environment and pipes. It decodes each tag-1 payload with `StreamReader`. One frame is one stream: schema, one batch, end of stream. Alignment observed on the success payloads is 8, and the first four bytes are the IPC continuation `0xff 0xff 0xff 0xff`.
- CSV `a224-shelf` yields `i64::MIN`, `i64::MAX`, and `0`, and decimal coefficients `3140`, `3140`, and `-10` at precision 8 scale 3. Quoted empty is empty. The null token `~` is null. A quoted comma stays one field. A quoted newline moves the later field to line 5. The input was a regular file.
- TSV `a224-tab` keeps a quoted tab and a comma inside a field. An unquoted empty field is null. `-0` is stored as `0`.
- JSONL `a224-notes` uses `batch_rows` 1 and writes three IPC frames. Label presence is `value`, `null`, then `missing`. The null offset is the key's opening quote. The missing offset is `{`. `-0` is stored as `0`.
- Lineage on those paths does not contain the checked cell text. Profile, config, and schema digests in the receipt match the harness preimages. The harness does not call library `profile_preimage`.

## Completion

- Empty input with `header=absent` writes one completion frame, `rows=0`, `arrow_ipc=false`, and the empty SHA-256.
- Empty input with `header=exact` exits 4, token `header_required`, and writes zero frames.
- A header-only exact CSV completes with `rows=0` and the shelf profile, config, and schema digests.
- Empty JSONL completes with `rows=0` and `reader_protocol=oo.source.jsonl/v1`.
- A late hash mismatch writes the IPC batch and the lineage frame, then exits 6. There is no completion frame. `config_identity` on that lineage is the digest of the preimage that contains the wrong expected hash. Stderr is `oo-file-parser-worker: hash_mismatch\n`.
- Success receipts set `admits_import=false`, `published=false`, `executes_native=false`, `pg_run_complete=false`, `resumable=false`, `kernel_network=unsupported`, `kernel_filesystem=unsupported`, `kernel_memory=unsupported`, `parent_kill=caller`, `parent_reap=caller`, and `launcher=not_this_process`.

## Bounds

- A request of 1,048,577 `{` bytes exits 2, token `request_too_large`, with zero frames.
- Format `parquet` on an absent absolute path exits 2, token `config`, with zero frames. Stderr does not contain `parquet` or the path. Unknown format is refused before open.
- `relative.csv` exits 2, token `path_form`. A directory, a symlink, and a fifo exit 2, token `not_regular`, with zero frames. The fifo is not opened. The check accepts a regular file on a caller-supplied absolute path. It does not jail the path and does not prove the file immutable.
- The ceiling fixture is 40 decimal columns, precision 2 scale 0, cell `0`, 204 rows, `batch_bytes` 16384. The child exits 8, token `ipc_ceiling`, and writes zero frames. That binds the IPC sink. This review did not measure resident memory. The public reader can allocate the `RecordBatch` before the sink refuses it.
- Holding stdin does not end by `deadline_ms`. The parent test kills the child. `status.code()` is `None`. No completion frame is written. `/proc/<pid>` is gone after the kill and after the drop guard. `parent_kill` and `parent_reap` are the caller.

## Packages

`tools/supply_audit.rs` on the warm cargo volume, with index and API bodies fetched in `linux/supply`, exited 0. Parent registry checksums are unchanged. Nothing was removed. The only additions are `arrow-cmp` 60.0.0, `arrow-ipc` 60.0.0, `arrow-select` 60.0.0, and `flatbuffers` 25.12.19. Crate bytes match the lock, the index, and the API. License is Apache-2.0. MSRV is 1.88 for the Arrow crates and 1.51 for flatbuffers. None are yanked. At clock `2026-10-05T21:49:34Z` the Arrow crates are 20 days old and flatbuffers is 289 days old. The baseline file was not edited.

## Preserved failures

- `linux/preoverlay` `supply-rustc.exit` 1: missing lifetime on `version_window`. Container `7343c8e6a86d`. Author tests and clippy in that same container exited 0.
- `linux/overlay` `test.exit` 101: missing lifetime on `find_field`. Container `4bc7a6b02333`.
- `linux/overlay-2` `test.exit` 101: schema temporary dropped while borrowed. Container `b749080d8fb0`.
- `linux/overlay-3` tests passed and the hash compare matched. Clippy `useless_format` exited 101. Container `9543062601a2`.

a207 `build` exit 101 for `cargo update -p arrow-ipc` is unchanged and was not repeated. a207's Python manifest was not used as a deciding hash.

## Left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while the dependency is compiled. Those files are vendor code and were not edited. Overlay-4 clippy still exited 0.

## Open

SP05-W02, AT-FILE-02, and the phase ledger stay unchecked. This binary is not the parent exporter, a loader, or a public endpoint. Kernel enforcement stays `unsupported`. Publication stays closed. PostgreSQL was not part of this run.
