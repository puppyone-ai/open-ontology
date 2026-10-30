# C5+ integration contract (W1-F)

Status: W1-F2 implements pinned scheduling, a MySQL layered producer and Gateway
projection. Agent DB/Loader/admission are delegated to W1-F1; real Loader integration
is not verified here. F2 baseline: `1b4393ceba4c5142c4cea2acbc1dd2f3907f469a`.
No new scheduler, Loader, admission mechanism or publication system is introduced.

## One round and the Loader boundary

Keep `c5_mode=full` as the absent-field default. Explicit `layered` uses the
existing occurrence, lease and source/target generation gate. L1 is positive
hinting only (NULL or unchanged UPDATE_TIME never suppresses L2/L3/L4). L2
reads an indexed business-time window with overlap; its watermark is only an
observation, not a trusted cursor. L3 compares identical, fixed source ranges
against the last **completed** source digest scan. L4 invokes the existing full
reader on its configured deadline, regardless of digest equality. Validate the
frozen source schema before each block; drift stops that table with SCHEMA_DRIFT.

Each changed block is a complete replacement of that source-key range. Stage
its complete typed Arrow rows, bind source/schema/config/Arrow digests through
source-admission, then use the existing Loader and publication path. Do not send
an Arrow block as `full_snapshot=true`: `native_update::key_plan` maps that flag
to the **whole-table** snapshot shape. The v1 partition protocol is also not a
substitute: it requires C4 and an OLAP source. Therefore introduce an additive
`oo.pipeline.native-c5-range/v1` request, leaving both existing protocols intact.
Its sealed scope must bind table, schema fingerprint, chunk topology revision,
chunk ID, `(lo, hi]` typed boundaries, prior membership revision, parent/source
generation, and complete Arrow digest (including an empty block). Reject unknown
or mismatched scope before writing. Never accept arbitrary target-side WHERE SQL.

For strings, source collation defines boundaries; PostgreSQL order does not.
Persist `chunk ID -> typed key identities` in Agent DB (ADR-020). Under the existing
fence, replace members absent from the new block with DEC-07 tombstones and apply
new rows. A newer generation's insertion of the same key supersedes its tombstone;
an old/replayed generation cannot resurrect deleted values. Update membership,
receipt, quality and the block checkpoint in **one target transaction**. Checkpoint
advancement requires the actual sealed Loader receipt, not producer success.
Crash before commit retries the same immutable batch; crash after commit replays
the receipt without duplicate application. No scan promotion while any block is
pending; topology changes require new IDs and a full read before parent retirement.

## Durable state and scheduling

Choose Agent DB `_oo`, rather than control DB or JSON: only Agent DB can atomically
couple progress with Loader receipts. Append a migration (never edit old checksums)
for scan identity/status/next-block, chunk bounds/row count/previous and current
128-bit digests/scanned_at, typed key membership, and recent-window observation.
Use existing owner/runtime roles, fixed-search-path security-definer entry points,
existing generation fence and no direct runtime catalog DML. A completed scan is
promoted atomically; an interrupted scan resumes its first unreceipted block and
must not infer deleted membership from missing/unscanned blocks. JSON remains only
a prototype API, never the authoritative state of a running layered binding.

The installed, pinned binding config supplies independent L1/L2/L3/L4 periods,
window/overlap, duty fraction, Threads_running threshold, server/client deadlines,
and execution window (with explicit timezone). L3/L4 obey the execution window;
one source connection and bounded chunks enforce pressure limits. Persist each
layer's due/completion state: L1 activity must not postpone L3/L4 indefinitely.
Do not derive these settings from producer output or repin on retry.

## Versions, quality and freshness

A successful check is not a data generation. When typed reconciliation finds no
change, retain source generation and existing `oo.jobs.native-c5-nochange/v1`
(`data_gen=false`, `apply=false`, `ack=false`); update observational check/scan time
without fabricating a Loader data receipt. A changed batch advances the existing
source generation only through sealed loading and acknowledgement. Multiple changed
blocks may be bounded batches with distinct generations; no round-wide atomic cut
is claimed. Ontology/Pack version changes only through existing reviewed publication,
not because a digest scan or new data generation occurred.

Add nullable C5 observations to `_oo.table_quality` and propagate them through the
existing quality reader, `/v1/freshness`, and query warnings: `c5_mode`,
`last_full_digest_scan_completed_at`, `recent_window_watermark`. Preserve
`capture_method=c5_diff`, `delete_detection=reconcile`, `DELETES_LAGGED`, B for
multi-snapshot chunks, CROSS_TABLE_SKEW and source time `unmeasured`. Do not turn
scan timestamps into source commit times or `snapshot-zero/v1` into ordered source
positions. Existing C5 per-table receipt readiness remains authoritative. C1
handoff still needs L0 capture, repair scan, CDC-wins fencing and L_end publication;
layered observations cannot independently upgrade consistency to T.

`native_schedule::c5plus_policy` now runs when creating producer input for a C5
layered occurrence. Configuration and due flags are stored in the existing immutable
`occurrences.producer_input`; retries reuse them, and operator overrides cannot
change C5 policy. `_oo` remains responsible for **data-plane** scan/chunk state.
`jw_native.c5_schedule` stores only **scheduling** progress, due flags and the pinned
check time. Completion is updated in the existing successful task transaction, after
verified Loader success or native-C5-nochange, never on producer exit alone.

Installed config accepts explicit `execution_timezone: UTC`; other zones fail
validation rather than implicitly using host/server time. `[0, 1440]` means all day.
The worker computes time eligibility; the producer checks real source load. Period
order is L1/L2/L3/L4. Poll interval must not exceed any configured layer period.
See `../mysql-preview/C5-LAYERED.md` for invocation and the F1 authority adapter.

## Wiring and verification gates

The producer calls
`oo_connectors::mysql::chunk::{next_boundary, range_where, read_chunk}`. `range_where`
returns a leading ` WHERE ` fragment plus typed values; strip only that prefix for
`c5plus::digest_sql` and convert each value losslessly to mysql_async parameters.
`read_chunk` owns/returns the connection and provides bounded typed Arrow. Preserve
source byte keys, immutable boundaries and chunk identity; never compare varchar
bounds in Rust or PostgreSQL. No chunk.rs edits or replacement boundary generator are needed: `c5_layered.rs`
already calls all three public functions. `Scanner::into_connection` only transfers
the same connection to the typed reader; digest/reconcile algorithms are unchanged.

Required gates before enabling layered: new-install/upgrade/checksum migrations;
independent layer deadlines/load/window/resume unit tests; owned MySQL 5.7 plus
PostgreSQL sealed Loader tests for integer and varchar keys covering initial load,
insert/update/delete/key reuse/nochange/interrupted scan/SCHEMA_DRIFT; receipt and
checkpoint rollback atomicity; Gateway fields and unchanged per-table readiness;
full/C1 baseline comparison. No customer access or real-source calibration is
part of this worktree. F2 mock-loader fixture gates are implemented; real sealed Loader, Agent DB
atomicity and end-to-end worker/admission integration remain W1-F1 merge gates.
