# a137 fixed R2-S Arrow ACL review

Verdict: APPROVE LIMITED for the R2-S slice on `dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200`.

This does not sign full R1, full R2, the reader, R3, or R4. The bounded TLS integration stays the a135 limited acceptance of `bd77b2520bcc3d63517bc49309002f25b58544b1`. The `types/mod.rs` re-export admitted for `75b993d0` stays a process violation on that landing. It is unchanged here and is not a new functional defect.

## Composition

Tree `dd20253a1e7bab45bb5a7d34677ad0b3154644bc`. Product differences from `75b993d0` are the ACL hunks in `snapshot.rs`, `sql.rs`, `tests/pg_stream_acl_live.rs`, and `contracts/source/pgsql/bounded-stream-v1.md`, plus the already-accepted driver files `vendor/sqlx-core/src/net/socket/buffered.rs`, `tests/pg_wire_tls_tail.rs`, and `vendor/PATCH.md`, plus the ledger.

Blobs on this tree:

| file | blob | same as |
| --- | --- | --- |
| `stream.rs` | `e7b5a6cc1714c67e818899788a09e0315b93b3ed` | `75b993d0` |
| `types/mod.rs` | `46402e2e9c37064f7a81a3374d7fd5073c0ced58` | `75b993d0` |
| `error.rs` | `15fe429ec540e4ff4be15ead2bda4474c09ff9e6` | `75b993d0` |
| `buffered.rs` | `3ba2b87507096bab77464eaab7797712e4bf02cc` | `bd77b252` |
| `snapshot.rs` | `3991674d164ff4fcf5150dded2105a1144028f53` | this commit |
| `sql.rs` | `ede6e5d30796fa8ec79696e47df80211438670bf` | this commit |

`stream.rs` order is still recheck, `guard_still_current`, then `SAVEPOINT oo_stream_lock`, then the portal (`stream.rs` 509-520). `error.rs` line 193 still maps SQLSTATE `42501` to `Privilege` / `insufficient_privilege`.

The executable ACL hunk adds one trailing boolean on `CURRENT_CATALOG` and reads it at result index 15. Indexes 0 through 14 are unchanged. `observation_limit` and `arm_rows` are not in the hunk. The ACL test's guard bind was `$3 = '4'` for two one-column tables, which is the existing ceiling `(columns.max(1) + 1)` per table. The failure fact was `select_not_granted`, so the extra cell did not shift the row decoder into `schema_drift`.

Author evidence `173e6eb0` says the tested snapshot blob was `87c4b403d332cfcb279e975dadc69a81360b3b33` and that the committed difference is the `ObservedAttr.select` doc comment. That blob is not in this object store. This review does not treat the author's hash list as a verified inventory. The committed comments do not change control flow. The executable additions are the `select` field, the index-15 read, and the `any(select != Some(true))` check after the shape and primary-key comparison.

## Environment

Linux arm64, Rust `1.94.0` (`4a4ef493e`, 2026-03-02), host `aarch64-unknown-linux-gnu`. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

Private network `oo-a137-eef831aa-net`, alias `oo-a137-pg`, no published ports. Postgres cap-drop `ALL`, cap-add `CHOWN DAC_OVERRIDE FOWNER SETUID SETGID`, not privileged. TLS SAN `DNS:oo-a137-pg`. Database `oo_a137`. Reader role `oo_a131_reader` because the original tests filter that name. Secrets stayed under `/tmp` and are not in this directory. `pointer-keys.txt` lists keys only. `log-secret-scan.txt` recorded `secret_hits=0`.

Containers: smoke `a588e35c072e`, review `49ec4215d413`, column-grant `f15406bc226a`, postgres `ba1b06486b152a`. Own volumes `oo-a137-eef831aa-cargo`, `oo-a137-eef831aa-target`, `oo-a137-eef831aa-pglog`, `oo-a137-eef831aa-pgdata`. `OOMKilled=false` on each. Memory 8GiB on the builders and 512MiB on Postgres.

## Smoke, before the functional overlay

