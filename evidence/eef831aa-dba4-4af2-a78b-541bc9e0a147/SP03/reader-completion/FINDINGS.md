# Findings — a147 reader completion

Fixed input `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`. Production commit `16ca2969cd449653a64d98d9831d3c8b8976942a`. Green evidence is `linux/probe-3`. Probe and probe-2 stay as failed harness runs.

## Admission gap from a146

a146 probe-4 showed `SnapshotOwner::finish` returns `SnapshotFinished` for no stream, an unpulled stream, one batch, and one of two selected tables, with `exhausted_seen=0`. That behavior is now explicit compatibility. `finish` is lifecycle release. It is non-admission.

## What mints `RunComplete`

`RunComplete` is constructed only by `mint_complete` after `StreamSlot::next_item` has delivered `StreamItem::Exhausted` for a scan whose envelope covers every selected table. The bit is an `AtomicBool` shared with the owner. A caller cannot set it. `TableEof` does not set it. `stream_plan_one` leaves `covers_selected` false, and `stream_envelope` recomputes the flag from the bound catalog names and empty bounds.

`finish_complete` still closes both connections when the bit is false, and returns `protocol` fact `not_exhausted`. Cancel, budget, portal or source failure, deadline, current catalog or ACL failure, and failed cleanup return that failure. They do not return `RunComplete`. A `Budget` error from the pull is preferred over a later cleanup error, matching `finish`.

The token carries the source id, generation, config identity, type-policy identity, `PgTypePolicy`, input digest, schema hash, the catalog captured at open, and the selected tables in admitted order.

## Probe-3 proof

`linux/probe-3/complete-outcome.txt`:

| Case | Result |
| --- | --- |
| Two selected tables drained through `TableEof` and `Exhausted` | generation 1, batches 5, item rows 2, line rows 3, `same_cut_match=1` versus the pre-writer oracle by key |
| `finish_complete` on that owner | `RunComplete` generation 1, tables `r4.item,r4.line`, schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db`, digest `9062f567c847ff02b07d7aabfa773cf29d9123bc831efcfe0ce88c0643883659`, both backends absent |
| Fresh generation after the writer commit | generation 2, `pine`, 4 lines, line 13 present, scan `2:oak,1:pine`, `cut_compared_by_key=1`, `old_generation_reused=0`, own `RunComplete` |
| `finish` with no stream | `SnapshotFinished` generation 3, `admission=0`, `close_only=1` |
| No stream | generation 4, `not_exhausted`, `backends_gone=2` |
| Stream opened, no pull | generation 5, `not_exhausted`, `backends_gone=2` |
| One batch, ids `1,2`, table holds 4 | generation 6, `not_exhausted`, `backends_gone=2` |
| `r4e.rows` reached `TableEof`, `r4e.extra` unread | generation 7, `first_eof=rows`, `unread=extra`, `not_exhausted`, `backends_gone=2` |
| Cancel | generation 8, `fact=cancelled`, `run_complete=0` |
| Portal past `max_value` | generation 9, `kind=Budget`, `fact=max_value`, `run_complete=0` |

Every absence sample in that file is `still=0` and `absent_ms=0`. The 5000 ms clock starts after the call returns. Stats use `pg_stat_clear_snapshot`.

`linux/probe-3/finish-outcome.txt` keeps the existing lifecycle test: drained rows return `SnapshotFinished`; cancel and `max_value` leave `snapshot_finished=0`.

Reader statements in the complete window have `dml_ddl_commit_terminate_offset=0`. Cleanup failure is not reported as source success. The `max_value` path returns the budget fact.

## Harness failures preserved

`linux/probe/` exit 1. Clippy was not installed in that container. The smoke container had installed it into a layer that exited with the smoke run.

`linux/probe-2/` exit 101. Two test-only diagnostics: `E0308` on `error_label(finished.expect_err("budget"))`, and `unused_assignments` on `let mut first_eof = None`. Both were fixed in the test before probe-3. Probe-2 files were not rewritten.

## Flags and contracts

`model.rs` and `types/mod.rs` are outside this commit. Probe-3 printed `scan`, `bounded_read`, and `bounded_reader` false. `bounded-stream-v1.md` blob `c0f01b52c4a408a7838b607e29322f318f16372e` is the fixed-input blob. It still calls `finish` the run finish. `reader-lifecycle-v1.md` now says `SnapshotFinished` is non-admission and that only `RunComplete` may admit an eventual loader. No production target admission exists in this slice.
