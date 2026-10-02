# a099 review — fixed forward target-segment NULL

Decision: **request_changes**.

Scope is the A085-F1 closed-segment correction and the explicit `target-fence-receipts/2` readiness requirement, including the old fence guards this migration must keep. Commit `9a71efaf9cb63a2d63061a241f6b8ee8b00426df`, tree `caa5b8eb9bb878cb310e11c0e0cc46d4e9bc5971`, single parent `82d4c85492fc1b76fa1048bbd970de2909594295`. Subject: pipeline: close unknown target segment index and count. Acceptance is the git archive of that commit plus this reviewer's probe. The working tree moved during the review and was not the fixture input.

On PostgreSQL 17.10 the final committed `0002` bytes close the unknown segment hole that A085-F1 stored: fresh generation-1 NULL mixtures raise `23514` before the fence check, the CHECK is wrapped in `IS TRUE`, lawful receipts stay byte-equal, and illegal history aborts with static `OO021` and no receipt mutation. The same run rejects approval. When `_oo.apply_receipts` is absent, `0002` raises `42P01` from `'_oo.apply_receipts'::pg_catalog.regclass` instead of contracted `OO002`. The transaction aborts. The SQLSTATE and the masked message are wrong. Expected gold was left at `OO002`.

SP04-W05, the SP04-W05-a parent checkbox, AT-PL-05, Loader, CDC, control mirror, ACK, 100 workers, physical mapping, a real vertical, dual arch, and release stay unsigned. This review does not edit the plan line that still reads unreceived.

## Identity

| Item | Value |
| --- | --- |
| Commit | `9a71efaf9cb63a2d63061a241f6b8ee8b00426df` |
| Tree | `caa5b8eb9bb878cb310e11c0e0cc46d4e9bc5971` |
| Parent | `82d4c85492fc1b76fa1048bbd970de2909594295` (one parent) |
| Ancestor of workspace HEAD at this write | yes; HEAD `d753050ff9752dc5619c9773d1f8cc15e8d89af8` |
| Workspace HEAD when the Linux archive was taken | `aeb351cfb3b8c31e305568cd655ed057d4afdbf5` |
| `83704982a8d9459c51ed7c2020c51d21c91fb8cd` ancestor of the fixed commit | yes |
| Archive tar SHA-256 | `d2ec51e8213faefc2060d1f616154b440faa18b6163da7265f4f1b1a8c872eba` |
| Archive file count | 577, links 0, mismatched 0 |
| `0000` blob / SHA-256 | `07bf5c870d29e995c28566b0f614ee2af61040c0` / `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001` blob / SHA-256 | `9808bb751fea383fc8017a0224298ee4ea189d9a` / `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0002` blob / SHA-256 | `14c73060fc212ff8e43cad31d166d3a0f8adf92e` / `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512` |
| v1 JSON blob / SHA-256 | `d60cd180436304b8584c7858f75fb10bc35149b3` / `429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521` |
| v2 JSON SHA-256 | `3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac` |

`0000`, `0001`, and the v1 JSON are the same git objects as in `83704982`. The v1 JSON SHA-256 matches that commit byte for byte. No second parent and no replaced v1 blob appear on this commit. Older evidence files were left as they were.

`0002` does not insert `_oo.schema_migrations`. The probe applicator inserts `target-fence-receipts/2` after the file returns, in the same transaction, as `oo_target_owner`, after `pg_advisory_xact_lock(6404202605)` and `migration_lock FOR UPDATE`. `CREATE FUNCTION _oo.segment_contract_ready` and `CREATE VIEW` are not `OR REPLACE`. A forced second execution of the file raises `42723` (`function "segment_contract_ready" already exists`) and leaves `applied_at` unchanged.

## What stayed in scope

In scope: unknown index/count/bitmap combinations, three-valued SQL at the CHECK and in `commit_apply_receipt`, fresh and upgraded installs, illegal existing rows, readiness, and the fence regressions named below.

