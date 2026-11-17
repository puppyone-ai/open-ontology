# Fault injection against the real engine (30 L3 acceptance 2, 10 section 12.5)

Every case runs the real `oo-docs-engine` (or its library in process with the real job scheduler) against a real
PostgreSQL container, the real doc-parser container where parsing is involved, and a real directory. Each case asserts
three things: the affected file ends in the right state with the right code, the other files keep flowing, and no job is
left running without a live lease (`assert_no_stuck_jobs`).

Run: `tools/oo-test/oo-test l3 run --suite super-ontology --only faults --run-id <id>`
(`crates/oo-docs/tests/faults.rs`, features `failpoints`, `--ignored`; raw output in `faults-run.txt`).
The PostgreSQL container is started with `track_commit_timestamp=on` (the `jobs_live` event order test needs it).

| Fault | Test | What it proves |
|---|---|---|
| Parse fails with a retryable error | `a_retryable_parse_error_backs_off_three_times_then_waits_for_a_manual_retry` | backoff 3 times, then `parse_failed` with `retryable`, waits for a manual retry, which finishes it |
| Parser crashes on one file (engine failpoint) | `a_poison_pill_is_quarantined_after_two_crashes_and_the_rest_flows` | quarantined after two crashes, 20 other files publish |
| Extraction fails | `an_extract_failure_is_extract_partial_and_a_manual_retry_finishes_it` | `extract_partial`, manual retry finishes |
| Engine killed (SIGKILL) while a job is leased | `a_killed_engine_resumes_the_leased_job_after_its_lease_ends` | the lease expires, the interrupted file is resumed by the next engine and publishes, the event sequence is gap free |
| **Sidecar killed mid parse** (docker kill of the Docling worker) | `a_sidecar_killed_mid_parse_leaves_the_file_waiting_and_the_rest_flows` | the file waits with a reason, the rest flows, it publishes when the sidecar is back |
| **A file that kills the sidecar worker every time** | `a_file_that_kills_the_sidecar_worker_is_quarantined_and_the_rest_flows` | `DOCS_PARSE_QUARANTINED` after at most 3 attempts, the others publish, no other file was retried |
| **PostgreSQL restarted** under a running engine (`docker restart`) | `postgres_restarted_under_a_running_engine_loses_nothing` | 60 md and 10 csv files all publish, the event sequence has no gap, no stuck job |
| **File still being written** (half written) | `a_file_still_being_written_is_hashed_once_when_it_stops_changing` | settle then hash once, never a version of the partial content |
| **Sidecar answers 5xx** (fault proxy in front of it) | `a_sidecar_that_answers_5xx_holds_only_its_own_formats` | markdown publishes while docx waits with `parser_unavailable`, docx publishes when the sidecar answers again |
| **Object store answers 5xx or 429** | `crates/oo-connectors/tests/s3_mock.rs` (`a_5xx_is_retried_and_the_object_is_read`, `a_429_surfaces_as_throttled_after_the_retries`) | per request retry 200 ms doubling, 4 attempts; `Throttled` honours `Retry-After` |
| Object changed during the read | `s3_mock::a_stale_etag_is_changed_not_mixed_bytes`, `s3_rustfs` (RustFS and MinIO), `download::an_object_changed_during_the_download_is_downloaded_again` | 412 is `Changed`, the download starts over and ends with the new content |
| Corrupt transfer | `s3_mock::a_corrupt_transfer_is_downloaded_again_by_crc64`, `three_corrupt_transfers_end_in_a_checksum_error` | CRC64 mismatch downloads again, three times, then an error |

Sidecar supervision (`sidecar_supervision.rs`, 6 tests): restart after a crash, worker kill, timeouts, oversized input,
health and queue behaviour. Routing (`l3_routing.rs`, 2 tests): every sniffed format goes to the right parser and
duplicates are parsed once.

## Results of the last run

See `faults-run.txt` for the exact lines. Summary: faults 9/9, sidecar_supervision 6/6, l3_routing 2/2, sidecar_formats 7/7
(RATE pdf 7/7, docx 10/10, pptx 6/6), jobs_live 11/11, ingest_fs 14/14, freshness_live 3/3.

## Known limits

- The object store fault is exercised against an in-process look-alike and against RustFS and MinIO for the 412 case; there
  is no test that stops the store (RustFS down) during a sync. The sync then fails with `DOCS_SOURCE_UNAVAILABLE`, nothing is
  swept (a listing that is not complete never sweeps, FM3), but that exact container stop is not automated yet.
- Disk full (loop device) is the disk guard's own test (`disk_guard_live.rs`), not part of this table.
