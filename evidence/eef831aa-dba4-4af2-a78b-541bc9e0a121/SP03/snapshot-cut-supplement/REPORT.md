# REPORT — a121 missing-proof supplement

**Verdict: REQUEST_CHANGES.**

Public `SnapshotOwner` on commit `2b6dd57a0dc3d36e098268638c0364af247d07fb` was executed against one private PostgreSQL 17.10 database. Attempt 6 is the proof run. Its oracle process exited 101 with `fail_count=1` and `fail_names=add_column_active_current_schema`. Same-cut rows, the column-privilege lattice, the deadline-before-idle case, the idle `25P03` case, and the SQL audit were measured on that run. An `ADD COLUMN` committed while an owner was open still returned `SnapshotFinished` for the old two-column token. That current-schema stop is unmet. This review does not approve R1.

R2, R3, R4, whole SP03, type approval, supply, license, and release stay unsigned. The coordinator owns plan checkboxes. This review did not edit them. Backend `Drop` was not retested and is not an R3 cancellation proof.

## Identity

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a121` |
| Role | Grok 4.7 xhigh, sole reviewer, no subagents, no worktree |
| Reviewed commit | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Tree | `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` |
| Parent | `e5a066622e0409762fe44f11c4d303d2468a3290` |
| Workspace tip at report time | `74fe4f0b8027b9a4b4e3c300998313a596634ae0` |
| Slot | A |
| Slot B | a120 handle 7304, live vendor writer, not cleaned |
| Report stamp | `2026-10-03T00:33:24Z` |
| Cleanup | `cleaned_at=2026-10-03T00:31:46Z` in `live/cleanup/result.txt` |

`git rev-parse 2b6dd57^{tree}` is `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc`. `git rev-parse 2b6dd57^` is `e5a066622e0409762fe44f11c4d303d2468a3290`.

The smoke and review builders used a `git archive` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` at `2b6dd57`, extracted under `/tmp/oo-a121-<uuid>/archive`. The probe `live/a121_oracle.rs` was copied onto a separate source directory. The archive had no `tests/a121_oracle.rs` (`smoke/launch.sh` tests that absence). Product, vendor, manifests, locks, CI, contracts, docs, spec, plan, and older evidence were not written.

Blobs at `2b6dd57`:

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `53aa65f239497d5cad451a2c518669a4e6ec770c` |
| `crates/oo-connectors/src/session.rs` | `5211832e3f9b6b297e6db21cfcd0e72d920d4704` |
| `crates/oo-connectors/src/sql.rs` | `07febf17b415abcb785644f60690c47939e0e50f` |
| `crates/oo-connectors/src/error.rs` | `16143c3f809af5f73810d49fa2051a92bef14472` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `6306ae1cc3c5ff84809c9150212a66844986ba9a` |

`product-sha256.txt` was checked `OK` by `sha256sum -c` inside the smoke builder and every review builder (`product_hash_exit=0`). File hashes:

| Path | sha256 |
| --- | --- |
| `Cargo.toml` | `a277789df0b8c74febf113a2acc80a0a9f4b51bdeaa662e5d4f761be2489046b` |
| `Cargo.lock` | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `rust-toolchain.toml` | `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8` |
| `crates/oo-connectors/src/lib.rs` | `460ae2460ace386f54a715a626de5a73d1ba9a9104aa6cf656ff5dfab0abab06` |
| `crates/oo-connectors/src/snapshot.rs` | `86361bc3254edfdc67f4fa70e2a304025da2298ecc76c86b71dd7e65b846e088` |
| `crates/oo-connectors/src/session.rs` | `70c16cea3aa12af9d09981714816f03522dcfdba4855116892c271d58c535e10` |
| `crates/oo-connectors/src/sql.rs` | `9434b059e24dd7ae9f113fa2e7572ed78f274144821d7fa86345e6e638e227ae` |
| `crates/oo-connectors/src/error.rs` | `49b19e22888247ce40e5972ccdd95136a6e6af9757fe86fae87b30b1699a4896` |
| `crates/oo-connectors/src/types/mod.rs` | `86fa06414a49e6b97425651f0ffc750bc634fe5423d1af70216b18da03be4b25` |
| `crates/oo-connectors/tests/snapshot_live.rs` | `b43dc30e4b791c0c5677906b41ce2c2714d615df701dbc1b589027881fb20bb7` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `095dd3b364d8284ec4a665a9125bfe0bfdff3b62736d461ae61a4fb271e571e0` |