Out of scope, and unsigned here: full Loader, the fence driver as a pipeline, ACK, CDC, 100 workers, physical mapping, full SP04, and release. A passing schema fixture is not those gates. `docs/03-sources-and-ingestion.md` is source ingestion and is not the fence authority for this slice. `docs/04-pipeline.md` keeps the shard advisory lock and the control-library heartbeat separate from target-write exclusion, writes quality rows in the target transaction, and puts reject rows in the control database before `XACK` / `XTRIM`. `docs/17-rust-implementation.md` treats the jobs lease as control-library task exclusion and requires SECURITY DEFINER functions to be checked by schema, signature, and GRANT. Plan AT-PL-05 still demands kill points across reject, target COMMIT, control mirror, XACK, XTRIM, and the lease. Those kill points were not executed. `execution/prompts/linux-msrv-focused-checks.md` asks for one finite linux/arm64 Rust 1.94 smoke and defers the full arm64/amd64 matrix to integration. That matrix was not repeated.

A085-F2, A085-F3, and A085-F5 stay open. They are recorded residuals, not items this migration claimed to close, and they are not waived by the segment results.

## Linux arm64, Rust 1.94

One builder. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `--pull never`. Run `c30452da-030f-4d51-aee8-74b51c9d227f`, container `oo-a099-msrv-c30452da-030f-4d51-aee8-74b51c9d227f`. Private bridge, tmpfs `/tmp`, named cargo and target volumes, no host port, no socket, no shared host cache, `--cap-drop ALL`, no capability added, `no-new-privileges`, not privileged. `linux/builders-before.txt` at `2026-10-02T11:52:25Z` listed no host toolchain comm and no rust-builder label.

`RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo `1.94.0 (85eff7c80 2026-01-15)`, Debian 13. Clippy was downloaded into that container (`airgap` is not claimed).

| Phase | Exit |
| --- | --- |
| `rustup component add clippy --toolchain 1.94.0` | 0 |
| `cargo fetch --manifest-path Cargo.toml --locked` (no `-p`) | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings` | 0 |
| docker run / worst | 0 |

Elapsed 35 seconds. `segment_null_correction` reported `1 passed; 0 failed; 1 ignored`. The ignored test is `live_segment_null_correction`. Rust 1.94 compiled that test and did not execute it. This smoke is package proof. It is not PostgreSQL proof of the final migration.

The author's live fixture is a separate, earlier binary. `evidence/.../a092/.../REPORT.md` records Homebrew `rustc 1.98.1 (48a229cea 2026-09-01)` for that `#[ignore]` test, then three harness failures and a later `live=pass` in 26.77 seconds, and says the process was compiled before the manifest-dir and two Clippy edits. `HANDOFF.md` says the same and says Rust 1.94 skipped the live test. Those files were not rewritten. The sentence in the author report that still names a088 for root and a093 for the connector is stale relative to this assignment: main root is a095 and the connector is a098. This review leaves that report unchanged.

The product MSRV record for this review is the 1.94 container above. The PostgreSQL record is the attempt-3 probe below, which fed the archive's `0002` bytes to PostgreSQL 17.10. The probe binary itself was compiled with host Homebrew `rustc 1.98.1`, edition 2021, `forbid(unsafe_code)`. That compiler is the harness compiler. It is not the product MSRV.

## PostgreSQL 17.10, attempt 3

