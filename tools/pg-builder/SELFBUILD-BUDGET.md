# Community self-build (candidate-only)

The shared implementation is `oo_builder::budget` / `oo_builder::community`.
`selfbuild-budget` is the JSON stdin bridge used by
`scripts/selfbuild_budget.py`. No Python token-allocation formula is maintained.
The legacy JSON S5 adapter also uses the bridge for allocation, but refuses a
batch that needs NDJSON column shards rather than losing columns.

## Run

```sh
cargo build -p oo-builder --bin selfbuild-budget --locked
python3 tools/pg-builder/scripts/selfbuild_budget.py \
  --input evidence/local/input.json --config evidence/local/config.json \
  --output evidence/local/draft
# After reviewing the preflight, append --execute to invoke the model.
# Reuse the output directory to reuse validated per-batch receipts.
# For an incremental cycle, pass --resume evidence/previous/result.json.
```

Input is a JSON object with `tables` and `relationship_candidates`. Tables use
logical `dataset` names, `columns` with `name`/`data_type`, and `primary_key`
arrays. Relationship candidates use `from`/`to` objects with `dataset` and
`columns`, `provenance` (`declared_fk`, `containment`, or `naming`), and numeric
confidence. This is a normalized semantic input, not a database connection or
an arbitrary native catalog protocol.

Configuration includes `model`, HTTPS `endpoint`, and `secret_ref` (a local
credential reference, never the credential). Capabilities are explicit inputs:
`window`, `max_output`, `system_overhead`, `json_mode`, `usage`, and `probed`.
Absent capabilities use 32k/4k and 2.5k injected system overhead; preflight marks
them unprobed. The legacy JSON adapter retains its historical route limits;
new routes should always supply a capability probe/override. No automatic
maximum-context probing or provider price lookup is performed.
For credential-free dispatch, set `transport: "model-egress"` and a literal
HTTP loopback endpoint. Supply the egress `oo.model-capabilities/v1` result as
`capability_probe`; the Rust bridge parses it for both language paths.
Overrides may lower limits, not enlarge a probed window/output limit or reduce
measured overhead. Legacy Rust generation replays usage calibration on cache
hits. The legacy JSON format remains supported; it does not silently discard
columns to satisfy an output limit.

The runner re-budgets the actual registry/instructions/repair context before
dispatch. Table and column IDs are framed physical-name hashes. Missing or
invalid NDJSON items get at most three attempts with bounded feedback. Partial
receipts survive interruption or quota refusal. Resume replays usage calibration
as well as candidate validation; another model/template/input cannot reuse a
receipt. Quota failures stop new dispatch; already in-flight calls may finish.
Concurrency starts at one, reaches at most two after complete calls, and returns
to one on incomplete results.

Graph planning folds structurally identical dated/backup/audit suffix families,
uses weighted Louvain communities, and adds compact hub signatures. The model
runs in-community, cut-edge, then merge passes. Registry candidates and folded
inheritance remain reviewable, never certified. Schema dependency snapshots
exclude observation counts and generations. Data-only replay makes zero model
calls; schema changes schedule affected communities, cut edges and merge items.

`result.json` contains the registry, per-item candidates, unresolved IDs,
semantic snapshot, work list and connectivity review. Review decisions must be
provided independently and bind the returned `summary_sha256`; model responses
cannot supply them. The CLI never publishes. `gate_g14` is enforced by
reviewed-community conversion and the Gateway publisher requires the bound
review envelope whenever a producer carries a community-review marker.
Pack compilation exports a visibility-filtered
`relationships/graph.json` (with JSON pages when needed).
That Pack graph is rendered from reviewed OOS entities/relationships, not
automatically from the new candidate registry.

The `reviewed` bridge operation accepts an
`oo.builder.reviewed-community/v1` command: candidate `draft`, existing reviewed
`effective`, `visibility`, `ceilings`, `orphan_threshold`, independent `review`
and the existing `classification_review`. Review binds draft/effective/summary
hashes, reviewer/date/mode, explicit entity key/display selections and G14
decisions. Conversion recomputes connectivity and compiles a role Pack;
it does not authenticate the reviewer, grant access, or publish.
Existing nonempty curated entities/relationships require reconciliation and
are refused rather than overwritten.