The extracted tar was not kept. After cleanup, the same `git archive --format=tar` command was run twice and both hashes were `e81cb9d790aac91285d6a33981b5fb3878ccfb76f1ca1a1b270e89c3758dc62c`.

a116 `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, `live/phases/oracle-outcome.txt`, `live/a116_oracle.rs`, and the a116 SQL log were read. `execution/decisions/a116-snapshot-review-receipt.md` refuses to sign R1. a116 assertions are not proof for this supplement. a116's eight fail marks stay failed in that evidence and were not rewritten:

`same_item_tokens`, `same_line_21_tokens`, `same_side_absent_23`, `deadline_probe_finish`, `deadline_finish`, `inflight_column_probe`, `add_column_agreement`, `add_column_finish`.

a116 also recorded `identity_drift source_inspected_not_executed` and `nested_transaction source_inspected_not_executed`, with `fail_count=8` and oracle exit 101. Quiet, abort, TLS, and generation-refusal observations in that file were not retested here.

## Capacity and allocation

Global maximum was 2 Rust builders including host and install. Own maximum was 1 builder and 1 private PostgreSQL 17.10 verify-full fixture. Each launch recorded `host_cargo=0`, `rust_builders=0`, `global=0`, `own_take=1`, `slot=A`, `slot_b=a120_handle_7304_live_vendor_writer`. Attempt 6 capture is `2026-10-03T00:24:11Z` in `live/capacity-6.txt`.

Not in this allocation: a second own builder, a host `cargo`/`rustc`, a historical matrix, a supply or `cargo deny` rerun, R2–R4, product or contract edits, and new dependencies. Clippy for 1.94.0 was installed inside the review container (`rustup component add clippy --toolchain 1.94.0`) and counts as that builder.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`. Postgres image sha256 `3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Containers used `--pull never`, `--init`, `--security-opt no-new-privileges`, `--cap-drop ALL`. Postgres also had `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. No host port. Inspect lines record `privileged=false` and `oom=false`.

## Smoke

Container `51936e834dc7cde6989e725b31c34026194d1ccde6b1bda8be666a0ede92825e`, exit 0. `smoke_start=2026-10-02T23:57:36Z`, `smoke_end=2026-10-02T23:58:17Z`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Commands, from `smoke/launch.sh` and `smoke/inside.sh`:

- `cargo fetch --manifest-path Cargo.toml --locked` — `smoke/phases/fetch.exit` is `0`
- `cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib -- --test-threads=1` — console: `test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`, `lib_exit=0`
- `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings` — `smoke/phases/clippy.exit` is `0`

`smoke/phases/worst.exit` is `0`. `smoke/docker-run.exit` is `0`. Package tests that need `OO_PG_FIXTURE` stayed not-run. Smoke was not rerun.

`live/phases*/lib.exit` and `live/phases*/fetch.exit` sit on the shared target volume and were copied out with each review. `live/inside.sh` prints `lib_retest=not-run` and does not run `--lib`. Those files are not a second lib run.

## Live attempts

Database `lotdb`, alias `a121pg`, server log: `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1)`. The database was not recreated. Later attempts kept their logs. Review command on attempts that reached the oracle: `cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test a121_oracle -- --test-threads=1 --nocapture`.

| Attempt | Container | Docker exit | Window (UTC) | What the files show |
| --- | --- | --- | --- | --- |
| 1 | `…-rust-review` `1e0aad5e6caf2fba99eebdaff053ce4391aa2f1fa8b44a6f7ca0bc4d731ec83a` | 101 | 23:58:53–23:58:56 | `compile_exit=101` (`E0521`, `E0308`). `clippy_exit=1`: cargo-clippy was not installed for 1.94.0. Oracle recompiled the same errors and `oracle_exit=101`. No `oracle-outcome.txt`. Owner not opened. |
| 2 | `…-rust-review-2` `22eb1d44c44d4af584f236340c6ed6bfffca079464f578dd80508ea6e8830f4a` | 101 | 00:10:02–00:10:07 | `compile_exit=0`, `clippy-add.exit=0`, `clippy.exit=101` (`clippy::collapsible_if`). `oracle.status=skipped`, `oracle.exit=0`. |
| 3 | `…-rust-review-3` `60a05ccbf64d1d74d76cb6b2232fba2975cd3859fc2ed55f20cf8ea24e1dc2af` | 101 | 00:11:09–00:11:40 | Clippy 0. Oracle ran. `fail_names=writer_blocked,inflight_denial,add_column_active_current_schema,cut_log_order`. |
| 4 | `…-rust-review-4` `5490a3c39b1350ad80707259b2383acb7f3bcd73f8d5f4ab659fbbeea76f7eb6` | 101 | 00:17:39–00:18:11 | Clippy 0. `fail_names=writer_blocked,add_column_active_current_schema,cut_log_order`. Lock visibility from the holding session stayed 0. |
| 5 | `…-rust-review-5` `f6cf1c8466c00de97fab6098a301c69302ba101efeaf928df1fb117d6f2f117c` | 101 | 00:22:18–00:22:44 | Clippy 0. Writer blocked, then SQLSTATE `42501` on `lot.tick` (`canary_after_writer=0`). `fail_names=canary_writer_delta,fresh_three_sets,add_column_active_current_schema,cut_log_order`. |
| 6 | `…-rust-review-6` `42792620d01212af2be53b246d160293e939cc57b1b0cfaebef00c3dfcfb772b` | 101 | 00:24:11–00:24:37 | Clippy 0. Oracle 22.66s. Panic at `a121_oracle.rs:51`: `a121 fails=add_column_active_current_schema`. `worst_phase_exit=101`. |

