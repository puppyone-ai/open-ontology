# a185 fixed bounded JSONL reader review

Verdict: **APPROVE_LIMITED**.

Fixed source `47743688d45ec7a001e796725febc5357b7e20bd`, tree `14b54c719c69d51fd87784dd9cc98f0c41b24668`, parent `ce040593f6af6a61a5c3e29abd494b1a863f32c5`. Author evidence `753423b7` is the implementation record. It is not this proof. Reviewer `eef831aa-dba4-4af2-a78b-541bc9e0a185`. No product file was edited.

This approval covers the `oo.source.jsonl/v1` pull over `std::io::Read`: an explicit profile, series-60 `RecordBatch` values for `utf8`, `bool`, `int64`, and `json_text`, and `JsonlComplete` only after EOF, profile coverage, and the whole-object SHA-256. `admits_import` and `published` are false. Arrow IPC, a file loader, process isolation, CSV, TSV, Parquet, DuckDB, GB18030, and SP05-W02 as a phase stay open.

## What was executed

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`. Private network `oo-a185-eef831aa-net`, cargo volume `oo-a185-eef831aa-cargo`, target volume `oo-a185-eef831aa-target`. Cap drop `ALL`, no new privileges, memory 8g, no host network, no PostgreSQL, no privileged mode. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. The archive is `git archive` of `47743688` for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. The overlay test was copied onto that archive only.

| Run | Result |
|---|---|
| `linux/preoverlay` | Before the overlay. `fetch.exit` 0, `clippy-component.exit` 0, `lib.exit` 0 (64 passed, 0 failed), `clippy.exit` 0 (`Finished dev profile` in 12.01s), `overlay-absent.exit` 0, `docker-run.exit` 0. |
| `linux/overlay` | Harness bug. `budget(4, 1024, 32, 8, ...)` rejects `max_total` because 8 is below `max_record` 32. `test.exit` 101. Seven other tests passed. Log kept. |
| `linux/overlay-2` | Eight tests passed. `test.exit` 0. Fresh container had no clippy component. `clippy.exit` 1. Log kept. |
| `linux/overlay-3` | Clippy installed. Eight tests passed. `clippy.exit` 101 on `needless_lifetimes` in the harness. Log kept. |
| `linux/overlay-4` | Decisive. `blob-cmp.exit` 0, `fetch.exit` 0 (offline), `clippy-component.exit` 0, `test.exit` 0 (8 passed, 0 failed, 1.92s), `clippy.exit` 0 (`Finished dev profile` in 0.22s), `docker-run.exit` 0. |

`sqlx-postgres` still prints two `unfulfilled_lint_expectations` warnings from `message/flush.rs:11` and `message/mod.rs:76`. Those files are vendor code. The oo-connectors clippy command still exited 0.

## Oracle

The harness hashes with `sha2` 0.10.9 at its own call site. It does not call a library hasher. `sha256sum` on the temporary file agrees.

Fixture `linux/overlay-4/oracle/fixture.jsonl`, 186 bytes:

`d988fc1a3142a23b8cd89fe4483f7159ec6525808bcf6879769b7e2a1d75ef11`

That digest is the Rust oracle, `sha256sum` of the temp file, `fixture.sha256`, and `JsonlComplete.object_sha256` (`complete_sha256` in `linux/overlay-4/test.stdout`). A host `shasum -a 256` of the saved fixture file produces the same digest.

Profile preimage SHA-256 `c99e1d7d9e07979e671057a0169ad1dec3b448bcc2482167b2e2c6a6842b15a5`. Config preimage SHA-256 `63133140d481e6b446f4301bd043d3144f4b41982a1d8deaa789fdc3645163b9`. Both match `JsonlProfile::profile_digest` and `config_identity` for this fixture. The preimage text is in `linux/overlay-4/oracle/`. Budgets are absent from both preimages.

## Requirement results

The decisive test is `crates/oo-connectors/tests/jsonl_reader_a185_review.rs` on the archive copy. Product lines below are `source/blobs/jsonl.rs`, which matches the fixed blob (`55056174f5ac1216c23590471b6ed40e84f2e84de940626bc27f564aeae99764`).

| Check | Result | Product lines |
|---|---|---|
| Temp file, 3-byte reads, explicit profile, Arrow `utf8` / `bool` / null / `int64` min and max / exact `9007199254740993` / nested `json_text` bytes | Passed in `explicit_profile_projects_arrow_and_matches_oracle_only_at_eof` | `898:936`, `1202:1282` |
| Lineage, whole-object hash, preimage | Passed. Row 2 is line 2 after CRLF. Missing `j` is `Missing` at `{`. JSON null `name` is `Null` at the key quote. | `824:840`, `1015:1023`, `1161:1180`, `278:301` |
| Raw file SHA matches the token at EOF | Passed. Prefix hash `ROW0` and a flipped digest return `hash_mismatch` and `finish` does not return `JsonlComplete`. | `754:781`, `824:840` |
| Partial batch, poison, cancel, drop | Passed. `finish` before exhaustion is `not_exhausted`. Unknown key `SHOULD_NOT_LEAK` is `unknown_key` and the display omits that text. Cancel before a read performs 0 reads. `cancel` returns `JsonlCancelled`. Cancel after `Exhausted` still refuses the token. Drop releases the reader with 0 reads. | `754:793`, `612:615`, `942:949` |
| Bounds before read, backpressure, totals | Passed. A past deadline and `max_columns` below the profile width fail in `open` with 0 reads. `batch_bytes` 7 splits two 7-byte records into two pulls, then a token. `max_rows` 1, `max_record` 4, and `max_total` 8 refuse a token. | `621:636`, `695:748`, `1042:1068` |
| Missing, unknown, duplicate, types, no f64 loss | Passed. `1.0`, `1.5`, and `1e2` are `not_int64`. `+1` is `type_mismatch`. `01` is `invalid_number`. Values outside `i64` are `int64_overflow`. `-0` is integer 0. `json_text` keeps the digits `9007199254740993`. Duplicate `\u006e` and a nested duplicate key are `duplicate_key`. | `1125:1130`, `1253:1268`, `1318:1319` |
| Profile has no PII, inference, native execution, or import grant | Passed. Header and token `admits_import`, `published`, `arrow_ipc`, and `executes_native` are false. A number on a `utf8` column is `type_mismatch`. Debug text of the profile has no `pii` or `semantic` field. | `186:247`, `445:452`, `661:681` |
| EOF, unterminated final record, CRLF | Passed. `{"n":1}` with no newline completes. Trailing CRLF completes as one row. `{"n":1}\n{"n":` returns `truncated` after the first row and does not mint. A bare `{"n":1` is `truncated`. | `978:985`, `1086:1150` |
| Blocking `Read` | Passed. Cancel during `read` slept 411 ms, then `cancelled`. Deadline during `read` slept 1507 ms, then `deadline`. The read ran to the harness release. This is not a proof that every blocked read has a deadline. | `304:311`, `942:949`, `1026:1064` |

`queue_batches` is 1 (`426:428`). `resumable` on the batch is false. Arrow field order is the profile order when the second object lists keys in another order. `json_text` metadata is `oo.jsonl.logical=json_text` and `oo.jsonl.encoding=raw-json-utf8`. The nested value `{"z":1, "b":[true, null]}` and the JSON string `"kept"` (quotes included) are the original bytes.

## Source blobs

| File | SHA-256 |
|---|---|
| `source/blobs/jsonl.rs` | `55056174f5ac1216c23590471b6ed40e84f2e84de940626bc27f564aeae99764` |
| `source/blobs/lib.rs` | `b376d8190029591d7f0c46e5fbf9da6cb69a610e3369b2b485a1c47f897629fb` |
| `source/blobs/jsonl_reader.rs` | `ff104861ebe6c856e801b510b0b88226d6ee69cb08e3deb1ee5c7d5eca3f55d3` |
| `source/blobs/jsonl-reader-v1.md` | `f0aab42b850a225bafd1fb30c0d2d983113e112a20701daa14ef6817e5ee987d` |

`linux/overlay-4/source-sha256.txt` records the same four digests inside the container. The overlay harness digest there is `c7b857c069adba56b1970397f2830f4d425d91c24ab315debfd57734289066ed`. `diff/parent.diff` is the four-file diff from parent `ce040593` to `47743688`.

## Scope left open

SP05-W02 still includes CSV, TSV, Parquet, encoding sniff, GB18030, DuckDB isolation, and a restricted parser process (`specs/open-ontology-v0.1-plan.md` SP05-W02, `docs/03-sources-and-ingestion.md` JSON Lines row, AT-FILE-02). This reader is an in-process `Read` pull. `lib.rs` lines 14–15 name `JSONL_PROTOCOL` as a separate file pull and say it does not admit an import. There is no decimal Arrow column. `ExactDecimal::parse_json_number` is the integer gate only (`1253:1268`). A stalled `Read::read` is not interrupted (`304:311`). The deadline is checked in `guard` before `read`, not during it (`942:949`, `1026:1064`).

Cleanup of the owned network, volumes, and `/tmp` archive is in `CLEANUP.txt`. The pinned Rust image stays. `docker prune` was not used.
