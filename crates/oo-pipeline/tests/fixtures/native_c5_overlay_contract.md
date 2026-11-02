# C5 overlay storage contract

Migration: `0066_native_c5_overlay.sql`, after 0065, as `oo_target_owner`.
0067 and the Rust/Gateway integration are owned by the coordinator.

```sql
_oo.c5_overlay_prepare(
  p_shard text, p_owner uuid, p_fence bigint, p_run uuid,
  p_scope jsonb, p_keys text[], p_column_idents text[], p_key_idents text[]
) RETURNS bigint; -- inherited rows, no parent data scan/copy

_oo.c5_overlay_finish(
  p_shard text, p_owner uuid, p_fence bigint, p_run uuid
) RETURNS bigint; -- inherited + changed-range rows

_oo.c5_overlay_is_valid(p_physical text) RETURNS boolean;
_oo.c5_overlay_row_count(p_physical text) RETURNS bigint;
```

Use one transaction:

1. Existing source-bound admission, fence and range guard.
2. Existing open-run and prepare-shadow.
3. Overlay prepare (also valid for a rootless first range).
4. COPY only the complete replacement range into the new `pt_` heap.
5. `SET CONSTRAINTS ALL IMMEDIATE`.
6. Overlay finish; it renames the heap to private `pd_` and creates `pt_` view.
7. Existing shadow-count, quality, Loader receipt and note-table; logical count
   must equal `inherited + upsert_rows`, not the size of `pd_`.
8. Existing seal-run, then C5 range-commit; commit the transaction.

The prepare/finish calls are same-transaction idempotent, with exact scope/key
matching. Batch replay after sealing remains the existing receipt replay path.
The public validators are SECURITY DEFINER STABLE with a fixed search path;
only boolean/count metadata is exposed. Row count returns NULL on invalid or
unsealed/unreceipted views. Schema USAGE is still required. Structural admission
of open views remains private and cannot be used by the Gateway.

## Flat storage

Every view references a frozen root heap (when present) and at most one private
delta heap per touched stable chunk. It never references a parent public view.
Component metadata is copied; parent rows and existing exclusion keys are not.
Repeated replacement of a chunk reuses its first exclusion-owner ID and changes
only its delta-owner ID. The base exclusion IDs are constants in the view.
The original heap, all old views and sealed receipts remain intact.

The explicit limit is 4096 active components, not 32 generations. Exhaustion
raises 54001 and has no full-copy fallback. No automatic compaction or garbage
collection is implemented. Validation visits the frozen root and actual
components, not a generation chain.

Source watermark is a global CAS; parent physical/column mappings come from
the table's freshness and sealed run generation. 0066 wraps 0065's guard and
commit functions rather than copying old historical guard bodies. The
shared-source regression uses the qualified multipart path; legacy
non-multipart guard semantics remain unchanged.

## Types, keys and source order

Supported keys: PostgreSQL int2/int4/int8, text/varchar and bytea, including
composites. Loader strings use UTF8 octet lengths, U+001F separators and
canonical lower-case `\x` binary hex independent of `bytea_output`.
All comparisons for replacement use typed byte identities, not SQL collation.
NULL/empty text keys, embedded separators in composite loader components,
bpchar and unsupported numeric/date key domains fail
closed; all non-key types must preserve OID, typmod, order and collation.

MySQL ranges are `(lo, hi]`. Integer/binary bounds can be checked locally.
PostgreSQL does not reproduce MySQL text ordering. Bounded text requires the
sealed multipart baseline's key-column authority plus that chunk's verified
original-part hydration receipt. The whole scope must still match the
authenticated source admission in Rust; this SQL is not a source-proof API.
Unproven bounded text is rejected, even if the sample keys happen to be ASCII.
Unbounded text does not require an ordering assumption.

Pending membership is unknown, not empty; the current 0065 guard requires all
active baseline chunks to be hydrated before apply. Legacy single-full baselines initialized before 0066 did not
persist key-column authority; they require an explicit qualified rebaseline.
The wrapped single-full initializer now records that authority.

## Fixtures and limits of evidence

Run `python3 product/crates/oo-pipeline/tests/native_c5_overlay_fixture.py`.
It creates a new socket-only PostgreSQL 17 cluster and `overlay_fixture`
database, accepts no external DSN and installs no packages. Stopped fixture
clusters/logs are retained in the printed temporary directory. `--scale-only`
and `--source-order-only` run independent subsets.

The fixtures exercise production SQL fences/receipts, actual COPY, unchanged
observation, updates, deletes, empty ranges, rollback/refusals, read-only grants,
definition tamper, exact typmods, shared-source table generations, 244 chunks
and 284 generations, and a real 1,000,002-row root. The large synthetic seed
temporarily disables shadow triggers only while generating fixture rows; all
tested apply operations use enabled guards. Parent relation transaction stats
must report zero scans/writes during apply, with only one delta row stored.

Mixed-case/accent/composite tests use synthetic source-ordered multipart
receipts and the real initialization/hydration APIs. They prove the SQL storage
contract, not a live MySQL collation or customer-source end-to-end acceptance.
Publication, original Arrow retrieval/proof and Gateway integration must also
pass their coordinator-owned tests before rollout.