Attempts 3–5 are retained harness history. Attempt 5's `cut_three_sets=pass` compared unchanged old rows because the writer rolled back. It is not the mutation proof. The `ADD COLUMN` finish-with-extra result is stable across attempts 3, 4, 5, and 6.

Before attempt 6 the fixture role `lot_writer` was granted `SELECT, UPDATE` on `lot.tick`. `init.sql` had granted only `UPDATE`. The canary trigger is `SECURITY INVOKER` and reads `hits`, so `UPDATE` alone returns `42501`. That grant is a live fixture change. It is not in `init.sql`. A re-init would lose it.

Attempt 6 phase exits: `product-hash.exit=0`, `compile.exit=0`, `clippy-add.exit=0`, `clippy.exit=0`, `oracle.exit=101`, `worst.exit=101`, `docker-run-6.exit=101`. Console: `lib_retest=not-run`, `package_live_pg=not-run`, `snapshot_live=not-run`.

## Preregistered cut

Hashes below were recomputed on the host from `expected/` and match attempt 6 `preregister=pass`, which the probe checks before `SnapshotOwner::open`.

| File | sha256 |
| --- | --- |
| `expected/rows.txt` | `99ff6b0be78bbf745cc1ce2d55efe05b9a39c92bc4c6f6c0d79dcd638c857e17` |
| `expected/catalog.txt` | `eb9a85123f3e9f4611c058bf0833cc4fb0bfebee39484ee3ac0cbc34becd44e9` |
| `expected/writer.sql` | `0eab79c34d8c9d14fe8f2157ac651f5967d205cd3bd205db7290fe06d551d79f` |

`expected/register.txt` says `registered_before_owner=yes`. Writer text:

```text
UPDATE lot.batch SET title = 'aspen', "odd""mark" = 'held' WHERE id = 11
UPDATE lot."child row" SET qty = 14 WHERE id = 31
INSERT INTO lot."child row" (id, batch_id, qty, note) VALUES (44, 11, 2, 'newleaf')
```

Old-cut tokens: batch 11 `int:11 text:maple bool:true text:K121 null`; child 31 `int:31 int:11 int:6 text:twig`; child 32 `int:32 int:11 int:8 null`; keys 44 and 90 absent. Fresh tokens: batch 11 `int:11 text:aspen bool:true text:K121 text:held`; child 31 qty 14; child 44 `int:44 int:11 int:2 text:newleaf`. Comparison is a set of `table|key|token`. The contract does not order probe rows.

Rows are synthetic. Source id `lot_src`. Application names `oo-pg-source`, `oo-a121-rr`, `oo-a121-writer`, `oo-a121-admin`, `oo-a121-oracle`.

## Requirement outcomes

### 1. Smoke, preregister, public API

Smoke is the first Linux arm64 Rust 1.94 run of this fixed archive for this session. No historical matrix and no supply scan. The oracle calls `SnapshotOwner::open`, `binding`, `read_probe`, and `finish`. It does not replace the reader. Attempt 6 header: `probe=a121_oracle`, `api=SnapshotOwner`, `cleanup_bound_ms=5000`, protocol `oo.source.pgsql.snapshot/v1`.

### 2. Two related tables

Attempt 6 outcome and an independent read of the last window in `live/sql-log.txt` (lines 2321–3178, `2026-10-03 00:24:15.046` through `00:24:37.574`) agree.

