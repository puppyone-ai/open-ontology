# D28 replica adjudication

`retained_build.py` now writes `adjudication.json` beside `review-input.json`.
The input pins the retained receipt/data vector. Collection uses the existing
read-only PostgreSQL aggregate reader and `database_containment`; it does not
connect to the original source or execute model SQL. Rechecking the retained
provenance rejects a moving cut. This is replica evidence, not source freshness
or business acceptance.

All default thresholds live in `scripts/adjudication-thresholds.json`.
Column/key profiles are full scans. Relation containment is row-weighted over
non-null child tuples, including composite tuples. Cardinality is measured,
not copied from the proposal. Type compatibility, observed length, names,
direct JOIN equalities in views, and accessible query history are recorded.
Query-history failure/unavailability is explicit; it is not measured zero.
Only direct qualified/aliased equality predicates are recognized, not arbitrary
SQL equivalence. All values stay in the replica; profiles contain counts,
entropy and format rates, never examples.

Entities with invalid keys remain graph datasets with an empty key. Display
selection excludes hidden columns. Metrics certify only when their SQL exactly
matches a supported independent reconciliation definition, units are established,
keys are unique, results are nonempty, and at least two non-overlapping windows
reconcile. Unknown definitions demote; database failures remain failures.
Incomplete/unknown code dependencies demote dependent metrics.

For explicitly proposed scoped code tables, the optional
`adjudication_specs.code_tables` input is a list of
`{id, from: {dataset, column}, to: {dataset, column}, scope: {column: value}}`.
The scope is applied inside the replica; only its hash leaves it.
`adjudication_specs.metrics` maps metric IDs to
`{measure, windows, code_dependencies}`; `measure` uses the existing
`measure_reconciliation.py` contract and dependencies name `code_table/<id>`.
Unsupported SQL is not wrapped and executed as a purported read-only query.

## Relationship discovery and evidence (V3-G-01)

Recall (`discover_candidates`) uses a candidate-key registry per table:
declared PRIMARY KEY, declared UNIQUE (`constraints_sql`), and observed-unique
columns whose type is text or integral and whose name is key-like
(`relation_keys.py`). Amounts, floats, times and booleans are never keys.
Candidates pair only within a type family and a value-shape bucket (length
range overlap, digits, leading zeros) taken from the bound facts profile.
Declared FKs are added; a child text column whose name ends in a parent's
observed-unique non-key text column and names that parent (`region_name ->
regions.name`) becomes a `snapshot_name_mapping` candidate. No table or
column names are hard-coded.

Every candidate is measured by one read-only aggregate (`measure_edge`) on
the same retained cut. Values compare as text, so no numeric cast drops a
leading zero. NULL and blank keys are counted and never matched. Evidence
records row and distinct coverage, unmatched rows and keys, blank rows,
parent duplicates, the maximum parent rows per matched child key (fan-out),
collisions after `lower(btrim())`, and, for a repeating parent, the
date/period/version/valid columns that make it unique (`scope_columns`).

Adoption requires a unique parent, fan-out 1, no normalized collision, and
row and distinct coverage both at the existing thresholds; open-recall
candidates also need a name, declaration or query signal. Rules:
`relation.parent_not_unique`, `relation.scope_required`,
`relation.normalized_collision`, `relation.no_signal`, plus the earlier ones.
Several edges from one child to one parent get a `role` from the child
column; roles are never merged. Entity keys follow the same registry order
and record NULL and blank counts separately (`entity_key.no_candidate` when
nothing qualifies).

`max_relationship_checks` is a resumable boundary. Unmeasured candidates are
listed in the top-level `discovery` object (`status: incomplete`) and
measurements are cached in `adjudication-measurements.json` (bound to the
facts digest and the edge), so a rerun resumes. `discovery` is not hashed;
the compiler re-derives the unchecked set from the input. An incomplete
document compiles only with the owner override
`discovery/relationships: {decision: accept_incomplete, reason}`.

