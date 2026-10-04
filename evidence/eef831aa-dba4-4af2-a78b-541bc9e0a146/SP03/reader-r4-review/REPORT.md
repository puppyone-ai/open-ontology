# Report — a146 full-mode reader R4 review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a146`. Slot B. No subagents and no worktree. Slot A `a145` was not running in the capacity samples taken before probe-3 and probe-4. Exited `oo-a064-*` containers were left in place.

Verdict: **REQUEST_CHANGES**. The full-mode reader is not approved. R4 approval would be the node that signs that sub-item for target apply. This review does not sign it. SP03-W03 stays open. Other databases, incremental mode, CDC, the registry API, release, converted types, full type support, and supply or license approval stay unsigned. The ledger was not edited.

Fixed source `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`, tree `6ec4bf24dd0111089b87b9b805e2ee71837cd34a`, parent `63161574edf932ea00e25b8a17f69b095c70051f`. Author evidence `dc5196b2`. Workspace HEAD during probe-4 was `f1f75f37036f53ff8f638db1aa154b5b4bcf5d9e`. Every compile and test used `git archive` of `f5f0a73b`. Product blobs at that commit, matched on smoke and on every probe (`archive_hashes_match=1`):

- `crates/oo-connectors/src/snapshot.rs` git blob `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`, content `7961fe972051d7ee4163eb3b38bfb0353a8a2a82b3dedb669971c323b7ffb4e8`
- `crates/oo-connectors/src/session.rs` git blob `7e191eacbf61439680cbd92a7109ded6c8a05596`, content `6933bf4e855682e6998b89c8c756f8db6239e28e4bc9a249fd34af98c088f16f`
- `crates/oo-connectors/src/stream.rs` git blob `ba9b215a059440a4681578200e7d719cf22407e2`, content `fe11aca7f711c97787c7420ce98e64a1fcd9c32fec6e07eb3bd65736f1013847`

`key_plan.rs` git blob `8f152763844330532df58efdc77568e1f36e3d40`. Finish-test git blob `06491036db04b58855b9e96c008d7e5e90bcbf74`. Lifecycle git blob `4409c11c99f4b3e5f478b94a1176aa75618ec439`, content `b368672db128280d957ee07a3cdee24118306817e389dd42ce5f4d00b2056391`. Those three product blobs are the a143 blobs. a144 added the finish test and the Caller paragraph. It did not change the three product files.

## Why the verdict is REQUEST_CHANGES

`SnapshotFinished` is the lifecycle close. It is returned after rollback and close when the pull task returns success. It does not require `StreamItem::Exhausted`. The struct carries `generation`, `source_id`, and `backend` (`snapshot.rs` 213–220). It has no row count and no exhausted bit.

`linux/probe-4/r4-outcome.txt` called production `SnapshotOwner::finish` on `f5f0a73b`:

- No stream opened, 0 rows, selected `r4e.rows`: `SnapshotFinished` generation 3.
- Stream opened, no `next_batch`, 0 rows: `SnapshotFinished` generation 4.
- One batch, ids `1,2`, table holds 4 rows, `exhausted_seen=0`: `SnapshotFinished` generation 5.
- Selected `r4e.rows` and `r4e.extra`. First `TableEof` was `rows` (4 rows). `extra` was never scanned. `SnapshotFinished` generation 6.

The same token is returned after a real full drain (generation 1, `exhausted=1`, batches 5, same cut as the pre-writer oracle). A loader that admits `SnapshotFinished` therefore admits an unread table. Reader SQL on that run included `ROLLBACK=19`. A completed rollback is release of the snapshot. It is not proof that every selected row was read.