One fixture. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `--pull never`, `--data-checksums`, scram-sha-256, no published port. Run `3c4c956c-6a7c-4695-a563-83a03d42f2bb`. Container `oo-a099-pg1710-3c4c956c-6a7c-4695-a563-83a03d42f2bb`, network `oo-a099-pg-3c4c956c-6a7c-4695-a563-83a03d42f2bb-net`, volume `oo-a099-pg-3c4c956c-6a7c-4695-a563-83a03d42f2bb-data`. Labels `oo.project=open-ontology`, `oo.owner=a099`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a099`, `oo.kind=postgres`. Privileged false. Capabilities: drop ALL, add CHOWN, DAC_OVERRIDE, FOWNER, SETGID, SETUID. `no-new-privileges`. tmpfs `/tmp`. `pg/capacity-postgres.txt` at `2026-10-02T12:25:21Z` recorded `own_pg_all=0` and `running_rust_builders=0`. `pg/postgres-before.txt` is empty.

`SELECT version()` returned `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu`. The following line of that capture is `on` (`data_checksums`).

Probe exit **1**. `require_fail=1`, `harness_fail=0`, `residual=4`, `cases=320`. The only `pass=false` require is `missing-v1-OO002` with `state=42P01`. `pg/attempt.txt` is `3`. `pg/ready.txt` still says `attempt=2` because attempt 3 did not rewrite it. The decision files are `attempt.txt`, `probe.exit`, `counts.txt`, `summary.json`, and `console.log`.

Snapshot SHA-256 values are hashes of the admin `SELECT` text (source, generation, batch, payload digest, shard, fence generation, index, count, bitmap length, bitmap hex, `target_commit_at`, and the three completion flags). Equal hashes mean those columns stayed byte-equal.

| Snapshot | SHA-256 before | SHA-256 after |
| --- | --- | --- |
| RED three-row history | `f9a1abeeef26a7cee4117f54d8323d9d35521a613c7c08bd1052835e81fdea3e` | same |
| Isolated null index | `1c9f71ceabe664ebf31a94aaf2e477b5dcdc6dcce8285cc36abb44c6641223a7` | same |
| Isolated null count | `0532af69e69e28b837cae18c672eefd60c6a984e71efaef588c818ea3ce46fda` | same |
| Isolated null both | `730ec2d94b86f20bf81847c3f2767e366c30cd230ac683ead8d8608d8c3ab4cc` | same |
| Five-row mixed history | `064adbfe28928bfff9e7523c39936dc37fe453ddfe18becc27955db81ccce7fe` | same |
| NOT VALID planted rows | `6884caab029985752e632efcaba8e917fecc45566e95139a6b1f15b2a27b9e40` | same |
| Lawful upgrade, also after replay | `25e27c5ddda2aa83206d7eb384d953112feb43e8621599fde20df444f1cd990b` | same, and replay same |
| Forced `OO003` history | `e76b7e8fff8114b05bb2b4a905b4961d43fe0b2f6f9b838ae965e2445a5c7821` | same |

## A085-F1 oracles that held

v1 still stores the three RED shapes at generation 1 with a one-byte bitmap and returns `ADOPT|1` `RESULT|inserted`: NULL index with count 1, index 0 with NULL count, and both NULL. On that same v1 database a second call with a stale generation still raises `OO013` (`red-stale-hide`). Forwarding that history through committed `0002` raises:

`ERROR: OO021: refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts`

The v1 function body still has no `IS NOT TRUE`, the v1 CHECK still has no `IS TRUE`, and no `target-fence-receipts/2` row is inserted. The same `OO021` text and unchanged snapshot hold for each shape in its own database, so one failure does not hide the next, and for a five-row history (null index, null count, both null, a second null count, and both-null with an 8192-byte bitmap).

The v1 CHECK cannot store a strictly FALSE segment predicate. Rows that make the OR UNKNOWN are planted through the v1 function. Rows that make the OR FALSE were planted by dropping the check, inserting, and adding the exact v1 expression `NOT VALID`. `bypass-all-fields` kept every planted row, kept that NOT VALID form, and still raised `OO021`. A NULL bitmap cannot be a stored history row: the column is `bytea NOT NULL`, and a direct INSERT raises `23502`. The function rejects a NULL bitmap earlier as `23514` `closed completion metadata`.

Fresh v2 calls, each on its own connection:

| Shape | Result |
| --- | --- |
| NULL index, count 1, one byte | `23514` closed segment metadata |
| index 0, NULL count, one byte | `23514` closed segment metadata |
| both NULL, one byte | `23514` closed segment metadata |
| absent: both NULL, empty bitmap | inserted |
| present, one byte, legal index | inserted |
| present, 8192 bytes, legal index | inserted |
| 8193 bytes | `23514` closed segment metadata |
| negative index, negative count, count 0, index = count, empty bitmap with a present index | `23514` closed segment metadata |
| NULL bitmap | `23514` closed completion metadata |

The cross-product is 180 cases (`6` index values × `6` count values × `5` bitmap lengths), `grid-001` through `grid-180`, `failed=0`. Legal means the bitmap is not null and either both index and count are null with length 0, or both are non-null with count ≥ 1, index ≥ 0, index < count, and length in 1..=8192. Every other cell raised `23514`.

A stale generation on the holding backend still raises `OO013` for a legal absent segment, and `23514` for an illegal segment, with no receipt stored (`stale-legal-OO013`, `stale-illegal-23514`, `stale-not-stored`). The segment check runs before `_oo._load_held_generation`, so an illegal shape is not reported as a stale fence.

Three-valued logic on a scratch table: the old OR accepts UNKNOWN (`true` flag), and `(expression) IS TRUE` rejects that row (`false` flag). `three-flags.stdout` is `true|false`.

Lawful v1 receipts (absent, one byte, 8192, and a partial legal present segment) upgrade with generation, payload digest, and `target_commit_at` unchanged. Replay of those rows returns `RESULT|replay` and the snapshot SHA-256 stays `25e27c5d…`. Fresh install and the lawful upgrade both show `READY_BEFORE|false`, then after the applicator insert `READY_AFTER|true`, `V1_READY|true`, `V1_OF_V2|false`. v1 `schema_ready` of the v1 file hash stays true. v1 `schema_ready` of the v2 file hash stays false. A v1 ready marker does not authorize v2.

Repeat of the same checksum leaves `applied_at` at `2026-10-02 12:25:45.479291+00`. A different checksum is the probe applicator DO block in `pg/sql/mismatch-protocol.sql`: it raises `OO001: migration checksum mismatch` and does not execute `0002` again. That `OO001` statement is not inside `0002`. Updating a stored checksum in place raises `OO014`. Forced `OO003` after `0002` and before the v2 bookkeeping row restores the constraint, the function, the marker, and the receipt snapshot.

## Blocking finding

`0002` lines 13–27:

```sql
IF to_regclass('_oo.apply_receipts') IS NULL
   OR NOT EXISTS (
     SELECT 1
     FROM pg_catalog.pg_constraint AS constraint_row
     WHERE constraint_row.conname = 'apply_receipts_segment_check'
       AND constraint_row.conrelid = '_oo.apply_receipts'::pg_catalog.regclass
   )
