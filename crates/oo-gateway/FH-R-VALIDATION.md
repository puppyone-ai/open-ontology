# FH-R: logical publication identity

## Threat model

The database administrator, schema administrator, publisher (`oo_target_admin`)
and complete backup/restore operator are trusted. A business-relation owner,
schema CREATE grantee, or Gateway reader cannot mint a publication identity.
Publisher approval does not prove data correctness. In-place DML remains
governed by the ingestion/publisher permission boundary, as with the old binding.

| Attack | Refusal |
| --- | --- |
| Drop/recreate the same name and columns with different data | `sql_drop` tombstones the publisher record; recreation and even restoration of the original shape do not clear it. |
| Rename-swap two tables | DDL invalidation observes the first missing/renamed relation and tombstones the record. The same applies to a view's underlying relations. |
| Redefine/redirect a view | Fixed-search-path `pg_get_viewdef(..., false)` and recursive relation definitions change; the record is invalidated and the query snapshot rechecks the digest. |
| Change column type, typmod, nullability or generated expression | Ordered logical column definitions change. Restoring the original definition still requires publisher approval. |
| Non-publisher creates a lookalike | The record remains invalid; neither identity-table DML nor fence publication is granted to the lookalike owner. Owner names and ACLs are also bound. |
| Truncate published storage | A publisher-installed statement trigger invalidates direct and dependent view identities. Disabling/dropping that trigger changes the identity and invalidates it. |
| Omit/disable DDL guards during restore | Gateway refuses logical binding unless both expected event triggers are enabled. |

Temporary schemas are explicitly searched last; the live run also checks a
temporary `pg_class` lookalike cannot change the fingerprint.

The new fingerprint uses relation/schema names, relkind, owner names, ordered
non-dropped columns, formatted types including typmods, nullability, named
collations, generated/default expressions, sorted ACLs and policies, sorted
relation options, and deparsed views/dependencies. It contains no catalog OIDs
or relfilenodes. Column order follows attnum, but dropped-column gaps are not
persisted (a logical restore compacts those gaps). Builtin function/operator
signatures in the Gateway receipt are symbolic too.

`_oo.publication_identity` binds the publisher's Pack id (`opened_pack_digest`),
release generation (`opened_version`), and relation identity digest. These are
publisher fields, not the principal-scoped `pub1_` HTTP receipt, which is computed
from that record plus existing data/authorization context. Only the publisher
and trusted table owner/DB administrator can write identity content; security
definer invalidation functions can only mark it invalid.

Publication locks the relation/view dependencies before capturing identity.
Queries retain the existing transaction-owned relation locks and snapshot
verification. No read path repairs or invents identity records.

## Upgrade and installation

Apply `crates/oo-pipeline/migrations/0135_logical_publication_identity.sql` as
the trusted database administrator, after `_oo`, `oo_publish_target.fence` and
the publisher role exist. Creating event triggers requires the administrator;
the publisher does not need superuser. It needs its existing relation ownership
or SELECT/TRIGGER privileges on published storage and SELECT on published views.
Grant only SELECT on the new identity table to Gateway metadata readers; native
mapping-reader provisioning now does this without adding write permissions.

There is deliberately no backfill from potentially substituted existing
relations. Re-run the normal reviewed publisher close/open flow. Until then,
logical-binding requests fail with the existing `PUBLICATION_DRIFT` error.
Previously issued OID-derived receipts are refused after upgrading; callers
must discard intermediate results and acquire a new receipt.

Targets without migration 0135 retain the old implementation during rollout.
Once the table or digest function exists, missing records/guards fail closed;
there is no fallback to OID binding on an incomplete logical installation.
Full restores must preserve owners, ACLs, functions, record data and triggers.
The live tests use PostgreSQL 17 on both sides; cross-major deparser equivalence
has not been established.

The migration is not automatically registered: the installer source is outside
this card's write scope. A coordinator-owned installation patch is required
before release acceptance. Apply it only after ensuring publisher-role bootstrap
runs before this installer; the existing installer takes administrator credentials.
The following exact patch passes `git apply --check`; it is not applied,
compiled, or runtime-tested:

```diff
--- a/crates/oo-pipeline/src/bin/install-retained-target.rs
+++ b/crates/oo-pipeline/src/bin/install-retained-target.rs
@@ -108,10 +108,26 @@
             .await
             .map_err(|_| "database")?;
     if installed {
+        install_logical_identity(&mut db).await?;
         return Ok("already_installed");
     }
     install(&mut db).await?;
+    install_logical_identity(&mut db).await?;
     Ok("installed")
+}
+
+async fn install_logical_identity(db: &mut PgConnection) -> Result<(), &'static str> {
+    let installed: bool =
+        sqlx::query_scalar("SELECT to_regclass('_oo.publication_identity') IS NOT NULL")
+            .fetch_one(&mut *db)
+            .await
+            .map_err(|_| "logical_publication")?;
+    if !installed {
+        db.execute(include_str!("../../migrations/0135_logical_publication_identity.sql"))
+            .await
+            .map_err(|_| "logical_publication")?;
+    }
+    Ok(())
 }

 async fn install(db: &mut PgConnection) -> Result<(), &'static str> {
```

