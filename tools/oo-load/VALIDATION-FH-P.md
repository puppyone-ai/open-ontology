# FH-P handoff

Status: partial. Neither S6-M-06 nor S6-M-07 satisfies every card exit.
Base: `1f267b6f5b95180139196fbb90c5b2e716d14699`.

## Implemented

- C5 retries a recognized table failure exactly once, then continues with a
  named stale result. Unknown errors, source connection/authentication/probe
  failures, instance-epoch reads and target failures remain global failures.
- A stale table's current retained storage is not replaced. A cycle ledger
  prevents interrupted/global-failure cycles and all-stale cycles from
  becoming partial publications.
- Completed cycle identities replay without rewriting their table outcomes.
  Replaying a superseded retained cut is refused.
- Candidate publication verifies each retained generation independently.
  Fresh, unchanged and stale tables can refer to different immutable storage
  runs. Existing recipe, view, column, membership and semantic-fence checks
  remain in place. No legacy ACKs or semantic approvals are fabricated.
- Stale intervals survive repeated failed cycles and clear after recovery.
  Published health changes at admission, not merely when a new candidate is
  prepared. Existing snapshot receipt identities remain compatible.

## Exact Q/R contract

C5 stdout uses `status: fresh|noop|stale`. Stale lines include `stale_reason`
and the fixed, bounded `stale_detail`:

`Table failed twice; previous retained generation preserved.`

The first failed cycle reports `attempts: 2`. Replays report `replay: true`.
No driver message or row value enters stale detail.

Exit codes: 0 = no stale tables; 3 = healthy plus stale; 4 = all stale;
1 = global failure. Partial stderr has `status: partial`, `error: sync_partial`;
all-stale stderr has `status: failed`, `error: sync_all_stale`.

Publication candidates, isolated-cycle receipts and the persisted release
ledger contain `stale_tables: [{table: {schema, name}, reason, since}]`.
`since` is an RFC3339 UTC timestamp. Each isolated receipt table also carries
`retained_run`, `status`, `stale_reason`, `stale_since`.

Migration 0127 introduces owner-side state:

- `_oo_load.sync_cycles(source, run, scope, state)`: source is the SHA-256 of
  the endpoint binding; state is running/succeeded/partial/failed. An abandoned
  global failure stays running (incomplete), which publication refuses.
- `_oo_load.sync_table_results(source, run, identity, table_ref, retained_run,
  state, reason, since)`: immutable completed-cycle outcomes.
- `_oo_load.table_health(identity, state, reason, since)`: latest cycle health.

Migration 0128 introduces published state for lane R:

- `_oo.table_freshness.sync_state`: fresh/noop/stale.
- `_oo.table_freshness.stale_reason`: nullable text.
- `_oo.table_freshness.stale_since`: nullable timestamptz.
- `_oo_load.gateway_publications.stale_tables`: JSONB array.

The existing `table_freshness.state` remains the admission state (live versus
awaiting_publish), not sync health. `_oo.native_library_retained_table` exposes
the isolated receipt table fields through its existing proof-returning API.
Gateway presentation itself is not changed in this lane.

## Unmet exits and scope blockers

S6-M-06 is incomplete for C1. `PgSync` uses one source scope, lineage and
durable cursor; connector `ChangeSource::next_batch` returns either a whole
source batch or an error without a table identity. Simply filtering failures
and acknowledging that source cursor would lose the skipped table's changes.
C1 was not given such an unsafe retry/skip wrapper.

S6-M-07 is not implemented. Concrete evidence in the base:

- `crates/oo-connectors/src/engine/mod.rs`: `DeclaredSourceColumn` contains only
  name, not_null and comment, not the required column shape.
- `crates/oo-connectors/src/mysql/engine.rs`: `table_shape` returns
  `shape_digest`; its input already reduces columns to mapped target
  oid/typmod, nullability and key membership.
- `crates/oo-connectors/src/oracle/engine.rs`: `table_shape` returns
  `TableMeta::shape()`, a SHA-256 string, not the source column catalog.
  `load_meta` is private; its current query also omits actual ordinal/charset.

A hash cannot prove trailing additions, unpublished-only changes or safe
widening. The card explicitly forbids edits to these connector files. The
existing whole-table equality checks therefore remain intact; they were not
weakened to manufacture passes. No catalog is reconstructed from hashes or
from target types. There is no new sealed-column migration, shared schema
classifier or widening rebuild implementation in this delivery.
Legacy baseline hash initialization is also unchanged: this delivery must not
be described as enforcing the new missing-seal/published-projection policy.

`FH-P-CONNECTOR-API.patch` is the exact, unapplied minimum shared API proposal.
It is a prerequisite, not a complete fix: the coordinator must authorize or
provide native PG/MySQL/Oracle implementations returning actual source
ordinals/types/charset/collation/key positions, then the formal seal,
projection classifier and per-table rebuild can be integrated. C1 also needs
table-scoped capture/recovery semantics before its source cursor can advance
past a rejected table.