THEN
  RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
    USING ERRCODE = 'OO002';
```

`to_regclass` is safe when the relation is missing. The `::pg_catalog.regclass` cast is planned anyway. On a database that has roles and no `_oo.apply_receipts`, PostgreSQL raises:

```text
ERROR:  42P01: relation "_oo.apply_receipts" does not exist
QUERY:  to_regclass('_oo.apply_receipts') IS NULL
     OR NOT EXISTS ( ... '_oo.apply_receipts'::pg_catalog.regclass ... )
CONTEXT:  PL/pgSQL function inline_code_block line 3 at IF
LOCATION:  regclassin, regproc.c:911
```

Evidence: `pg/cases/missing-v1.stderr`. The contracted state is `OO002` and the contracted message is `target-fence-receipts/1 segment relation is absent`. The oracle was not changed. When the relation exists and the constraint has been dropped, the same DO raises `OO002` (`invalid-shape-OO002`, `pg/cases/shape-forward.stderr`). The absent-relation path is the one that fails. The transaction does not leave a partial v2 marker. The caller still sees the wrong SQLSTATE, so a missing v1 install is not the static masked `OO002` the contract publishes (`bookkeeping_incomplete`).

A repair has to avoid evaluating `regclass` on the absent name: use `to_regclass` for the constraint lookup, or catch `undefined_table`, and then RAISE `OO002`. This review does not apply that repair.

## Readiness and the stored lie

`_oo.segment_contract_ready(text)` is `STABLE SECURITY INVOKER`, owner `oo_target_owner`, `search_path=pg_catalog, _oo`. It is true only when the stored `target-fence-receipts/2` expand checksum equals the caller argument and the argument matches `^[0-9a-f]{64}$`. The function body is a `SELECT` on `_oo.schema_migrations`. It does not open `0002_target_segment_null_expand.sql`. The view `_oo.segment_service_readiness` is `security_invoker`. `schema_version_present` is the version and phase comparison. Catalog `readfile` count on this install is 0.

The stored-lie database booked checksum `e` repeated 64 times. `lie-view.stdout` is that digest followed by `|true`. `segment_contract_ready` returned true for the lie. The file hash was not an input to the function. v2 JSON states the same rule: `segment_contract_ready_reads_migration_file` is false, and `v1_schema_ready_authorizes_this_contract` is false. The external conjunction remains an installer obligation: hash the file outside the database and require `segment_contract_ready` of that digest. This review records the lie. It does not treat the function as a file reader or as a closer of applicator truth. A085-F4 is answered only as that conjunction.

Runtime `SELECT` of the readiness flags returned `true|false|true|true` (`runtime-ready.stdout`): v2 ready for the real file hash, v1 `schema_ready` false for the v2 hash, v1 `schema_ready` true for the v1 hash, and the lie path is the separate database above.

## Catalog, grants, and search_path

From `pg/cases/catalog.stdout` after a v2 install created as `oo_target_owner`:

| Object | Owner | Security definer | search_path / form |
| --- | --- | --- | --- |
| `commit_apply_receipt` | `oo_target_owner` | true | `pg_catalog, _oo` |
| `segment_contract_ready` | `oo_target_owner` | false (invoker) | `pg_catalog, _oo` |
| `apply_receipts_segment_check` |  |  | closed OR expression wrapped in `IS TRUE` |
| `segment_service_readiness` | `oo_target_owner` | invoker view | `{security_invoker=true}` |

`oo_pipeline_runtime` is not a member of `oo_target_owner` (`member_runtime|false`). The bootstrap login is not a member either. Runtime has EXECUTE on `commit_apply_receipt` and `segment_contract_ready`. Receipt ACL for runtime is false for SELECT, INSERT, UPDATE, and DELETE. A runtime INSERT raises `42501`. The function body contains no `GRANT` of the owner role. Nobody's CONNECT to `db_fresh` fails with `FATAL: permission denied for database "db_fresh"`. The probe's SQLSTATE field on that FATAL line is empty because the message has no five-character state token before the first colon; the require passed on the error text.

Search-path poison installed `clock_timestamp` in `a099_evil` and in `_oo`, and `_oo.octet_length(bytea)` returning 0. `commit_apply_receipt` still stored `2026-10-02 12:25:44.754763+00`. `pg_catalog` wins under the function's `search_path`.

The app install owner (`oo_a099_bootstrap`, NOINHERIT, `GRANT oo_target_owner` with `INHERIT FALSE` and `SET TRUE`) creates the definer by `SET ROLE`. The new function and view are created as the current user, so they are owned by `oo_target_owner` on this path. No `GRANT oo_target_owner TO oo_pipeline_runtime` was added. Unsafe adoption of an existing definer owned by another role was not given a passing shortcut; the created objects above are owned by `oo_target_owner`.

## Fence regressions that held

Same-session replay slept 8 seconds. Admin stamps mid-sleep and after exit are both `STAMP|2026-10-02 12:25:47.958888+00|1|` plus 64 `e` characters, and stdout contains `RESULT|inserted` and `RESULT|replay`. A new backend replay returns `RESULT|replay` with that same stamp. Generation passed to the new backend is the generation `adopt_target_generation` just returned, not a hardcoded 1.

Conflicting digest raises `OO010`. The side-row count is 0. The reject and the side insert leave together.

While one runtime backend holds the shard lock, a second adopt on that shard raises `55P03` (`target shard lock is held by another backend`). A different shard adopts at generation 1 and inserts. The `55P03` stderr also contains `WARNING: 01000: you don't own a lock of type ExclusiveLock` from the v1 exception handler unlocking a lock that `pg_try_advisory_lock` did not acquire. The holder's lock stayed held. That warning is observed. It is not a new segment-rule defect.