Log line order:

| Event | Log line | Time (UTC) |
| --- | --- | --- |
| Owner `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY` | 2345 | 00:24:15.340 |
| Side first data `SELECT ... FROM lot.batch WHERE id = $1` (no `LIMIT`) | 2419 | 00:24:15.475 |
| Admin `UPDATE lot.gate SET ready = true` | 2430 | 00:24:15.476 |
| Writer `COMMIT` | 2437 | 00:24:15.479 |
| Owner probe `SELECT id,title,ready,code,"odd""mark" FROM lot.batch WHERE id = $1 LIMIT $2` | 2445 | 00:24:15.480 |

Writer statements on lines 2434–2436 are the preregistered update of both tables and the child insert. `canary_before=0`, `canary_after_writer=3`, `canary_final=3`, `canary_oo_delta=pass`. `writer_blocked=true` with `writer_activity=515:oo-a121-admin:::active,516:oo-a121-writer:Lock:transactionid:active`. The side session recorded the old rows before the barrier. The owner was open before the barrier and before the writer commit.

Emitted old sets, all five keys, owner and side and expected:

```text
batch|11|int:11 text:maple bool:true text:K121 null
child row|31|int:31 int:11 int:6 text:twig
child row|32|int:32 int:11 int:8 null
child row|44|ABSENT
child row|90|ABSENT
```

Owner hashes: batch 11 `ddc0e3a5bbb55df0f2cd52dce01a8751e49a0f2ae53131822afe9ab44a4895ca`; child 31 `abd2559932659b33d8f8b27cff370770576036d30ef83a32ee998ea876530942`; child 32 `f193816687181167c7b554ff474763a76fae0645db9cb8884ca246c1b9f43c37`. `cut_three_sets=pass`. Finish `SnapshotFinished generation=11 pid=517`, `cut_gone_ms=0`.

Fresh generation 12, pid 519, start `2026-10-03 00:24:15.484248+00`, emitted owner, independent oracle, and expected:

```text
batch|11|int:11 text:aspen bool:true text:K121 text:held
child row|31|int:31 int:11 int:14 text:twig
child row|32|int:32 int:11 int:8 null
child row|44|int:44 int:11 int:2 text:newleaf
child row|90|ABSENT
```

`fresh_three_sets=pass`. Finish `SnapshotFinished generation=12 pid=519`, `fresh_gone_ms=0`. Earlier failed attempts kept the mismatched token lines in their outcome files.

### 3. Column ACL

`acl.sheet` before any table grant, attempt 6: `table_select=false`, `table_insert=false`, `col_id=true`, `col_tag=true`, `col_note=true`, `super=false`, `bypass=false`, `inherit=false`, `owner=postgres`, `members=0`, `relacl=` empty, `attacl=id:SELECT,tag:SELECT,note:SELECT`. `column_only_fixture=pass`. `SnapshotOwner::open` returned `kind=Privilege fact=select_not_granted`. `inflight_column_only_read=not_reached`. `check_relation` in `snapshot.rs` lines 864–866 requires `has_table_privilege` SELECT. A column-only grant cannot start an owner.

The in-flight owner was therefore opened only after `GRANT SELECT ON TABLE acl.sheet` (log line 2645). Then `REVOKE SELECT ON TABLE acl.sheet` (line 2693) and `REVOKE SELECT (note)` (line 2694). PostgreSQL cleared the column grants: `sheet_after_revoke` has every `has_*` SELECT false and empty `attacl`. The production probe is `ERR kind=Privilege fact=insufficient_privilege sqlstate=42501`. `error.rs` line 193 maps `42501` to `Privilege` / `insufficient_privilege`. Finish returned that same error. `inflight_gone_ms=0`. Marks `inflight_denial`, `inflight_not_finished`, and `inflight_backend_released` pass. The server log line 2706 is `ERROR: permission denied for table sheet`. This is a table-revoke cascade, not a one-column denial that left the other column grants in place.

Positive control `acl.wide`: table SELECT granted, then `REVOKE SELECT (note)` only (log line 2764). After that revoke, `table_select=true` and `col_note=true`, `relacl=SELECT`, `attacl` empty. Production probe `int:8 text:wool text:bolt` equals the expected token. Finish `SnapshotFinished generation=23 pid=523`. `a116_explanation=table_select_keeps_column_select`. `wide_explains_a116=pass`, `wide_gone_ms=0`. a116's successful column read is this table-level SELECT behavior. It is not a privilege breach.