The live permission tests revealed another connector boundary: MySQL returns
`catalog/missing_table`, Oracle `catalog/table_not_visible` after permission
revocation. These cannot distinguish removal from loss of visibility. They
are isolated as `schema_unproven`, not guessed to be `source_permission`.
Thus their T9-01 tests intentionally fail the exact named-reason assertion
even though exit 3, exit 4, replay, healthy-table progress and recovery work.

## Validation

Raw evidence is git-ignored under `evidence/fhp-20261107/` and the following
independent run directories:

- `fhp-publication-20261107`: initial real PG17 candidate/admission validation.
- `fhp-publication-final-20261107`: final migration and partial-receipt checks.
- `fhp-t9-{pg17,mysql84,oracle23}-confirm-20261107`: final-binary live C5 checks.
- `fhp-t3t4-{postgres,mysql,oracle}-20261107`: full requested T3/T4 attempts.

The new T4 expectations test both C1 and C5 against the published projection,
including a healthy second table; they no longer count a whole-source
rebaseline refusal as a schema-policy success. `widen_string` is separate
from integer/decimal widening. The new T9 script records unsuccessful and
unimplemented cases explicitly and returns nonzero for them.

T9-02 timeout injection is not implemented. Unpublished-only changes and
nullability changes are not live-covered. There are no schema-classifier
unit tests because that implementation is blocked, not complete.

## Sign-off evidence

All command logs below are under `evidence/fhp-20261107/`:

| Command | Result | Log |
|---|---|---|
| `cargo check --workspace --locked` | pass | `commit-check.log` |
| `cargo test --locked -p oo-pipeline --lib` | 265 pass, 15 ignored | `pipeline-lib-complete-code.log` |
| `cargo test --locked -p oo-pipeline` | 358 pass, 55 ignored | `commit-pipeline-tests.log` |
| `cargo test --locked --manifest-path tools/oo-load/Cargo.toml` | 41 pass, 7 ignored | `load-default-final-contract.log` |
| Same command with `--features mysql-binlog` | 42 pass, 7 ignored | `commit-load-tests.log` |
| `cargo clippy --workspace --locked -- -D warnings` | pass | `commit-clippy.log` |
| `cargo clippy --locked -p oo-connectors --all-features -- -D warnings` | pass | `commit-connectors-clippy.log` |
| Touched-crate fmt and Python compile checks | pass | `commit-format.log` |

`fhp-publication-final-20261107/publication.log`: one real PG17 target test
passed. It exercises actual partial activation with fixture-owner fences,
queries the carried payload, reads published stale proof fields, refuses
all-stale/interrupted cycles, and checks recovery and stale interval
continuity. This is native publication acceptance on synthetic data, not
public Gateway HTTP or customer/business acceptance.

Full T3/T4 runs:

| Engine measured | Passed | Failed | Not run | Not applicable |
|---|---:|---:|---:|---:|
| PostgreSQL 17.11 | 10 | 5 | 0 | 2 |
| MySQL 8.4.7 | 10 | 5 | 0 | 2 |
| Oracle 23.5.0.24.07 | 8 | 6 | 1 | 2 |

The five schema-policy failures on every engine are add_column, drop_column,
rename_column, widen_type and widen_string. These are unmet card exits, not
baseline-failure exemptions. Oracle's extra failure was the resetlogs
fixture's old whole-call error expectation. The updated adapter requires
exit 4, stale/rebaseline reasons, an observed epoch change, an unchanged
target and a successful rebaseline; its real rerun passed in
`fhp-t3-oracle23-resetlogs-confirm-20261107`. Oracle retention expiry was
not injected because the pinned slim image has no RMAN.

Oracle widening was corrected to `NUMBER(10) -> NUMBER(19)`: 10 and 18 both
map to int64, whereas 19 crosses to numeric. The original failed evidence
was retained. `fhp-t4-oracle23-widen-confirm-20261107` re-ran the corrected
C1/C5 assertion and still failed because schema classification/rebuild is
not implemented.

The full T3/T4 runs record their earlier binary SHA, not the frozen final
commit. Final native publication and T9 confirmation runs use the rebuilt
runtime; a complete frozen-SHA matrix remains a coordinator acceptance gate.

T9 confirmations (nine recorded subcases per engine, including separate
drop and rename cases):

| Engine | Passed | Failed | Not run |
|---|---:|---:|---:|
| PostgreSQL 17 | 2 | 6 | 1 |
| MySQL 8.4 | 1 | 7 | 1 |
| Oracle 23 | 1 | 7 | 1 |

T9-11 passed on all three engines. PostgreSQL T9-01 additionally passed the
exact permission reason, same-ID replay, exit 4 for all-stale, unchanged
stale payload and exit 0 recovery. MySQL/Oracle exercised the same isolation
and exit behavior but failed the required permission-reason assertion.
T9-03 through T9-07 remain failures; T9-02 is explicitly not run. Ignored and
not-run tests are not counted as passes.

## Integration notes

No Cargo.lock changes; no changes to existing migration bytes, shared root
manifests, connector implementations, Gateway, CLI, deployment, specs,
execution records or customer systems.

Changed files bound by `supply-chain/secret-review.json`:

- `crates/oo-pipeline/tests/native_library_publication.rs`
- `tools/oo-load/src/main.rs`

Their existing review hashes need coordinator review; this lane does not
self-approve or modify the shared review ledger.