The `library_policy.rs` hook recompiles the supplied `community_review` command
and compares the effective document, visibility and producer before admission.
The envelope's ordinary `review` remains the separate classification review;
existing publisher classification/authentication/receipt checks still apply.
Missing, false or null markers cannot substitute for a valid review. A
`community_review_sha256` marker without the required envelope also fails closed.

## Hierarchical navigation and preflight

Pack README and INDEX now link `summaries/index.json`. Deterministic extractive
summaries flow from visible columns to table cards, visible graph communities
and schema-domain groups. Upper levels consume child summaries, not raw facts.
These are navigation groups, not inferred business domains or new certification.
Unapproved descriptions follow the existing `include_unreviewed` policy and
remain labelled. Hidden columns/tables are removed before repartitioning.
Each level is paginated: follow every `next` link and filter children by the
parent's `select` fields. Three-title previews do not replace complete children.

Both dry-run and `--execute` write and print `preflight.json` before generation.
The three-pass structural dry-run includes tables, columns, entities, internal
relations, cut edges and merge work. Future semantic entity text is unknown:
structural placeholders estimate its size, not a guaranteed upper bound.
Every actual model dispatch prints and retains a refreshed estimate after
registry growth, calibration or repair. Cache-only/data-only cycles make no
model calls.

Optional configuration (illustrative rates, not provider prices):

```json
{
  "price_table": {
    "example-model": {
      "currency": "USD",
      "input_per_million": 2,
      "output_per_million": 8
    }
  },
  "seconds_per_batch": [10, 30]
}
```

The exact model ID selects the configured rate. Missing rates are explicitly
`unknown`, never free. Estimated input includes configured system overhead;
output uses the Rust output reserve. The cost range spans estimated input-only
to input plus reserved output. It excludes repairs and may overstate cached work.
Duration uses batch waves at the runner's current adaptive concurrency (1 or 2)
and the configured latency range. Missing latency is `unknown`; the separately
reported deadline is a timeout budget, not a throughput estimate. No price lookup
or paid capability probe is performed by preflight.

## Retained evidence and remaining acceptance

Run ID: `g-s2s3-20261104`. This section records the pre-integration run.
Raw files are ignored under `evidence/`. Continuation run
`g-commit-cont-20261104` reproduced the five historical-evidence failures on
base `2bca1de16`; under task-card rule 9 they do not block commits.

- Rust builder tests and portable Python tests exercise 300-column coverage,
  660-table three-pass coverage, 1,000-table graph bounds, missing-item repair,
  quota stopping, model-bound receipts, calibrated resume and data-only cycles.
- The live two-table test completed two model calls with no unresolved items;
  an entity conflict correctly left G14 blocked pending independent review.
- The live 660-table attempt was interrupted to bound usage while acceptance
  prerequisites were unavailable. Retained partial receipts are not a T5 pass.
- At that run the lane-H generator was absent; scale tests used a local
  neutral fixture, not that generator's gold output.
- The full standalone Rust adapter suite requires ignored historical MongoDB,
  Oracle and stored-receipt evidence absent from this checkout. Those tests
  remain failures; no synthetic replacement was installed and no guard relaxed.
- At that run legacy JSON generation was not migrated wholesale to the three-pass NDJSON
  runner. Automatic capability discovery, full preflight cost/time estimates,
  hierarchical domain summaries and publisher integration remain follow-ups.

## Continuation acceptance

Run ID: `g-commit-cont-20261104`. The seven retained commits were rebased onto
`v0.2/product-loops` before integrating egress and lane H's generator.