`linux/smoke` compiled `git archive dd5bf441` with no review overlay.

| phase | exit |
| --- | --- |
| toolchain | 0 |
| snapshot | 0 |
| clippy-component | 0 |
| fetch | 0 |
| lib | 0, 57 passed, 0 failed |
| clippy `-p oo-connectors --all-targets -- -D warnings` | 0 |
| docker-run | 0 |

Elapsed 49s. Vendor `sqlx-postgres` printed two existing `unfulfilled_lint_expectations` warnings. Clippy still exited 0.

## Original tests once

`linux/functional` on the same archive. `pg_stream_acl_live`, `pg_stream_live`, and `pg_stream_bounds` exited 0. The first review binary exited 101; that run is `linux/functional/attempt1` and is the column-only open case in FINDINGS. It is not the revoke result.

ACL outcome:

`acl_a_id=7 acl_a_eof=1 generation=1 data_pid=145 data_start=2026-10-03 17:41:21.107706+00 guard=select_not_granted finish=select_not_granted reader_after=6 reader_writes=0 second_savepoint=0 second_scan=0 backends_gone=2`

Log lines 111-126 in `linux/functional/postgresql.log`: admin `REVOKE SELECT ON acl.acl_b FROM oo_a131_reader`, then the reader repeatable-read node, relation, and `has_column_privilege` attribute probe, then one `CURRENT_CATALOG` statement (`aclexplode`, bind `$3 = 4`), then `ROLLBACK` on both reader sessions. The next lines are admin checks that pids 145 and 146 are gone. There is no `SAVEPOINT oo_stream_lock`, no `SELECT` of `acl.acl_b`, and no `permission denied` in that window.

Live outcome kept the pre-writer oracle: parent 4, child 3, empty 0, zero 2, payload 59. Arrow preimage 132. Assembly 5528. Wire view peak 8192. `admin_sees_late_parent=1`. Reader audit `dml_ddl_commit=0`, writer writes 2. Held state `active` / `ClientRead`. Finish, abort, and drop released pids 149, 152, and 154.

Bounds refused `max_value` and `max_row` with display `source error: budget fact=...`. The slow consumer moved from 1 execute to 2 only after the next demand (`log_delta_after_demand=1`).

Whole-log reader audit: 481 statements for `app=oo-pg-source` and `user=oo_a131_reader`, 0 of those statements contain a write verb (INSERT, UPDATE, DELETE, TRUNCATE, MERGE, COPY, CREATE, ALTER, DROP, GRANT, REVOKE, COMMENT, VACUUM, CLUSTER, REINDEX, CALL, DO, COMMIT).

## Inherited grant

`linux/column-grant` exited 0. `SELECT` on `colg.via_parent` is granted to `colg_parent`, and the reader inherits that role. The table ACL text is `{postgres=arwdDxtm/postgres,colg_parent=r/postgres}`. `attacl` is empty. `oo_a131_reader=` is absent. As the reader, `has_table_privilege` is true, the table `aclexplode` arm is true, and the `CURRENT_CATALOG` case is true.

The stream returned Arrow int32 `id` 3, generation 1, data pid 206, `finish=SnapshotFinished`, both backends released, reader writes 0, two reader statements naming `via_parent` (portal and fetch).

## What a134 required, and what this run saw

The post-open revoke of the next selected table returns `Privilege` / `select_not_granted` from the current-catalog guard, before the second savepoint and before the scan. No revoked row is emitted. Finish carries the same fact. Both reader backends are gone.

`has_column_privilege` is still the repeatable-read attribute probe. On this log it runs after the revoke and does not stop the pull. The following current-catalog statement does.

## Out of scope, left open

No type matrix, security matrix, license scan, dual-arch run, R3 cancel matrix, or large allocation run. a134's batch-byte split on `stream.rs` blob `e7b5a6cc` was not repeated. The bounds run on this tree reconfirmed the demand-gated execute. Do not mark the bounded reader or R3/R4 accepted from this verdict.

Own containers, volumes, network, and `/tmp` paths are removed in `cleanup.txt`. No prune and no other task's containers.