The compiler emits OOS relationship `evidence`, never `many_to_many`, and
`join_hint: inner` only at full row and distinct coverage. The rest goes in
the effective extension `relationship_evidence`
(`oo.builder.relationship-evidence/v1`, carried through the OOS tree as
`evidence/relationships.json`, bound by digest). Role Packs receive
`relationships/evidence.json` and `relationships/query-paths.json` (start
table, grain, role, join keys and type, coverage, unmatched handling,
withheld reasons, and a SQL example that aggregates the child before the
join), trimmed to the role's visible columns.

Data-only reuse (`oo.library.data-only-semantics/v2`) binds key quality, and
retained/scheduled reuse re-measures every adopted relationship of a
confirmed adjudication (`relationship_quality_drift`): a change in
uniqueness, fan-out, collisions or coverage class forces review; row growth
alone does not.

Live acceptance: `OO_RELATIONSHIP_LIVE=1 python3 -m unittest
test_relationship_live` (throwaway labelled `postgres:17.11-bookworm` on
loopback, removed afterwards).

## Confirmation integration for V3-A-02

The JSON schema is `crates/oo-builder/tests/adjudication.schema.json`.
The graph is always present, including rejected/dashed relations.

1. Keep the exact `review-input.json` bytes. `input_sha256` hashes those bytes,
   not a parsed/reserialized object.
2. Write an `oo.adjudication-override/v1` object, even when `items` is empty.
   Its `summary_sha256` binds the original adjudication.
3. Call `oo_builder::adjudication::apply_adjudication_overrides`. Overrides
   update the graph and propagate incomplete code dependencies before hashing.
4. Obtain one explicit `oo.publish-confirm/v1` confirmation. Its summary hash
   binds the post-override artifact; `overrides_sha256` hashes the sorted-key
   UTF-8 JSON override object. `confirmed_at` is a UTC timestamp in
   `YYYY-MM-DDTHH:MM:SSZ` form.
5. Pass this envelope to `compile_reviewed_community`:

```json
{
  "adjudication_input": "<exact input file contents>",
  "adjudication": {},
  "overrides": {},
  "confirmation": {}
}
```

The objects shown empty above must contain their complete protocol documents.
The `selfbuild-budget` bridge accepts
`{"operation":"reviewed","command":<envelope>}` on stdin.
The result retains `oo.builder.reviewed-community-result/v1` and includes the
applied summary and filtered visibility. New retained inputs carrying
`adjudication_replica` cannot fall back to per-item review. Historical inputs
without that marker retain the legacy conversion.
Data-only reuse of a confirmed registry replays the same compiler gate and
compares the compiled artifact before handing off to the publisher.

Demoted metrics appear in `example_queries` with `status: unverified`.
They are not falsely converted into OOS `VerifiedExample` records or published
as certified metrics. Delivering unverified examples in the role pack requires
a separate OOS representation; this lane does not weaken that shared schema.

The compiler does not authenticate the confirmation, grant access, or publish.
The publisher still requires its coordinator-owned D28 adapter.
`V3-G-01-publisher.patch` contains the exact proposed patch; it is not applied
or runtime-tested here. The outer reviewed registry uses the confirmation as
`review_decision`, the envelope as `command`, and the replayed result as
`compiled`. Existing publisher authentication and retained-cut checks stay in
place.

## Tests

Install the test-only dependency from `scripts/requirements-test.txt` in an
ignored virtual environment. Run:

```sh
cargo check --workspace --locked
cargo test -p oo-builder --locked
python -m unittest discover -s tools/pg-builder/scripts
python -m unittest discover -s tools/oo-test -p test_d28.py
```

Set `OO_LIBRARY_BUILDER_BIN` to the built `oo-builder` binary for the existing
wide-library Python integration tests. The schema test uses Draft 2020-12
validation and rejects missing graphs, invalid class decisions and row counts.
See `tools/oo-test/V3-G-01-VALIDATION.md` for fixed live evidence and limitations.