After the holder is gone, takeover moves the shard to generation 2 and owner `22222222-2222-4222-8222-222222222222`. The old token is blocked with `55P03` while the new holder sleeps. A later call at generation 1 raises `OO013` and stores no `after-old` row. Replay of the original receipt returns `RESULT|replay` and keeps fence generation 1, `target_commit_at` `2026-10-02 12:25:56.731362+00`, and the 64-`e` payload.

A second `pg_advisory_xact_lock(6404202605)` raises `55P03` in 2088 ms while the first session holds that key (`cases/mig-wait.ms`).

Kill before COMMIT: backend seen, `57P01`, receipt absent, retry `RESULT|inserted`. Kill after COMMIT: receipt present, stamp `STAMP|2026-10-02 12:26:02.352937+00|1|` plus 64 `e`, replay returns `RESULT|replay` and the stamp matches. Client disconnect before COMMIT: the `a099drop` backend was visible (`pg_stat_activity` count 1 in `drop-leak.stdout`) and the `drop-batch` receipt count was 0. The probe then terminated that backend. The receipt count stayed 0. The durability observation is the absent row. The leftover backend count is recorded and was cleaned by this probe's own terminate.

Bad codec still raises `OO015`.

These results are receipt-schema results. They do not show that a real user table is fenced. A085-F3 stays open: a runtime member inserted a user-table row with no adopt and no receipt, inside a transaction this probe rolled back after observing it. Publication count stayed 0.

