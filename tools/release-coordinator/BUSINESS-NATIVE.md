# Native publication-bound business execution

This extends the existing private-policy/four-issuer test-scope path; it does
not introduce IAM, change the SQL allowlist, grant access, or touch a source DB.
The v1 ordinary-table fixture path remains available and separately tested.

## Admitted native schema

Policy protocol `oo.release-coordinator/business-policy/v2` replaces
`generation_relation` with `native_mapping`, `statement_timeout_ms` and
`execution_deadline_ms`. There is no synthetic generation relation in this path.
All other candidate/effective/visibility/issuer/oracle/review bindings remain.

The verified scope is a **published MySQL C1 collection** using existing
`oo.pipeline.native-publication-receipt/v1`, `run_ids`, `mysql-gtid/v1`,
transactional consistency grade T and ordinary full-import physical tables.
All scoped target and independent-oracle relations must be in that receipt.
At most eight scoped relations, 16 selected columns/relation and 32 fixed plans;
there is **no row-count cap** or data sampling/courier for native proof binding.

Awaiting-publication runs, stale data generations, changed target generations or
positions, unsupported source codecs, native-library/C5/per-table collection
variants, native overlays, arbitrary views and RLS are refused. Supporting those
variants requires their own validated authority/ACL contracts, not a fallback
table read or a grant to underlying view storage.

## Reused authorities and exact mapping

`target::resolve_native_binding_body` is now crate-visible and is called inside
the same controlled transaction. The existing validated native context,
`_oo.lookup_physical_mapping`, source/config/generation identity, physical
namespace/table and every column mapping remain authoritative. No physical
name supplied in a business request is used directly.

For every scoped table, the resolver compares the retained publication to:

- `_oo.full_import_run`: published state, source/config/source generation,
  target generation and run identity.
- `_oo.full_import_table`: exhausted table, physical identity, accepted rows,
  canonical digest and applied position.
- `_oo.native_publication_receipt`: exact receipt SHA, complete table vector,
  source/target generation, position codec and actual publication transaction.
- `_oo.table_freshness`: live/full, exact source/target generation, physical
  identity and last position.
- `_oo.source_watermark`: live, exact source/target generation, epoch, position,
  lineage and consistency.

The native metadata vector includes those actual bounded rows, mapping identity,
selected column/schema/OID and table/column ACLs, role identity/membership and
validity, plus exact installed migration checksums for mapping and publication
contracts (0004, 0027, 0036, 0082). It preserves existing NULL source-time fields;
published/latest synchronized state is not a claim of source-real-time freshness.

Source watermark/freshness and selected run/table rows are locked `FOR SHARE`
through execution or the existing pointer CAS. The lock is source/run scoped,
not a global data-table lock. The controller needs private metadata SELECT and
row-lock privileges; it needs **no table-level SELECT/UPDATE on business data**.
Physical business rows are not read or hashed while building this vector.

## Execution, columns and resource limits

Only the existing generated logical `SELECT count(column) FROM schema.table` or
`SELECT sum(column) FROM schema.table` plans are admitted. They must exactly
match the candidate metric/example and separately pinned oracle/eval plan.
The plan is rendered to the authoritative quoted physical column/relation.
Logical and actual SQL digests are both retained and rechecked.

Target execution/evaluation uses the sealed target role; the oracle uses its
separately registered login and independent mapped relation. The existing
column-level SELECT must be present. No table-level SELECT or hidden/system
column grant is issued. The target type scope remains builtin integer/numeric.

The controller exports one repeatable-read PostgreSQL snapshot. Executor,
oracle and evaluator import that exact snapshot before reading; their receipts
bind the snapshot and native publication vector. All queries return one bounded
scalar (<=128 bytes), not an array of source rows. COUNT/SUM may scan server-side;
the adapter does not pull a 55M-row table into the process for a digest.

The private policy pins statement/lock timeout (1..60000 ms) and total execution
deadline (1..300000 ms). The existing per-connection connect/metadata transaction
budgets still apply. When the query execution deadline expires, the client sends
PostgreSQL CancelRequest over the existing verified TLS connector, bounds that
cancel wait to two seconds, rolls back/drops the read transaction and emits no
successful receipt. A partial failed run cannot pass full review coverage.

## Same gate path, fresh data-only generation

The immutable issuer ledger, exact bundle review, expected-vs-observed result
checks and test-scope-only gate admission are unchanged. Plan and CAS re-resolve
the current native authority. A real newly published source/target generation
invalidates old evidence even if the semantic candidate and returned COUNT/SUM
are unchanged. No old receipt/policy is rewritten: install the new pinned
mapping/publication policy and execute/review a new run.

Maintain preflight now permits the same-database native path **only** when the
verified business proof's native-mapping digest equals the request mapping.
The existing target resolver/grant/fence mechanisms still apply. This delivery
tests the shared native preflight plus real test-scope plan/CAS; it does not
claim full native maintain target-publication or public Gateway acceptance.

## Owner interface / handoff

No pipeline/Gateway/SDK/semantic files are modified. The minimal payload is:

```text
candidate: current stored effective_oos, visibility, source_hash
native_mapping.datasets[]: dataset, source, config_digest, generation
native_mapping.publication: protocol, run_id, receipt_sha256, receipt
receipt.tables[]: original per-table run/source/target generation and publication fields
```

The original `sealed-collection.json` and `data-publication-v2.json` artifacts
are useful authoritative inputs; do not reconstruct their hashes from a count
query. Installation must expose only the required private metadata read/lock
and mapping-function privileges to the coordinator and retain already-approved
column grants for each executing role. Live integration needs the producer
owner's confirmation of receipt/current-watermark semantics for C1 tail/data-only
updates. Until a new publication agrees with current authority, this adapter
refuses stale input rather than inventing a generation or silently using a tail.

The root `c1onboard_20261027` DB is **not accessed by this delivery**.
No S5, provider call, fabricated source observation or customer business approval
is needed or claimed for this mechanism test.

## Owned fixture and evidence

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/run_business_guard_fixture.py \
  --native --evidence /private/new-release-native-evidence
```

The runner creates its own private TLS PG and four independent issuer logins.
It applies unchanged product jobs/pipeline migrations (through 0082), records
their actual file hashes, and executes actual jobs lease/CAS, C1 staging/typed
comparison, full-import seal, ACK and native publication functions. Anonymous
12,000-row data is generated inside the owned DB; it is not a MySQL source
capture. The publication table is never filled by an invented test receipt.

The test retains exact generated native contexts for two real publication
generations, per-issuer immutable receipts and real CLI execution/review output.
It verifies native positive plan/CAS, zero data-row courier for binding,
one-row aggregates over >1000 rows, column-only SELECT, hidden/system column
refusal, revoked access, unknown same-name view refusal, tampered receipt,
real data-only publication drift before CAS, bounded statement failure, explicit
client-deadline cancellation and no remaining active query.

Old positive and safety-fallback history/pins remain unchanged. Production,
customer signing, full L1-05 and the overall TODO goal are not marked complete.
