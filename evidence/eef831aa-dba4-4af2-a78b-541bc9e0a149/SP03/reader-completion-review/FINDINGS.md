# Findings — a149 reader completion review

Verdict: **APPROVE_LIMITED** on the full-mode reader production API at `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`.

The a146 completeness gap is closed in the executable API. `finish_complete` admits a run only after a covering scan delivered `StreamItem::Exhausted` and the release checks passed. `finish` still returns `SnapshotFinished` and that value does not admit. This review does not check SP03-W03, does not flip capability flags, and does not sign full types, CDC, other databases, incremental mode, or release.

## The a146 gap, on this commit

a146 probe-4 called `SnapshotOwner::finish` on `f5f0a73b` and received `SnapshotFinished` for all four early shapes, with `exhausted_seen=0`. The same token followed a real full drain. That was REQUEST_CHANGES.

`16ca2969` keeps `finish` as lifecycle release. The new method is `finish_complete`. Independent calls in `linux/probe/review-outcome.txt`:

```
early_idle generation=5 rows_read=0 exhausted_seen=0 selected=r149e.rows finish_complete=not_exhausted backends_gone=2 run_complete=0
early_before_poll generation=6 rows_read=0 exhausted_seen=0 selected=r149e.rows finish_complete=not_exhausted backends_gone=2 run_complete=0
early_after_one_batch generation=7 rows_read=2 rows_in_table=4 exhausted_seen=0 ids=1,2 finish_complete=not_exhausted backends_gone=2 run_complete=0
table_eof_subset generation=8 first_eof=rows unread=extra rows_of_first=4 exhausted_seen=0 table_eof_is_not_admission=1 finish_complete=not_exhausted backends_gone=2 run_complete=0
```

Each absence sample is `still=0` and `absent_ms=0`. The clock starts after `finish_complete` returns. The admin sample uses `pg_stat_clear_snapshot` and matches pid plus `backend_start`.

`lifecycle_idle` generation 3 and `lifecycle_after_exhausted` generation 4 both returned `SnapshotFinished` with `admission=0` and `run_complete=0`. Generation 4 had already taken `StreamItem::Exhausted`. Lifecycle close and admission are different values.

## What minted

Generation 1 selected `r149.item` and `r149.line`, opened before the writer commit, and drained 5 Arrow batches, 3404 bytes, fields `id,name` and `line_id,item_id,qty`. Item rows 2 (`1:willow`, `2:oak`), line rows 3, `exhausted=1`, `same_cut_match=1` against the pre-writer oracle compared by key. The outside writer had committed item 1 `pine` and line 13.

`finish_complete` returned `RunComplete` generation 1, `source_id=r4_review_src`, tables `r149.item,r149.line`, schema hash `b44ae80c153942684999963bca7017de7d009524917200bbb2dba836e27e5dac`, digest `f34ebaca9313b6951ad5ecefe19137a0044a07e5076841d154bd19d2bf7955fd`. Both backends were absent at the first sample.

Generation 2 read `pine`, 4 lines, line 13 present, physical scan `2:oak,1:pine`, `cut_compared_by_key=1`, `old_generation_reused=0`, and its own `RunComplete`. The test asserted `fresh_token.input_digest != generation 1 digest`. Schema hash stayed the same because the catalog identity of those two tables did not change.

## Exact coverage

A caller cannot mint by stopping at `TableEof`, by draining a key range, or by draining one chunk.

Generation 9, key range upper exclusive 2: the item stream emitted `Exhausted`, `covers_admission=0`, one item row, zero line rows, `finish_complete=not_exhausted`.

Generation 10, chunk ordinal 0, keyed: `Exhausted` emitted, `covers_admission=0`, one row, `finish_complete=not_exhausted`.

`overlay/run_complete_private_fail.rs` cannot build `RunComplete` and cannot name `oo_connectors::stream::StreamPlan`. Cargo exited 101 with E0639 (`non_exhaustive`), E0308 (seal expected `CompleteSeal`, found `()`), E0599 (no `RunComplete::new`), E0603 (`stream` is private), and E0423. The compiler's struct-literal hint lists `covers_selected` and `exhausted_delivered`. Those fields exist on the private plan. The integration test cannot set them. There is no public setter and no caller assertion that mints.

## Refusals that did not mint

| Generation | Call | Result |
| --- | --- | --- |
| 11 | cancel after `Exhausted` | `Protocol` fact `cancelled`, `run_complete=0` |
| 12 | portal value past `max_value` 4 on a 10-character text | next fact `max_value`; `finish_complete` `Budget` `max_value`, `run_complete=0` |
| 13 | deadline already passed | `Budget` `deadline`, absent at 3 ms, `run_complete=0` |
| 14 | `REVOKE SELECT` committed after `Exhausted` | `Privilege` `select_not_granted`, `run_complete=0` |
| 15 | `ALTER` committed after `Exhausted` | `Catalog` `schema_drift`, `run_complete=0` |
| 16 | admin `pg_terminate_backend` on both reader backends after `Exhausted` | `Protocol` fact `database`, `run_complete=0` |

Generation 16 is a killed-backend release failure. It did not return the named fact `cleanup_unconfirmed`. It also did not return `RunComplete`. The timeout arm that the lifecycle text calls `cleanup_unconfirmed` was not provoked. Reporting the observed fact as `database` is the whole cleanup claim.

Reader SQL in the window: 724 statements, `dml_ddl_commit_terminate_offset=0`, verbs `RELEASE=13, ROLLBACK=42, SAVEPOINT=14, SELECT=623, START=32`. The reader role did not issue `pg_terminate_backend`. The terminate was the admin path.

## Flags and wording that stay

`connector_capabilities` `scan` and `bounded_read` are `supported=false`, reason `not_in_this_slice`. `bounded_reader` is `supported=false`, reason `not this slice`. The lifecycle contract on this commit still says those flags stay unchanged. Turning them on is a follow-up activation after this acceptance. The false advertisement is not a functional failure of the API this review executed.

`bounded-stream-v1.md` line 9 still says `SnapshotOwner::finish` is the run finish, and that `SnapshotFinished` is only `finish` after a clean close. `key_plan.rs` lines 67–68 still say the loader treats anything other than `SnapshotFinished` as incomplete. `reader-lifecycle-v1.md` says the opposite about admission: only `RunComplete` may admit an eventual loader, and `SnapshotFinished` cannot. The live `finish` after `Exhausted` followed the lifecycle contract and returned `SnapshotFinished` with `admission=0`. The two older sentences are residuals. They do not by themselves reopen the admission gap.

## Harness failure preserved

`linux/probe-1/` is the first probe. `docker-run.exit` 101 and `clippy.exit` 101. Container `f7929b4827990c46179798d3127f30c4491e5ffbdad9d86fb622ecd34d3fc941`, privileged false, `CapDrop=ALL`, private network, elapsed 1s. The failure is two overlay `format!` calls that passed `generation` both implicitly and positionally. Production files were not changed. The corrected overlay is the one `linux/probe` compiled. Probe-1 was not overwritten.

`linux/probe/private-construct.cargo.exit` 101 is the required compile failure, not a product failure. `private-construct.exit` is 0.

## Reused evidence

Session, key plan, model flags, type flags, SQL privacy, connector manifest, root lock, bounded-stream bytes, and reader-keys bytes match the a146 blobs. The a146 live matrix for snapshot binding, budgets, keys, cancel, drop, and the reachable `max_value` finish path stands on those bytes. This session's smoke is the Linux 1.94 regression of the new archive: 63 library tests passed, scoped clippy passed. The fresh proof is the admission composition above.
