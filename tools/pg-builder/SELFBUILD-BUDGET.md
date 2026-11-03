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
cannot supply them. The CLI never publishes. `gate_g14` is available to the
coordinator's publication path, but that external publisher is not wired by
this lane. Pack compilation exports a visibility-filtered
`relationships/graph.json` (with JSON pages when needed).
That Pack graph is rendered from reviewed OOS entities/relationships, not
automatically from the new candidate registry. The registry-to-reviewed-OOS
adapter and the authenticated publication hook remain integration work.

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
- The lane-H generator is not present in this checkout; scale tests use a local
  neutral fixture, not that generator's gold output.
- The full standalone Rust adapter suite requires ignored historical MongoDB,
  Oracle and stored-receipt evidence absent from this checkout. Those tests
  remain failures; no synthetic replacement was installed and no guard relaxed.
- Legacy JSON generation is not migrated wholesale to the three-pass NDJSON
  runner. Automatic capability discovery, full preflight cost/time estimates,
  hierarchical domain summaries and publisher integration remain follow-ups.