## Residuals left open

A085-F2. Owner plus the writer GUC can still insert a receipt. This probe rolled that transaction back after observing it (`owner-guc-insert-still-possible`). Superuser and owner bypasses from a085 are not closed by `0002`.

A085-F3. The receipt function does not fence later table DML. The arbitrary-DML gap stays explicit.

A085-F5. `owner_token` remains the token stored by whoever adopted the shard. It is not an external worker identity. This probe did not reopen the a085 nonce, gateway, and forged-token matrix, and it does not close F5.

A085-F4, for this contract only. The stored checksum is the caller value. The installer still has to hash the file and conjoin that hash with `segment_contract_ready`. The lie experiment is the evidence that the function does not do that hash.

`55P03` paths emit warning `01000` as described above.

Full supply-chain attestation, a second architecture, runsc, customer data, and protected CI were not part of this run. Missing audit tools are not a pass.

## Preserved attempts

Attempt 1, probe exit 2, `require_fail=10`, `harness_fail=1`, `residual=4`, `cases=316`. Container `oo-a099-pg1710-2953befa-9b60-48c9-a06f-707571da017f` and its network and volume, removed later by exact name. Classification in `pg/attempt-1/classification.txt`: harness, not product. Upgrade re-executed `0001` and stopped at `42P06` (`schema "_oo" already exists`), so lawful byte-equality and forced `OO003` never reached `0002`. `sqlstate()` returned on the first non-ERROR line, so same-shard and old-writer looked empty even though the stderr contains `ERROR: 55P03` after warning `01000`. Stamp queries ran as runtime and raised `42501`; the replay and conflict bodies had already returned `RESULT|replay` and `OO010` with side count 0. The product miss already present, and left unchanged, is `missing-v1` `42P01`.

Attempt 2, probe exit 1, `require_fail=3`, `harness_fail=0`, `cases=320`. Container `oo-a099-pg1710-79f07ac9-3b03-470f-94ce-877f2595ea00` and its network and volume, removed by exact name. Two of the three failures were harness: `V2Upgrade` committed `0002` and inserted the marker, then the script omitted the `READY_AFTER` and `schema_ready` selects. A follow-up on that same database, saved as `pg/attempt-2/cases/lawful-ready-supplement.stdout`, returned `READY|true|true|false`. The third failure is the same product `42P01`. Gold was not edited. The select emission was fixed in the probe before attempt 3.

Attempt 3 is the decision run. Attempts 1 and 2 are not a product pass and were not deleted.

## Cleanup

Linux builder cleanup: `rm_exit=0`, `network_exit=0`, `cargo_volume_exit=0`, `target_volume_exit=0`.

Attempt-3 PostgreSQL cleanup: `rm_exit=0`, `network_exit=0`, `volume_exit=0`, `tmp_removed=true`, `exists=false`. Probe cleanup exit 0. That removed `/tmp/oo-a099-eef831aa-dba4-4af2-a78b-541bc9e0a099` (snapshot, secrets, probe binary). The probe source remains at `probe/src/main.rs`.

A later inventory in this review, filtered by `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a099` and by the name prefix `oo-a099-`, listed no containers, networks, or volumes. The tmp directory is absent. No prune and no foreign cancellation.

Evidence stores SQLSTATE text, snapshot hashes, and environment key names (`pg/env-keys.txt`). It does not store passwords or private keys.

## Unsigned

Plan `specs/open-ontology-v0.1-plan.md` line 90 remains `- [ ] SP04-W05-a` with the note that a092 delivered `9a71efa` for a099 to check and that it is unreceived. This review does not check that box and does not sign SP04-W05 or AT-PL-05.

The parent slice stays open until the absent-relation path raises `OO002` with the static message, on the final committed file, under an independent PostgreSQL 17.10 run. The segment oracles above can be reused. They are not a substitute for that SQLSTATE.
