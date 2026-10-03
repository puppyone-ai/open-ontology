# Findings — a121 missing-proof supplement

Verdict: **REQUEST_CHANGES**.

Reviewed commit `2b6dd57a0dc3d36e098268638c0364af247d07fb`, tree `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc`, parent `e5a066622e0409762fe44f11c4d303d2468a3290`. Proof run is attempt 6, container `42792620d01212af2be53b246d160293e939cc57b1b0cfaebef00c3dfcfb772b`, oracle exit 101, `fail_count=1`, `fail_names=add_column_active_current_schema`. Full tokens, binds, and hashes are in `REPORT.md` and `live/phases-6/oracle-outcome.txt`.

a116's eight marks stay failed in a116 evidence and were not rerun: `same_item_tokens`, `same_line_21_tokens`, `same_side_absent_23`, `deadline_probe_finish`, `deadline_finish`, `inflight_column_probe`, `add_column_agreement`, `add_column_finish`.

## F1 — current schema during an open snapshot — request changes

`ALTER TABLE drift.item ADD COLUMN extra text` committed while generation 41 was open. Admin catalog then showed `extra` at attnum 6. The owner admitted only `id` and `label`. The production probe returned `int:5 text:pebble` and `finish` returned `SnapshotFinished generation=41 pid=527`.

`snapshot.rs` `recheck` (1026–1049) calls `read_attributes` (1244–1266) on the same repeatable-read session. That function runs `sql.rs` `SNAPSHOT_ATTRIBUTES` (334–341). `attributes_match` (882–897) compares attnum, name, type oid, typmod, and not-null with the shape captured in that transaction. The new column is invisible there, so the owner is not poisoned. `finish` (457–471) returns `SnapshotFinished` when rollback and close succeed and the owner is not poisoned.

Prepared review card `execution/prompts/v01-pg-snapshot-owner-review-prepared.md` line 11 requires current schema changes to stop. Contract line 25 defines `schema_drift` as drift from the catalog captured in the same transaction, which is the behavior measured here. Contract line 47 says a shape mismatch is `schema_drift`. The recheck never saw the mismatch. REQ-PL-07 at `specs/open-ontology-v0.1-spec.md` line 403 is pipeline unmapped-column handling and is outside this R1 slice.

A later open with the old two-column shape returned `Catalog` / `catalog_identity`. A later open with the observed three-column shape, attnum 6, returned `int:5 text:pebble null`, matching an independent autocommit oracle, and finished as generation 43. Old-snapshot visibility and fresh-generation visibility were recorded separately. The requirement is not redefined around the old snapshot.

Next minimal implementation: a non-snapshot attribute read in `recheck` / `read_attributes` and `SNAPSHOT_ATTRIBUTES`. Mismatch returns `schema_drift`, poisons the owner, and does not return `SnapshotFinished`. Keep the observed attnum 6.

Attempts 3, 4, and 5 already finished the old owner after the same class of alter. Those runs failed additional harness marks. The product result is the same.

## F2 — same-cut rows — measured on attempt 6

Side repeatable-read session selected `lot.batch` at log line 2419, `00:24:15.475`, before the gate update at line 2430. Owner `START TRANSACTION ... REPEATABLE READ, READ ONLY` is log line 2345, `00:24:15.340`, before writer `COMMIT` at line 2437. Writer then updated `lot.batch` and `lot."child row"` and inserted child 44. Canary went from 0 to 3. Owner, side, and expected old sets were emitted and were equal, including absent keys 44 and 90. Fresh generation 12 matched the new-cut oracle: `aspen` / `held`, qty 14, child 44 `newleaf`. Sets, not ordered lists. `cut_three_sets=pass`, `fresh_three_sets=pass`.

Attempt 5 is not this proof. Its writer stopped on `42501` and the canary stayed 0.

## F3 — column privileges — measured lattice, a116 read explained

Column-only `SELECT` on `acl.sheet` (`inherit=false`, no table SELECT, `attacl=id:SELECT,tag:SELECT,note:SELECT`) makes `SnapshotOwner::open` return `Privilege` / `select_not_granted`. No user-table probe runs. `check_relation` requires table SELECT.

After an owner is open on table SELECT, `REVOKE SELECT ON TABLE` clears column grants. The production read returns `Privilege` / `insufficient_privilege` / `42501` (`error.rs` line 193). Finish is that error. Backend gone in 0 ms. Server text: `permission denied for table sheet`.

`REVOKE SELECT (note)` while table SELECT remains leaves `has_column_privilege(note)` true. The wide probe returns `int:8 text:wool text:bolt` and finishes generation 23. That is why a116's column read succeeded. It is not a breach. Contract line 47 takes `has_*_privilege` as the server answer.

## F4 — deadline and idle — measured as separate facts

Deadline bind pid 524, `lot_reader`, `lotdb`, `oo-pg-source`. Probe and finish are `Budget` / `deadline` with no SQLSTATE. Backend gone in 1 ms. The server idle guard was the longer budget.

Idle bind pid 525. Backend was absent before finish. Wait 8172 ms. Finish is `Protocol` / `database` / `25P03`. Backend gone in 0 ms. Log: `terminating connection due to idle-in-transaction timeout`. Contract line 53 allows that cleanup error. `SESSION_GUARDS` uses one parameter for statement timeout and idle-in-transaction timeout, so the cases differ by which budget is shorter than the client deadline.

Both releases are within 5000 ms. Neither is R3 cancellation.

## F5 — identity_drift and nested_transaction — not runtime-tested

Role rename while generation 51 was open left the probe at `int:1 text:still` and finish at `SnapshotFinished`. Session `current_user` did not change. The role was restored. `recheck` lines 1028–1032 only compare session identity fields. There is no public connection handle to issue a second identity.

`session.rs` `begin` lines 277–280 is `pub(crate)` and returns `nested_transaction` before SQL when a transaction is already open. The oracle cannot call it. Recording those source lines does not certify the runtime fact.

## F6 — SQL audit and canary — measured

Last log window matches the outcome histogram. `oo-pg-source`: 346 `SELECT`, 11 `START`, 10 `ROLLBACK`, zero DML, DDL, and `COMMIT`. Writer: 2 `UPDATE`, 1 `INSERT`, 1 `COMMIT`. Owner canary delta 0. Writer canary delta 3. Secret-file hits in the saved log: 0.

## F7 — smoke and Clippy — measured, failures retained

Linux arm64, Rust 1.94.0, lib 49 passed, smoke exit 0, smoke Clippy exit 0. Review attempts 1 and 2 failed in the probe (compile, then `collapsible_if`) and did not open an owner. Attempts 3–6 compiled and Clippy was 0, locked, offline, `-D warnings`, package `oo-connectors`, all targets. Attempt 6 oracle exit 101 is the assertion panic for F1.

## F8 — scope left unsigned

R2 streaming, R3 cancellation, R4 combined apply, whole SP03, type approval, release, and a110 `cargo deny` were not rerun and are not approved. a116 lifecycle cases (quiet finish, abort, TLS, generation refusal) were not retested. `Drop` was not observed in this supplement.
