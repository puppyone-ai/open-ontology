# Findings

Verdict: APPROVE LIMITED for `install` and `verify_readiness` at `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`.

## Dedicated caller connection

`install` is `pub async fn install(conn: &mut PgConnection)` at `crates/oo-pipeline/src/target_installer.rs:309`. `verify_readiness` takes the same connection type at line 392. The crate does not construct a pool, read a password, or choose a host. The source does not contain `PgPool`. The oracle opened each call with `PgConnection::connect_with` and `PgSslMode::Disable` on the private network. No host CA was mounted.

## Lock, then the schema transaction

`install` begins one transaction at line 317, sets the local lock timeout, then runs `pg_advisory_xact_lock(6404202605)` at line 332. When `_oo.migration_lock` exists it locks that row `FOR UPDATE` at lines 343–350 before the route runs. Scripts and marker inserts share that transaction. `commit` is line 384. `fail` rolls it back.

The oracle held `pg_advisory_xact_lock(6404202605)` on a second connection to the same empty database. `pg_locks` showed `classid|objid` `1|2109235309`. `install` returned sqlstate `55P03` after at least 10 seconds. Readiness stayed `not_installed` and `_oo` was absent. After the holder rolled back, `install` committed the fresh catalog.

## Marker and schema atomicity

`OO001` is raised inside the transaction at lines 359–373 and the transaction is rolled back. The lie checksum was still the stored v3 row. No mapping row appeared. `verify_readiness` returned `ChecksumMismatch`.

`OO002` from a dropped `apply_receipts_segment_check` is `Route::AbsentRelation` at lines 646–658. The pre-call migration text was unchanged and readiness was `Incomplete`.

`OO021` from `0003` on an illegal receipt returned through `fail`. The v3 marker was absent, the receipt row was unchanged, and readiness was `Incomplete`.

The mapping-only success inserted one row, `physical-mapping/v1`, in the same transaction as `0004`. Bootstrap, v1, and v3 `applied_at` values were unchanged. A later replay left the new mapping `applied_at` unchanged as well.

## Embedded digests

`compiled_digests` hashes the `include_str` bytes of `0000`, `0001`, `0003`, and `0004`. The oracle compared those digests with the file bytes and with the readiness token:

| File | SHA-256 |
|---|---|
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0002_target_segment_null_expand.sql` | `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `0004_physical_mapping.sql` | `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95` |

`0002` equals `HISTORICAL_V2_SHA256`. The installer source has no `include_str` of that file. Git blob ids are in `SOURCE.txt`. `0000`–`0003` match a108 `6cd955af`. `0004` matches a142 `b5415544`. Those SQL bytes were not retested as a privilege matrix.

## Routes the author live fixture did not build

`Route::InstallMapping` is lines 525–532 and 636–644. It runs `0004` only and inserts only `physical-mapping/v1`. The author report said no live database started from a valid v3 row without a mapping row.

The oracle built that pre-state from the frozen `0000`, `0001`, and `0003` bytes, inserted the three markers, and confirmed `physical_mappings` was absent. The product call was `install`. It returned `InstalledMapping`. v2 row count stayed 0. `verify_readiness` succeeded with `retains_historical_v2` false. Replay did not change `applied_at`.

Valid v1 did not insert a v2 row. Valid v2 kept its checksum, `applied_at`, and the one legal receipt.

## Cluster-wide roles block a second fresh database

`fresh_catalog` at lines 491–499 requires both product roles to be absent. `route_for` at lines 509–510 sends any non-fresh catalog that lacks `_oo.apply_receipts` or `apply_receipts_segment_check` to `0003`. `0003` raises `OO002` when the relation is absent.

Observed on database `oo_a152_second` while `oo_target_owner` and `oo_pipeline_runtime` still existed from `oo_a152_empty`:

```
second=OO002
second_ready=not_installed
second_schema=0
first_unchanged=1
```

`0000_bootstrap_roles.sql` was not edited. A cluster that already has the product roles does not get a second fresh install from this function. The limited approval does not cover an arbitrary target database on such a cluster.

## Runtime cannot install

`principal_can_install` allows a non-superuser only when the catalog is not fresh and the session is a member of `oo_target_owner`. `0000` does not grant `oo_target_owner` to `oo_pipeline_runtime`. The runtime member received `VerifiedReadiness` and `InstallError::NotAdministrator`. The migration text did not change.

## Not approved

SP04-W05, SP04-W05-a, AT-PL-05, the Loader, CDC, control mirror, ACK, TRIM, fenced user-table `COPY`, publications, and source registration remain open. `verify_readiness` does not take the migration lock. The contract says it must not. The caller-hash functions are not this readiness check.

## Harness failure kept

`linux/probe/attempt-1/` is the first probe. `docker-run.exit` is 2. `console.log` ends at `clippy component missing`. The smoke container had downloaded Clippy into its own layer. The next container was the original image. The check was removed from the oracle script. The green probe is `linux/probe/`. Production source was not edited.
