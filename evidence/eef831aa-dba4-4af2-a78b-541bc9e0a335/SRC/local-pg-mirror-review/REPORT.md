# SRC-D01 local PostgreSQL mirror review

Verdict: **WITHHELD**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a335`. This directory reviews `tools/local-pg-mirror` at fixed commit `84763bc401b94806adcadd9bd9d99740a2d622e3`, tree `76010c0c0e820bbfedac29300f9370e874388dcd`. The author record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a318/SRC/local-pg-mirror`. No product file, root lock, spec, or ledger was edited. This file does not accept the parent goal, does not accept a full source copy, and does not accept a customer copy.

`customer_complete` stays false. No enterprise endpoint was supplied. No enterprise host was guessed and no enterprise row was read.

## Why this is withheld

The original task requires one source connection for the read-only snapshot and its catalog. The development card says "source-readonly transaction plus catalog exactly one connection". The plan and ledger record the same hard budget: one source connection, batch at most 1000, statement timeout at most 30s, lock timeout at most 1s, snapshot at most 300s.

The fixed tree does not meet that connection budget. `SnapshotOwner::open` reserves two permits and opens two sockets: one repeatable-read read-only data session and one read-committed catalog guard. `ImportSession::open` calls that function. After the open, `copy` stores `source_socket_peak` 2 and `source_socket_fact` `verified_reader_guard` as constants. The author's `CONTRACT.md` describes those two sockets and says a request cannot hide the guard. That contract text changes the task budget. This review does not treat the change as compliance and does not edit it.

The fresh oracle measured the pair. During a copy of 20000 rows, `pg_stat_activity` showed at most 2 backends for `oo_rev_src` on `revsrc`, both with `application_name` `oo-pg-source`. It did not show a third. Drift refusal, which returns before `SnapshotOwner::open`, recorded peak 0.

## What was run

Pre-overlay smoke on the fixed archive: rustc 1.94.0, host `aarch64-unknown-linux-gnu`, tool present, probe exit 0. Builder `oo-a335-smoke`: 6GiB, 1 CPU, `CapDrop=ALL`, not privileged.

Pre-overlay author suite, before `tests/independent.rs` existed in the work tree:

| Check | Exit |
| --- | --- |
| version | 0 |
| source archive | 0 |
| generate-lockfile | skipped; committed lock kept |
| lock subset (`lock_subset_ok tool_checksums=251`) | 0 |
| fetch --locked | 0 |
| cargo test --locked --offline | 0 |
| lock stable and root lock stable | 0 |
| clippy --all-targets -D warnings | 0 |

Tests: `closed_budget_edges`, `forbidden_key_stays_off_the_output`, `generation_changes_the_plan_hash`, `missing_secret_is_not_a_connect`, `mysql_is_unsupported_before_credentials`, `over_budget_is_refused_before_credentials`, `tamper_does_not_read_a_secret`, and `public_fixture_copy_is_local` (5.57s). Tool lock SHA-256 `78117125b0ebcdfbf1363d949428655fb1d7dd7be126567fdc2003a605ad874b`.

Independent oracle on a new PostgreSQL data volume, two databases `revsrc` and `revmirror`, then Clippy of the overlaid tree: test exit 0 (11.43s), clippy exit 0, lock unchanged. Builder `oo-a335-independent` id `876f7a35958c7fa815c7739e2fcad582f46354444eeb867b2f322f2567520d3a`: 6GiB, 1 CPU, `CapDrop=ALL`, not privileged, private network, no host port (`ports.json` is `{}`).

Observed on that fresh fixture:

- Source `INSERT` as the read login returned SQLSTATE `42501`.
- Three typed tables copied. Local read after the copy matched the hand values: int8 `9223372036854775807` and `-5`, numeric `10.50` and `0.00`, SQL NULL, timestamp `2024-03-04 05:06:07`, json `{"k":"v"}`, bytea `0001ff`, bool `true`/`false`, and an empty string.
- Logical name `public.amounts` mapped to physical `pt_8dea6fb03a75dfa45503c67cf36965ad2727a40ec2f1a32f1c8b83f3a14c` (63 characters). Column physical names use the `pc_` form. int8 oid 20, numeric oid 1700, text oid 25, timestamp oid 1114, json oid 114, bytea oid 17, bool oid 16. The manifest records `_oo_pos numeric(38,0)` and `physical-mapping/v1`.
- The target file is `oo.pipeline.local-pg-mirror-target/v1`, `verify-full`, reader `oo_rev_read`, database `revmirror`, with no password and no source id.
- An added column on `public.amounts` was `refused` / `catalog_drift` with peak 0. The earlier gold rows stayed.
- Cancel, stop-after-one-batch, and a 1ms deadline were `incomplete`, not `single_snapshot`.
- `jsonb` oid 3802 was `unsupported_column` with a quarantine row and no cell bytes.
- Resume of the incomplete manifest with a missing password path returned `resume_refused` and did not create the path. `read_local` on that manifest returned `refused` / `not_complete` before any credential read. A direct select of the partial physical table returned `42501`.
- Closed ceilings rejected batch 1001, statement timeout 30001, lock timeout 1001, and snapshot deadline 300001 before a credential read. `mysql` returned `unsupported_engine engine` before a credential read and before a plan file existed.
- After `REVOKE CONNECT` on `revsrc` and `pg_terminate_backend` of `oo_rev_src`, a source dial failed and `read_local` still returned the five gold rows.
- Replay of the complete manifest with the same missing password path returned `replay_complete`. Source backends for `oo_rev_src` were 0 before and after that call. The local row count stayed 5. `customer_complete` stayed false.

## Attempts kept

| Attempt | Result |
| --- | --- |
| independent-attempt-1 | Test exit 101. The oracle looked up `CARGO_BIN_EXE_local_pg_mirror` at runtime. Cargo exposes `CARGO_BIN_EXE_local-pg-mirror` to `env!`. No database was created. |
| independent-attempt-2 | Test exit 0 and the same socket observation. Clippy exit 101 because the component was not installed in that container. |
| independent-attempt-3 | Test exit 0 and the same socket observation. Clippy exit 101: useless `format!` and three `too_many_arguments` hits in the oracle. |
| independent | Test exit 0 and clippy exit 0 after those oracle-only edits. |
| inventory-attempt-1 | Docker exit 125. Docker Desktop refused the `/tmp` bind for the sums file: `bind source path does not exist: /host_mnt/private/tmp/oo-a335-eef831aa-dba4-4af2-a78b-541bc9e0a335/inventory-out`. No container remained. |
| inventory-attempt-2 | The same mount refusal, exit 125, after the output directory was recreated on the host. The launch script then stopped using that bind and copies `SHA256SUMS` out of the container. |

`cargo generate-lockfile` was not run. The author's attempt 1, which selected checksums absent from the fixed root lock, remains in the a318 evidence. This review used the committed tool lock and the fixed root lock. Both stayed byte-stable.

## Upstream interface

Do not drop the catalog guard to force one socket. The guard exists so a read-committed session can see committed catalog changes while the data transaction stays repeatable-read. One PostgreSQL backend cannot hold both isolation levels at once. Reading `pg_attribute` again inside the repeatable-read transaction would miss committed DDL. That would weaken the current-catalog check. This review does not ask for that change.

The smallest upstream interface that can satisfy the original budget without that weakening is a new frozen decision on `oo.source.pgsql.snapshot/v1`, made before any edit to this child:

- Authorize `SnapshotOwner::open` as two sockets, counted together before either connect, peak 2, no third socket, and require the caller to report that peak. The function already does this. The child could then match the amended budget without a behavior change.
- Or add a reviewed one-backend owner whose contract states how committed catalog changes are observed without a second session. No such function exists. `PgConnector::prepare_snapshot` is one connection and does not copy rows.

Until one of those is frozen, this child remains withheld. The public-fixture behavior above is evidence for the next decision. It is not acceptance of an enterprise copy.