The missing production API is a separate admission value. Suggested name `RunComplete`. It is constructible only after every selected table has reached `TableEof` (`StreamItem::Exhausted` at `stream.rs` 139–140) and the existing release checks have passed. `SnapshotFinished` can remain the close marker. `finish` can instead return `protocol` fact `not_exhausted` unless Exhausted was delivered, and return the admission value only then. A capability-flag flip is not that proof. `connector_capabilities` still reports `scan` and `bounded_read` as `supported=false` reason `not_in_this_slice` (`model.rs` 733–749). `type_capabilities` reports `bounded_reader` false, reason `not this slice`. The lifecycle contract says those flags stay unchanged. They stay false until an admission token exists. This review does not ask for them to be turned on in the same change as a comment.

a144 proved a drained `r3f.rows` of ids `1,2,3` can return `SnapshotFinished`, and that a real `max_value` portal fault does not. This session reran that test (`linux/probe/finish-outcome.txt`, `snapshot_finished=0`, `reader_rollback_lines=0`, `reader_reset_lines=1`). Probe-4 repeated the portal fault: `finish=Err kind=Budget fact=max_value`, `snapshot_finished=0`. That author proof does not accept a general complete-EOF contract. The missing requirements are the four early-finish cases above, and an admission value that is withheld when any selected table has not reached `TableEof`.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `HOME=/tmp`. Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/init.stdout` recorded `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a146-pg` id `a37bb920e13293d5a20b947fe2cd0b56a8f5dd6c2063a362d724623671b78049`, privileged false, network `oo-a146-eef831aa-net`, memory 536870912. Private volumes `oo-a146-eef831aa-cargo`, `oo-a146-eef831aa-target`, `oo-a146-eef831aa-pgdata`, `oo-a146-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. Passwords and the TLS material stayed under `/tmp/oo-a146-eef831aa-dba4-4af2-a78b-541bc9e0a146/secrets`. A scan of the 163 files under this evidence directory found none of those three password strings.