Contract line 47 allows a denial to surface as `select_not_granted` or SQLSTATE `42501`. Both facts were observed on the paths above. `has_*_privilege` was queried independently of the owner.

### 4. Deadline and idle

`sql.rs` `SESSION_GUARDS` lines 6–10 bind `statement_timeout` and `idle_in_transaction_session_timeout` to the same `$1`. The public budget cannot split them. The two cases use different budget values against the client deadline.

Deadline: bind `pid=524 start=2026-10-03 00:24:16.143458+00 user=lot_reader db=lotdb app=oo-pg-source`. Server guard parameter in the owner session for the earlier cut was `30000` ms (log line 2343); this case sleeps past a 12s client deadline while the server guard stays longer. Probe and finish: `kind=Budget fact=deadline` with no SQLSTATE. `deadline_before_idle=pass`, `deadline_not_finished=pass`, `deadline_gone_ms=1`.

Idle: bind `pid=525 start=2026-10-03 00:24:28.558163+00 user=lot_reader db=lotdb app=oo-pg-source`. `idle_backend_absent_before_finish=true`, `idle_wait_ms=8172`. Finish `kind=Protocol fact=database sqlstate=25P03`. `idle_25p03=pass`, `idle_not_finished=pass`, `idle_gone_ms=0`. Log line 2958: `FATAL: terminating connection due to idle-in-transaction timeout`. Contract line 53: cleanup failure is returned and is not `SnapshotFinished`. The `25P03` fact was kept. The deadline case was not rewritten to accept `25P03`.

Release observations are `<= 5000` ms. They are not R3 cancellation.

### 5. ADD COLUMN

Old owner generation 41, pid 527, start `2026-10-03 00:24:36.888346+00`, admitted columns only `id` attnum 1 and `label` attnum 2. `ALTER TABLE drift.item ADD COLUMN extra text` is log line 3010, applied while that owner was open and before its user-table probe. Admin catalog after the alter: attnum 1 `id`, attnum 2 `label`, attnum 6 `extra` (type oid 25, nullable). Attnum 6 is the live catalog fact. Earlier drop/add cycles on this same database left dropped attnums 3, 4, and 5. The preregistered `drift_new` shape expected attnum 3, so the probe recorded `fresh_new_shape_source=observed_admin` and used attnum 6. That observed number is the one in the outcome. It was not edited to match the preregistered attnum.

Old probe token `int:5 text:pebble`, hash `a8c9bd7271a6370bfef4abd3bd55fbe4d24e35d4855b3221d98df17eb509271c`. Finish `SnapshotFinished generation=41 pid=527`. `drift_old_finished_with_extra=true`. Mark `add_column_active_current_schema=fail`.

Fresh open with the old two-column shape: `kind=Catalog fact=catalog_identity`. `fresh_old_shape_rejected=pass`.

Fresh open with the observed three-column shape, generation 43, pid 529, start `2026-10-03 00:24:37.174446+00`, schema hash `6b93cbe23b3e953d04aac66efff01ede5be7eef82803ed1568eeb1cd3e001a4b`. Probe `int:5 text:pebble null`, hash `8a3e5364217ecc6c9396e948df9bf24e605919b7fa0bf2fc1468f9cf78fd61a0`, equal to the independent autocommit oracle and the preregistered token. Finish `SnapshotFinished generation=43`.

Mapping:

- Contract line 25 defines `schema_drift` as drift from the catalog captured in the same transaction. `recheck` reads `pg_attribute` inside that repeatable-read transaction, so the new column is invisible and the old probe still returns the two-column token.
- Contract line 47 says a shape mismatch is `schema_drift`. This recheck did not observe a mismatch.
- `execution/prompts/v01-pg-snapshot-owner-review-prepared.md` line 11 requires current schema changes to stop. That requirement is unmet.
- `execution/prompts/v01-pg-snapshot-owner.md` line 15 says changed schema stops according to documented same-snapshot semantics, and says not to invent that MVCC freezes ACL checks. The ACL half was measured separately. The schema half is the gap. Old-snapshot visibility is not accepted here as a redefinition of the current-schema requirement.
- `specs/open-ontology-v0.1-spec.md` line 403, REQ-PL-07, is pipeline handling of a new nullable column (sync, unmapped). It is outside this R1 owner slice and is not used to accept `SnapshotFinished`.

