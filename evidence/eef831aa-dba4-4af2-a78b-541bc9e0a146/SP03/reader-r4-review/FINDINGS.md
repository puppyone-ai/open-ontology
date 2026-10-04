# Findings — a146 R4

Verdict: **REQUEST_CHANGES** on the full-mode reader at `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`.

## The admission gap

`SnapshotFinished` (`snapshot.rs` 209–220) means rollback and close succeeded. `finish` (`snapshot.rs` 566–609) builds that value, takes the pull, and returns it when the pull error is absent, the cancel flag is clear, the catalog recheck passes, both releases succeed, the owner is unpoisoned, and the deadline has not passed. Budget pull errors return at lines 599–601 before `Ok(finished)`.

`scan` returns `Ok(())` on `Signal::Stop` at `stream.rs` 528, 559, 622–624, 639–640, 648–649, and 661. `Hold::Stop` (`stream.rs` 685–687 and 719–721) closes the portal and returns. `TableEof` is documented as neither run exhaustion nor `SnapshotFinished` (`stream.rs` 121–122). `StreamItem::Exhausted` (`stream.rs` 139–140) is the point where every selected table has reached `TableEof`. `finish` does not read that item.

Live production calls in `linux/probe-4/r4-outcome.txt`:

```
early_idle generation=3 rows_read=0 exhausted_seen=0 selected=r4e.rows finish=SnapshotFinished generation=3
early_before_poll generation=4 rows_read=0 exhausted_seen=0 selected=r4e.rows finish=SnapshotFinished generation=4
early_after_one_batch generation=5 rows_read=2 rows_in_table=4 exhausted_seen=0 ids=1,2 finish=SnapshotFinished generation=5
missing_selected_table generation=6 first_eof=rows unread=extra rows_of_first=4 exhausted_seen=0 finish=SnapshotFinished generation=6
```

Generation 1 did see `exhausted=1` and also returned `SnapshotFinished`. The token fields recorded there are `generation,source_id,backend`. `rows_in_token=absent`. The contract line 13 says a drained portal EOF can return `SnapshotFinished`. The contract line 42 calls `SnapshotFinished` the only successful close. A caller that treats the only successful close as a full round will load the generation 6 output, which never scanned `r4e.extra`.

The same probe logged `ROLLBACK=19` on `app=oo-pg-source` for the reader role, and zero `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `MERGE`, `COPY`, `CREATE`, `ALTER`, `DROP`, `GRANT`, `REVOKE`, `COMMIT`, `OFFSET`, and `pg_terminate_backend`. The early closes rolled back. That rollback does not prove the selected tables were read.

## Missing API

Add one admission value that outside code cannot construct. Suggested name `RunComplete`. Construct it only when:

1. The pull delivered `StreamItem::Exhausted` for this owner, so every selected table reached `TableEof`.
2. The cancel flag is clear.
3. The catalog recheck passed.
4. Both releases succeeded and the owner is unpoisoned.
5. The deadline has not passed.

Until that value exists, `SnapshotFinished` stays a close marker. Suggested failure for the four cases above: `SourceError::protocol("not_exhausted")`. Tests belong on the production owner, with the same four shapes as `overlay/pg_reader_r4_probe.rs` phases `phase_finish_idle`, `phase_finish_before_poll`, `phase_finish_after_one_batch`, and `phase_missing_selected_table`. The drained two-table cut is the case that may return the admission value.

Update `contracts/source/pgsql/reader-lifecycle-v1.md` line 13 and line 42 so a drained EOF is the admitting case, and a mid-stream stop remains a close without admission. Leave `connector_capabilities` and `bounded_reader` false in that change. Turning `bounded_read` on is a later contract edit, after the admission value is the only success a loader can treat as a complete round. `type_conversion` stays false. Catalog rows in probe-4 are `KnownBuiltinNotConverted`.

## What a144 already closed, and what it left open

`settle_after` (`stream.rs` 1067–1092) turns `portal_poison` into `Ok` and `mark_skip_rollback`. `prefer` (`stream.rs` 1108–1116) keeps a `Budget` error and `cancelled` / `cleanup_unconfirmed`. The live `max_value` path is `Hold::Fail`, then `prefer`, then `finish` at lines 599–601. Author `linux/probe/finish-outcome.txt`: `bound_finish_fact=max_value`, `snapshot_finished=0`, `reader_rollback_lines=0`, `reader_reset_lines=1`, data pid 201. Probe-4: `portal_bound` `next_fact=max_value`, `finish=Err kind=Budget fact=max_value`, `snapshot_finished=0`, data pid 281. A real bound-portal error does not return `SnapshotFinished`.

`Hold::Stop` with `closed = Err(portal_poison)` would be scan success plus `skip_rollback` if `close_owned` saw an already poisoned portal. That arm was not provoked. The Stop arm that was provoked is a clean close, and it returns `SnapshotFinished` with rows unread. That is a different branch from the a143 residual, and it is reachable.

`cleanup_unconfirmed` for a drain past 5000 ms, a failed guard cancel, and the no-runtime `Drop` arm were read in `stream.rs` `await_cancellable` and `signal_cancel`. They were not provoked. The shortened deadline and the owner drop were run: deadline `elapsed_ms=19784` generation 6; drop absent at 53 ms, `snapshot_finished=0`.

## Longitudinal cut

Admin oracle before the writer: `r4.item` 2 rows, item 1 `willow`, `r4.line` 3 rows. Writer then committed item 1 `pine` and line 13. Generation 1, opened before that commit, drained `item_rows=2 line_rows=3 exhausted=1 batches=5` and matched the pre-writer cut. Accounting `executes=7 consumed_rows=5 decoded_payload=53 wire_view_peak=8192 wire_accepted=10188`. Generation 2 matched the post-writer cut by key, including line 13, and did not reuse generation 1. Physical scan order after the update was `2:oak,1:pine`.

## Composition that passed on this blob

Recorded under `linux/probe/` and `linux/probe-2/`, exits 0 unless noted in `REPORT.md`:

- Stream live: oracle payload 59, Arrow preimage 132, wire view peak 8192, parent and child batches 2, zero-column batch 1, reader `dml_ddl_commit=0`.
- Bounds: `max_value` and `max_row` refused, slow consumer `ClientRead`, one extra execute after demand.
- Keys: ICU collation `af-NA-x-icu` differs from C, sparse chunk matches the cut, late keys stay out of the old cut, no-key generation 3, `OFFSET` count 0.
- Cancel: deadline, `57P01` after the owned connection was killed, old run did not finish, fresh generation unmixed.
- Drop: backends absent at 53 ms, `Drop` did not return `SnapshotFinished`.
- Finish live: drained ids `1,2,3` return `SnapshotFinished`; cancel fact `cancelled`; bound fact `max_value`.
- ACL on the truncated log: privilege loss is `select_not_granted` on the guard and on `finish`, no second scan.

`sql` stays a private module (`lib.rs` line 25). Reader audits show no source writes.