Smoke and probes: `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, private network. Smoke hostconfig `ExitCode=0`, `OOMKilled=false`, `Privileged=false`. Probe-4 hostconfig `ExitCode=0` and the same containment. Probe-1, probe-2, and probe-3 hostconfigs are `ExitCode=101` with the same containment. Those 101s are recorded below. They are harness failures, preserved.

One own Rust builder at a time. Capacity files for probe-3 and probe-4 record `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`.

## What ran

Smoke finished before any probe. `cargo test --locked --offline -p oo-connectors --lib` → `63 passed; 0 failed` (`linux/smoke/lib.exit` 0). `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` exit 0. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail the package lint. Elapsed 38s. `linux/smoke/docker-run.exit` 0.

Probe-1, one PostgreSQL, `--test-threads=1`, archive plus the first overlay. Exits: stream live 0, bounds 0, keys 0, cancel 0, drop 0, finish 0. ACL 101. R4 overlay compile 101. `docker-run.exit` 101.

Probe-2 reran ACL on a truncated log, then the overlay. ACL exit 0. R4 exit 101 at the fixture writer `UPDATE`. `docker-run.exit` 101.

Probe-3 reran only the overlay after `GRANT SELECT, INSERT, UPDATE`. Exit 101. The fresh generation had already read `(2, oak), (1, pine)`. The oracle `ORDER BY id` expected `(1, pine), (2, oak)`.

Probe-4 compared the cut by key and ran the early-finish phases. `pg_reader_r4_probe.exit` 0. `test result: ok. 1 passed` in 2.16s. `docker-run.exit` 0. Elapsed 4s. `archive_hashes_match=1`.

No production, manifest, lock, spec, or ledger file was edited.

## Requirement, fixed source, evidence

Original card: `execution/prompts/v01-pg-bounded-reader-prepared.md`. Contract: `contracts/source/pgsql/reader-lifecycle-v1.md` at `f5f0a73b`. Plan item SP03-W03 remains unchecked.

| Claim | Fixed source | Evidence | Result |
| --- | --- | --- | --- |
| Production owner, one RR read-only snapshot, catalog bound in that transaction | `SnapshotOwner::open`, binding catalog on the owner | `linux/probe/stream-live-outcome.txt` oracle before writer `parent=4 child=3 empty=0 zero=2`; probe-4 catalog read from `binding().catalog` | Re-proved on these blobs. a137 receipts are an older snapshot blob and were not reused as current proof. |
| One connection, nonreusable generation. Exported PG snapshots are not restart checkpoints | `snapshot.rs` `finish` consumes the owner | Probe-4 generations 1 through 7. Cancel `killed_next_fact` `57P01`, `old_run_not_finished`, `old_partial=[1]`, `fresh=[1, 2, 3, 4]`, `fresh_generation=9` | Re-proved. |
| No reader `COMMIT`, no reader DML or DDL, no arbitrary SQL export | `lib.rs` `mod sql` is private. Public surface is the owner API | Stream, bounds, keys, cancel, drop, finish, and probe-4 audits. Probe-4 verbs `RELEASE=6,ROLLBACK=19,SAVEPOINT=7,SELECT=284,START=14`. ACL `reader_writes=0` | Re-proved. Fixture writes stay on admin and writer sessions. |
| Full table with no stable key is one bounded stream and does not invent a resume cursor | `stream.rs` scan of selected tables. `TableEof.resumable_key` | Keys `nokey_generation=3`. Probe-4 `r4.line` has no primary key and was read in the same owner as `r4.item` | Re-proved by the keys live test on this blob, plus the no-key line table in probe-4. |
| Typed keys use PostgreSQL collation, half-open ranges, bounded metadata, no `OFFSET` | `key_plan.rs` blob `8f152763844330532df58efdc77568e1f36e3d40` | `keys-outcome.txt`: collation `af-NA-x-icu`, `pg_order=a,A,ä,b,B,z`, `c_order=A,B,a,b,z,ä`, `sparse_chunk0_matches_cut`, `dml_ddl_commit_offset=0` | Re-proved. a139 APPROVE stands. This run does not widen it. |
| Batch rows and bytes, max value and row, backpressure, wire cap before a batch check, no whole-table `collect` | `StreamBudget`, portal bounds in `stream.rs` | Stream live: `held_state=active wait_event=ClientRead`, `parent_batches=2`, `wire_view_capacity_peak=8192`, `wire_accepted_logical_payload=12531`, payload 59 matches the oracle. Bounds: `max_value`, `max_row`, `log_delta_after_demand=1`. Probe-4 `batch_rows=1` produced 5 batches for 5 rows; `batch_rows=2` returned exactly ids `1,2` | Re-proved on the current stream blob. |
| Over-limit value is a typed stop. The successful batch is not incomplete | `prefer` keeps `Budget` (`stream.rs` 1108–1110). `finish` returns that error at `snapshot.rs` 599–601 | Finish live `bound_finish_fact=max_value snapshot_finished=0`. Probe-4 `portal_bound` the same fact | Re-proved. |
| Cancel, drop, deadline, and killed backend release the owner. Stats use a current `pg_stat_activity` sample | `CLEANUP_OBSERVATION_BOUND_MS` 5000. Contract requires `pg_stat_clear_snapshot` | Drop absent at 53 ms, `snapshot_finished=0`, pids 191 and 192. Cancel deadline `elapsed_ms=19784`. Finish of a drained table absent at 1 ms | Re-proved once on this session. The 5000 ms drain, guard-cancel failure, and no-runtime `Drop` arm were read and not provoked. |
| 300s deadline does not silently extend or open a fresh per-table snapshot under the same generation | `SNAPSHOT_DEADLINE_DEFAULT` is 300s (`key_plan.rs` 26). `finish` returns `budget` `deadline` when `Instant::now()` has passed (`snapshot.rs` 592–593, 607) | Shortened deadline returned `deadline` and did not finish generation 6. A new generation read the new cut | The constant and the shortened path were proved. A wall-clock 300s run was not started. |
| Source loss invalidates the unpublished run. A new generation rereads every selected table with no mixed rows | `finish` does not reconnect. A new `open` is a new generation | Cancel old partial `[1]` versus fresh `[1, 2, 3, 4]`. Probe-4 generation 1 stayed on `willow` and 3 lines after the writer committed `pine` and line 13. Generation 2 read `pine` and 4 lines, `fresh_has_13=1`, `old_generation_reused=0`. Scan order `2:oak,1:pine` differed from `ORDER BY id`. The cut was compared by key | Re-proved. |
| Rejected and partial output stay distinguishable from a successful full EOF | `snapshot.rs` 566–609 returns `SnapshotFinished` whenever the pull returns `Ok` and release checks pass. `stream.rs` 528, 559, 622–624, and 661 return `Ok` on `Signal::Stop` | Probe-4 generations 3, 4, 5, and 6 | Failed. This is the change request. |
| Capability list matches a usable full-mode admission | `model.rs` 733–749. `types/mod.rs` line 110. Lifecycle paragraph 1 | Probe-4 lines 2–11, all `supported=false` | The list matches the current contract, which still says this slice does not implement the reader. It does not match a signed full-mode reader. Leave the flags false until `RunComplete` exists. |
| Source types stay unqualified | Catalog `type_support.converted` | Probe-4: `item.id`, `item.name`, `line.line_id`, `line.item_id`, `line.qty` all `converted=false` reason `KnownBuiltinNotConverted` | Recorded. Converted types, full types, and supply approval are not signed. |
| a143 `skip_rollback` plus scan success does not become `SnapshotFinished` on a real portal fault | `settle_after` 1067–1092 can swallow `portal_poison` into `Ok` and `mark_skip_rollback`. `prefer` keeps the budget error. `Hold::Fail` is the `max_value` arm. `Hold::Stop` is the caller-stop arm and closes the portal | Live `max_value` finish is the budget error, twice (author finish test and probe-4) | The reachable budget fault does not finish. A poisoned close on the Stop arm was not provoked. The Stop arm that did run is the clean early finish, and that one does return `SnapshotFinished`. |

## Harness failures preserved

`linux/probe/pg_stream_acl_live.exit` 101. `pg_stream_acl_live.rs:210` counted `SAVEPOINT` lines in the whole PostgreSQL log and saw 10 where it expected 1, after stream and bounds had already logged `SAVEPOINT oo_stream_lock`. The functional asserts had passed. Probe-2, on a truncated log, wrote `stream-acl-outcome.txt`: `acl_a_id=7`, `guard=select_not_granted`, `finish=select_not_granted`, `reader_writes=0`, `second_scan=0`, `backends_gone=2`.

`linux/probe/pg_reader_r4_probe.exit` 101. The first overlay did not compile (`world` not mut, use of `finished` after move, missing `Connection`, `SelectedTable::new` received `&&str`).

`linux/probe-2/pg_reader_r4_probe.exit` 101. The fixture writer `UPDATE r4.item SET name = 'pine' WHERE id = 1` returned SQLSTATE 42501. The grant was `INSERT, UPDATE` (`aw`) without `SELECT`. PostgreSQL requires `SELECT` to evaluate `WHERE`. The overlay grant became `SELECT, INSERT, UPDATE`. That is fixture SQL, not a product change.

`linux/probe-3/pg_reader_r4_probe.exit` 101. Order-sensitive `assert_eq` on the fresh cut. The panic text is the oracle: left `(2, oak), (1, pine)`, right `(1, pine), (2, oak)`. Probe-4 records `fresh_item_scan=2:oak,1:pine` and `cut_compared_by_key=1`.

`complete_release_wait_ms=1` in probe-4 is the wall time of `finish` on the drained owner. That probe does not poll `pg_stat_activity`. Absence for a drained finish is `normal_absent_ms=1` in `linux/probe/finish-outcome.txt`.