Cause: `snapshot.rs` `recheck` lines 1026–1049 calls `read_attributes` lines 1244–1266, which runs `sql.rs` `SNAPSHOT_ATTRIBUTES` lines 334–341 on the owner session. `attributes_match` lines 882–897 compares attnum, name, type oid, typmod, and not-null against the admitted shape captured in the same transaction. The sets match, so the owner is not poisoned. `finish` lines 457–471 then returns `SnapshotFinished`.

Minimal next implementation: compare attributes with a current non-snapshot read of `pg_attribute`. On mismatch return `schema_drift`, poison the owner, and do not return `SnapshotFinished`. Touch `snapshot.rs` `recheck` / `read_attributes` and `sql.rs` `SNAPSHOT_ATTRIBUTES`. Do not hide attnum 6 by editing the expected catalog.

### 6. identity_drift and nested_transaction

Both are `not_runtime_tested`. Source inspection is not runtime certification.

Identity attempt: `ALTER ROLE lot_reader RENAME TO lot_reader_x` while generation 51 was open. Probe `int:1 text:still`. Finish `SnapshotFinished generation=51 pid=531`. `identity_reason=rename_executed_session_identity_unchanged`. `identity_restore=ok`, `identity_gone_ms=0`. `recheck` lines 1028–1032 compares `current_user`, `session_user`, and `database` with the binding. Renaming the role does not change those session values. The public API does not expose the connection and does not offer a second role. No product edit was made to force the branch.

Nested: `session.rs` `begin` lines 277–280 is `pub(crate)` and returns `nested_transaction` before SQL when `in_transaction` is set. `SnapshotOwner::open` starts one transaction. An integration test cannot call `begin`. `nested_reason=no_public_second_begin`.

### 7. SQL audit

`live/sql-log.txt` is 794687 bytes, 3181 lines. Password-file scan at cleanup: `live/cleanup/log-secret-files.txt` is `0`. Outcome `log_secret_hits=0`.

Last window, independent verb counts on `LOG: execute` lines, including the two admin marker `SELECT`s:

| Actor | Counts |
| --- | --- |
| `oo-pg-source` | `ROLLBACK=10 SELECT=346 START=11` |
| `oo-a121-writer` | `BEGIN=1 COMMIT=1 INSERT=1 SELECT=7 UPDATE=2` |
| `oo-a121-rr` | `BEGIN=1 ROLLBACK=1 SELECT=10` |
| `oo-a121-admin` | `ALTER=3 BEGIN=1 COMMIT=1 GRANT=1 REVOKE=3 SELECT=84 UPDATE=1` |
| `oo-a121-oracle` | `SELECT=17` |

`oo-pg-source` has zero `INSERT`, `UPDATE`, `DELETE`, `COMMIT`, `ALTER`, `GRANT`, `REVOKE`, `DROP`, `CREATE`, and `TRUNCATE` in that window. `sql_audit=pass` and `cut_log_order=pass` on attempt 6. Canary delta from the owner is 0. The writer delta is 3.

## Cleanup

`live/cleanup.sh` removes only names that start with this UUID, including `rust-review-4`, `rust-review-5`, and `rust-review-6`. It ran after attempt 6 and before these reports.

`cleaned_at=2026-10-03T00:31:46Z`. Removed containers: `…-pg`, `…-rust-smoke`, `…-rust-review`, `…-rust-review-2` through `…-rust-review-6`. A name diff against the pre-cleanup `docker ps -a` list shows those eight names removed and no names added. `oo-a064-pg1710-jobs`, `oo-a064-rust194-contract`, `oo-a064-rust194-contract2`, and `oo-a064-rust194-smoke` were still present. UUID containers, volumes, and network were absent afterward. `/tmp/oo-a121-<uuid>` was absent. No container name matched `7304` or `a120`; slot B was not removed.

## Raw exits

| Step | Exit |
| --- | --- |
| Smoke fetch / lib / clippy / worst / docker | 0 / 0 / 0 / 0 / 0 |
| Review 1 compile / clippy / oracle / docker | 101 / 1 / 101 / 101 |
| Review 2 compile / clippy / oracle / docker | 0 / 101 / 0 (skipped) / 101 |
| Review 3 compile / clippy / oracle / docker | 0 / 0 / 101 / 101 |
| Review 4 compile / clippy / oracle / docker | 0 / 0 / 101 / 101 |
| Review 5 compile / clippy / oracle / docker | 0 / 0 / 101 / 101 |
| Review 6 compile / clippy / oracle / docker | 0 / 0 / 101 / 101 |

Attempt 6 Clippy command: `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings` with `RUSTUP_TOOLCHAIN=1.94.0`. `clippy_exit=0`.