```sh
python3 tools/pg-builder/scripts/live_selfbuild_acceptance.py \
  --config evidence/local/stack.json --output evidence/local/t4-t5 \
  --tables 660 --retained-receipts evidence/previous/draft/receipts --execute
# Small connected-table T4 run; reuses the completed initial semantic snapshot.
python3 tools/pg-builder/scripts/live_selfbuild_acceptance.py \
  --config evidence/local/stack.json --output evidence/local/t4-connected \
  --tables 660 --base-result evidence/local/t4-t5/draft/result.json \
  --connected-mutations --execute
```

The harness imports oo-test's fixture runner/generator unchanged, substitutes
lane `g` labels for its own resources, and refuses cleanup on either lane or
run-id mismatch. Sources use the read-only fixture principal; only the separate
admin seeds and applies T4 mutations. Listeners are ephemeral and closed.

- PostgreSQL **17.10**, native linux/arm64, pinned image. `--tables 660`
  creates 660 small tables plus the fixed fixture: **694 tables/views and
  1,713 columns**, including all 300 columns of `wide_rows`.
- T5: full table/column coverage, zero unresolved items, **7/7 declared FKs
  mapped**. The first attempt failed on three hyphenated cardinalities.
  Actionable enum feedback fixed repair without accepting invalid values.
  The resumed run reused **19 receipts**, made **4 new calls**, and passed.
  Original failures remain in `live-t4-t5`; success is `live-resumed`.
- Metered connected T4 (`live-connected-t4`): data-only **0 calls**;
  add column and type change **5 calls each**, including repairs, each scoped
  to **1 community / 2 cut edges / 1 merge item**; new table **2 calls**,
  **1 community / 0 cut edges / 1 merge item**. Initial snapshot reuse made
  zero calls. An earlier isolated-table T4 also passed.
- Probe: **272,000 window / 128,000 output / 2,490 minimal-prompt tokens**.
  These are provider metadata and measured minimal-prompt usage, not boundary
  testing or an isolated measurement of injection.
- Final-code replay uses copied receipts, makes zero provider calls and matches
  accepted candidates, registry, work, snapshot and review exactly.
- G14 is **not approved**: live T5 reports 16 components and an orphan ratio
  of about 0.652. No reviewer decisions or authentication were fabricated.
  Local conversion/Pack tests use explicitly synthetic reviews.
- Fixture row scale is 1,000; this is schema T4/T5, not row-throughput or
  cross-engine acceptance. No live 1,000-table run was started.
- No lane-owned containers/volumes or listeners remain. No lane-authored
  Cargo.lock changes. Shared publisher integration is still coordinator work.

## G-S3b acceptance

Run `g-s3b-20261104` supersedes the publisher, hierarchy and preflight gaps
above. The lane rebased on `v0.2/product-loops` (already current); earlier
S2/S3 commits remain intact. The publisher proposal was applied and removed.
The SHA-indexed summary is `tests/selfbuild-s3b-evidence.json`.

- Workspace check and all 182 builder tests pass. Gateway: 283 pass,
  47 ignored (these are not passes).
- Fourteen Python acceptance tests pass, including 660-table coverage,
  300-column repair, 1,000-table graph planning, dry-run without provider
  configuration, configured/unknown pricing and duration, before-call output,
  current concurrency and zero-call data-only replay.
- One synthetic single-table live run through an ephemeral model-egress:
  two streamed calls, three candidates, zero unresolved; measured 4,832
  prompt and 276 completion tokens. No live price or latency assumption was
  supplied: both remain unknown in the initial preflight.
- Standalone pg-builder: 43 library tests pass and three require absent
  historical evidence; contract 15 pass; combined seven pass and two require
  absent historical receipts. All five failures reproduce on archived base
  `48e19f035`. No historical inputs or gold assertions were replaced.
- The broader combined suite exposed bare unreviewed prose in the initial
  summary implementation. The inline warning is now retained, with a
  regression test; the original failing test passes unchanged.
- No database engine was started or retested in this card. The earlier
  PostgreSQL 17.10 T4/T5 evidence remains historical, not a new run. No public
  publication, owner approval or business acceptance is claimed. The owned
  ephemeral listener closed; no containers were created. No lockfiles changed.
