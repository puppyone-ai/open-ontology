# V3-G-01 / D28-T sign-off

Status: partial. Implementation and owned-replica integration run; the
historical relation-recall non-regression exit is not established.

## Resume verification

Run `v3-g-01-resume-20261109` independently repeats the owned PostgreSQL
17.11 integration using the retained logistics fixture. All confusion
matrices below reproduce exactly. `live/manifest.json` has SHA-256
`d13c7b5edfbfa0bd77ed1ffcd243d517fb8c26890deeca893f00acea1a0afd11`;
`d28-result.json` has SHA-256
`8debaee957432211245f071886fc2880308f5b08e18a6f6a1bd62aafb0aa6cd6`.
Both live artifacts pass JSON-schema validation and Rust compilation.
The D28 command again exits 2: the historical baseline is still missing.

The resumed implementation also replays the Rust confirmation compiler before
data-only registry reuse. Tests reject compiled-artifact tampering, stale
confirmation hashes, and a retained cut that changes during collection.
The scorer now rejects duplicate baseline IDs, invalid decisions/classes,
and refuses acceptance when any prediction lacks a gold label.

Fresh commands/results (`python` uses the prior run's ignored test venv):

- `cargo check --workspace --locked`: pass.
- `cargo fmt -p oo-builder --check`: pass.
- `cargo test -p oo-builder --locked`: 193 passed.
- `OO_LIBRARY_BUILDER_BIN=target/debug/oo-builder python -m unittest discover
  -s tools/pg-builder/scripts`: 359 tests, 346 passed, 13 skipped.
- `python -m unittest discover -s tools/oo-test`: 184 passed; seven D28 tests.
- `python tools/oo-test/d28_live.py --output
  evidence/v3-g-01-resume-20261109/live --logistics
  evidence/v3-g-01-20261109/logistics`: exit 0.
- `tools/oo-test/oo-test d28 --manifest
  evidence/v3-g-01-resume-20261109/live/manifest.json --output
  evidence/v3-g-01-resume-20261109/d28-result.json`: exit 2, incomplete.
- pg-builder Rust tests: the same 67 passes and five failures listed below.
  Base proof remains the original run's base archive/logs; Rust formatting
  still has only pre-existing differences.
- `git apply --check tools/pg-builder/V3-G-01-publisher.patch`: pass only;
  patch remains unapplied. No owned containers remain.

The first resumed cut-change unit test failed because its test double omitted
the required receipt hash. Its raw failure is preserved in `retained.log`;
the corrected double uses real observed provenance. No production check or
gold expectation was weakened.

## Original run

Run: `v3-g-01-20261109`, under ignored `evidence/`.
Base: `d941238aa`. No customer/source system or model was contacted.
No migration, Cargo dependency or Cargo.lock changes.

## Fixed live evidence

`live-7` uses one task-owned PostgreSQL **17.11**
(`postgres:17.11-bookworm`, Debian build `17.11-1.pgdg12+2`).
Fixtures are seeded through a separate admin role; product aggregate queries
use a SELECT-only role. Every container created by the harness carries
`oo.lane=g` and `oo.task=V3-G-01`; none remain.

T6: 12 tables from `testing/engines/seed-common/generate.py`, scale 200,
corresponding to `testing/engines/gold`. OO-Logistics: seven tables from
`bench/datagen`, xs, seed 28, generated tree hash
`8035f107b458ebaaecf11c7c470d86051b6ce372ca8f60ab37418e752637176e`.
The evaluator uses fixed positive/negative candidate pools. These numbers
measure adjudication, not end-to-end relation discovery, SQL answering,
all 18 logistics tables, source-engine support, or public publication.

| Dataset | Class | TP / FP / FN / TN | Precision | Recall |
| --- | --- | --- | --- | --- |
| T6 | relation | 7 / 0 / 0 / 1 | 1 | 1 |
| T6 | sensitive | 0 / 0 / 2 / 37 | undefined | 0 |
| T6 | metric | 1 / 0 / 0 / 1 | 1 | 1 |
| T6 | entity_key | 12 / 0 / 0 / 0 | 1 | 1 |
| T6 | code_table | 1 / 0 / 0 / 0 | 1 | 1 |
| OO-Logistics | relation | 3 / 0 / 0 / 1 | 1 | 1 |
| OO-Logistics | sensitive | 0 / 0 / 3 / 65 | undefined | 0 |
| OO-Logistics | metric | 1 / 0 / 0 / 2 | 1 | 1 |
| OO-Logistics | entity_key | 7 / 0 / 0 / 0 | 1 | 1 |
| OO-Logistics | code_table | 1 / 0 / 0 / 1 | 1 | 1 |

The five semantic PII canaries are not phone/ID/email/card-formatted values.
They remain positive gold: the new name-only-negative-profile rule exposes
them. No gold labels or thresholds were changed to conceal this mismatch.
Separate real-format synthetic probes pass all four format detectors and
the negative-name-only case. Bank/ID tests are format checks, not proof of a
real account or identity.

There is no pinned historical per-item human-review baseline in this
worktree. No fixture-generated acceptance list is substituted for one.
The D28 command exits **2**, with `status: incomplete`,
`baseline_status: missing`, and null non-regression results for both sets.
Even perfect recall on these candidate pools is not baseline acceptance.

Fixed SHA-256:

- `live-7/manifest.json`:
  `60437fdb36752f638fa166063ffef89d900ec70e26f677aa14b1980eb4a1df9b`
- `d28-acceptance-final.json`:
  `9da8c3757d5eeb5c59cde86c1d69ad227885d291a0a347af2eae2743692f3a73`

The manifest pins each gold and adjudication file. Both actual artifacts
pass JSON-schema validation and compile through the Rust confirmation
consumer. The default incomplete-code-dependent metric demotes. Raw outputs
of unsuccessful earlier runs are retained; none are represented as passes.

## Reproduction

Use a private ignored virtual environment with
`tools/pg-builder/scripts/requirements-test.txt`, and build the builder binaries.

```sh
cargo build -p oo-builder --bins --locked
cargo run --manifest-path bench/datagen/Cargo.toml --locked -- \
  generate --scale xs --seed 28 --output evidence/RUN/logistics
python tools/oo-test/d28_live.py --output evidence/RUN/live \
  --logistics evidence/RUN/logistics
tools/oo-test/oo-test d28 --manifest evidence/RUN/live/manifest.json \
  --output evidence/RUN/d28-result.json
```

To evaluate the historical baseline, add a hash-pinned `baseline` reference
to each manifest dataset. Its protocol is `oo.d28-human-baseline/v1`, with
`gold_sha256` (canonical gold JSON), `evidence_run_id`, `review_mode: per_item`,
and `adopted_relations` (stable gold IDs). A changed gold, missing prediction,
unknown baseline relation or duplicate ID cannot silently pass.

## Tests and pre-existing failures

- `cargo check --workspace --locked`: pass.
- `cargo fmt -p oo-builder --check`: pass.
- `cargo test -p oo-builder --locked`: 193 passed.
- Python pg-builder suite: 357 tests, 13 pre-existing/environment skips;
  remaining tests pass. Set `OO_LIBRARY_BUILDER_BIN` for the wide-library tests.
- Python oo-test suite: 181 passed, including four D28 scorer tests.
- `cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --tests
  --no-fail-fast`: 67 passed, **five failures**, identical on the base archive
  and lane. All five require absent historical evidence:
  - `document_profile::tests::shipment_catalog_keeps_weight_and_the_nineteen_approved_columns`
  - `source_contract::tests::derived_source_bound_projects_native_notes_without_a_forged_grade`
  - `source_contract::tests::oracle_batch_and_ts_keep_real_caveats_and_omit_grade`
  - `stored_receipts_keep_party_and_account_in_one_pack`
  - `fixture_and_stored_generations_are_mixed_scope`
- pg-builder Rust formatting also fails identically on the base; this lane
  changes its Python code only, and does not reformat unrelated Rust files.

Base proof: `base-pg-all.log`, `base-pg-fmt.log`; lane results: `pg-all.log`,
`pg-fmt.log`. The base was extracted with `git archive` inside this worktree's
ignored evidence directory; no other checkout was touched.

## Integration limits

`tools/pg-builder/V3-G-01-publisher.patch` is the exact coordinator-owned
publisher adaptation requested. `git apply --check` passes; it is unapplied
and untested. V3-A-02 must supply the documented confirmation envelope.
New retained inputs fail closed without it; old unmarked review artifacts
still compile by the historical route.

Unverified demotions are returned as `example_queries`, not falsely
published as OOS `VerifiedExample` records. General model SQL reconciliation,
query-history access on a configured live extension, and the publisher/UI
integration are not acceptance claims of this run. Schema-unqualified or
complex JOINs may have no recognized signal.