Pre-commit checks run against the complete lane working tree; the migration-only
commit does not change Rust code (its clean-base suite passed separately).

No Cargo.lock or dependency changes are required. Existing targets still require
the explicit re-publication step, not an automatic trust upgrade.

## Freshness

The quality reader uses JSON extraction, so older targets need no new columns.
It recognizes `state = stale` or boolean `stale`, with `stale_reason`, `reason`,
or `failure_reason`; absent reasons are explicitly unavailable. Reasons are
bounded and control characters removed. Source/table scope is checked before
emitting `STALE_DATA`. Retained cuts read current stale status separately without
copying pending timestamps or generations into immutable receipt observations.
The same warning projection is used by query responses and `/v1/freshness`.

The lane-P report is not present in this branch. Its final field contract and
end-to-end partial-publication integration still need coordinator verification.

## Validation

- Base: `1f267b6f5b95180139196fbb90c5b2e716d14699`, clean on entry.
- Raw evidence is ignored under `evidence/fh-r-20261107/` and the named live runs.
- Baseline `cargo test --locked -p oo-gateway`: 299 passed, 0 failed, 48 ignored.
- Final `cargo check --workspace --locked`: passed.
- Final `cargo test --locked -p oo-gateway`: 301 passed, 0 failed, 49 ignored.
- `cargo clippy --locked -p oo-gateway --all-targets -- -D warnings`: passed.
- `cargo fmt -p oo-gateway -- --check`: passed. Logs: `*-final5.log`.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`, and
  `check_v02_plan.py`: all passed.
- `python3 tools/oo-test/t9_restore.py evidence/fh-r-t9-09`: final T9 run.
  PostgreSQL 17.11: 30 HTTP assertions passed, plus 1 view-receipt test and
  two non-publisher privilege denials. All owned clusters stopped.
  The runner uses actual authenticated Gateway HTTP responses/receipt headers,
  not CLI exit status as the acceptance oracle.
- `python3 crates/oo-gateway/tests/f22_owned_pg.py evidence/fh-r-f22`:
  existing F22 live Gateway/executor test passed (1/1).
- `CARGO_TARGET_DIR=target python3 crates/oo-gateway/tests/published_cut_owned_pg.py evidence/fh-r-published-cut`:
  existing pending-generation isolation test passed (1/1).
- Other opt-in database tests remain ignored, not passed.

T9 covers a fresh PG17 cluster with changed relation and user-defined type OIDs,
`pg_dump`/`pg_restore`, and a separate `pg_basebackup` restoration. The original
publication id and data-vector hash survive both restores. Table tamper negatives
exercise the real Gateway with both pinned and current receipt requests.
View positives/negatives exercise `receipt_in_snapshot` directly in an owned
PG fixture (three attacks, repair-without-publisher refusals, and explicit
publisher recovery); restored view identity is checked too. This is not a claim
of a live retained-view HTTP restore test or a source ingestion acceptance test.

Evidence SHA-256 (raw files remain ignored):

- `evidence/fh-r-t9-09/result.json`: `a8369c016be45f1c2bd80062192df4275d2d14b33c1312f2e59c7492db146c0c`
- `evidence/fh-r-20261107/test-final5.log`: `92617ffe4714a5ebc05a1927780dcff14f35836850dd92176de1ae0437df6a9f`
- `evidence/fh-r-20261107/clippy-final5.log`: `65612b896acd6cc27c313365dd67224298d4eb72c042f6eca62559611111784a`

Earlier unsuccessful attempts remain intact: T9-01 assumed a body receipt
instead of reading HTTP headers; T9-02 tried a view through a table-only legacy
catalog and was correctly refused; T9-06 failed compilation of the new freshness
test due to an unqualified macro. An initial clippy run found a redundant async
block. All were corrected without changing the expected data or refusal codes.

## Supply-chain review and handoff

Changed files already hash-bound in `supply-chain/secret-review.json`:

- `crates/oo-gateway/src/public_surface.rs`
- `crates/oo-gateway/src/query_executor.rs`

The new PG test and T9 runner read generated runtime credentials from private
fixture files. They contain no literal credentials. The coordinator must review
the changed/new sources and regenerate any required review entries; no
hash-only self-approval patch has been applied.

No containers were created. Owned local clusters and Gateway processes are
stopped by the runners; raw databases/dumps remain only as ignored evidence.
DDL invalidation scans current identity records; large-catalog DDL latency has
not been measured.

Remaining gates: installer wiring, lane-P field-contract integration, independent
secret review, and live native/retained-view HTTP restore acceptance. No production,
customer, source database, cloud, or pre-existing service was used.
