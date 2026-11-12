# V3-I-11: exact numeric scoring and reviewed lookup bindings

STATUS: **complete; ERP mean paired improvement +16.94%, target met**.
Measured November 12, 2026. This is synthetic-fixture acceptance, not
customer or production acceptance.

Evidence:

- `evidence/v3-i11-20261112/`: plan, case audit, regression logs, runtime
  build, capture drivers, replay verification and cleanup.
- `evidence/v3-i11-erp3-20261112/`: installed MySQL ERP publication and
  three fresh paired captures.
- `evidence/v3-i11-qs-20261112/`: installed demo and QS-01..05.

## Numeric contract and case diagnosis

All 24 historical C3 numeric-type failures have exactly equal values:
lg25 and lg32..lg38, on all three seeds. The same audit recovers 29 C0
failures. `numeric-cases.json` retains each seed, arm, question, original
typed result and gold. Requested order and duplicate multiplicity are checked;
the diagnosis does not sort away required ordering.

The frozen result-comparator contract requires identical column types.
Its decimal equality is exact rescaling, not floating-point tolerance.
The new manifest option `numeric_comparison: exact-decimal-v1` explicitly
normalizes both sides before calling that unchanged primitive. It validates
the original typed cells first, represents int64 as scale-zero decimal, and
canonicalizes decimal coefficients/scales, including zero. Bool and text are
never numeric aliases. NULL, missing, unavailable, row multiplicity,
projection width, requested order and completeness remain distinct.

The default `typed-v1` preserves historical scoring. The option is bound
into the recording manifest and reported by the scorer. Gold preflight
remains typed; questions, gold SQL and values are unchanged.

This belongs in the value-comparison policy: these questions request numeric
values, not a particular PostgreSQL aggregate return type. There is no reviewed
business output-type contract justifying blanket bigint casts. Such casts
would also risk overflow. No money value is rounded or compared as a float.

## Refusals and compiled guidance

Historical responses expose only `{"refusal":true}`, not a reason. The I-10
controlled ablation supports wording as a factor, but neither that experiment
nor these responses identifies a causal sentence for each refusal.

The potentially misleading text includes empty certified-metric collections
described as readiness gaps and candidate warnings against treating an
unreviewed definition as a complete business answer. Certification warnings
remain. New compiled guidance separates access from assurance: visible inputs
are usable for explicitly scoped calculations with caveats; missing assurance,
demotion or stale data alone does not require refusal. Missing meanings must
not be invented, and hidden data or Gateway denials must not be bypassed.

An offline recompilation of the identical I-10 reviewed input changes only
README.md and its derived manifest. Effective facts and visibility are
identical (`same-input-guidance-diff.json`). This is not a fresh model ablation.
The measured campaign combines guidance changes with the separately disclosed
reviewed bindings below; its gains are not allocated to individual sentences.

## Reviewed dictionary bindings

The retained builder accepts a hash-pinned definition file, not inferred
dictionary meanings. Definitions bind concept, discriminator/value, code and
label columns, interval convention and provenance. A graph digest binds the
definition list into confirmation, so changing meanings invalidates the old
confirmation even when graph edges and decisions remain unchanged.

Replica measurements check scoped interval validity, overlaps, historical
gaps and, when a fact/time binding exists, reference coverage and ambiguity.
Ambiguous or invalid intervals withhold the binding, including after decision
overrides. Lookup-only bindings explicitly do not claim fact-reference
coverage. Both half-open intervals and inclusive date intervals are supported.
NULL ends are unbounded; inclusive date intervals require date-typed bounds.

The Pack exports `dictionaries.md` only when every dependency is visible.
RLS or denied global statistics suppress aggregate evidence. Application role
codes remain data, never grants. Adding/removing definitions disables the
data-only reuse shortcut and requires fresh review.

The synthetic input maps all four dictionaries in the public generator's
catalog, not just the evaluated questions. It contains no answer labels,
question IDs or gold SQL. Its fixture-curation provenance is not independent
human or customer approval. Independent fixtures test repeated codes,
historical labels, both boundary conventions, gaps, overlapping intervals,
missing references, hidden columns and confirmation drift.

**lg02 and lg08 both pass in C3 on seeds 42, 7 and 2026.**

## Fixed measurement and results

The pre-capture plan fixes source seed 42, model seeds 42/7/2026,
`gpt-6.1-sol`, full-artifact context and exact-decimal comparison for both arms.
C0 remains table/column names only. All 252 ERP and 30 demo model requests are
fresh; no historical C0 or C3 responses are reused. All 282 captured responses
return the requested model identity and `finish_reason: stop`, without error
envelopes. No-output counts are zero.

All ten source CSVs match the frozen bank's hashes. Questions, gold, roles,
demo seed and Cargo.lock match I-10. Only compiler-derived demo artifacts
are regenerated. Authenticated, publication-pinned knowledge reads match the
published ERP Pack, which also equals native recompilation of the exact
reviewed command. All 40 unchanged typed gold preflights pass.

| Dataset | Seed | C0 | C3 | Paired relative change |
| --- | ---: | ---: | ---: | ---: |
| ERP | 42 | 31/40 | 36/40 | +16.13% |
| ERP | 7 | 28/40 | 35/40 | +25.00% |
| ERP | 2026 | 31/40 | 34/40 | +9.68% |
| Demo | 42 | 2/2 | 2/2 | 0% |
| Demo | 7 | 2/2 | 2/2 | 0% |
| Demo | 2026 | 2/2 | 2/2 | 0% |

ERP arithmetic mean paired relative change is **+16.93548%** (21/124).
C3 >= C0 on every seed in both datasets. The +15% mean target applies to ERP;
demo retains its already-perfect baseline and is not pooled into ERP.
All six reports equal their saved replays byte for byte.

For comparison, the I-10 typed-policy mean was +8.60%. Rescoring those same
historical captures under exact-decimal comparison gives C0/C3 counts
29/30, 29/28 and 30/30: **0.00% mean paired change**. This is a diagnostic
rescore, not a new measurement or a rewritten historical report. Numeric
normalization alone does not establish an ontology gain.

Disjoint accuracy outcomes, 120 trials per arm:

| Outcome | I-10 C0 typed | I-10 C3 typed | I-11 C0 exact | I-11 C3 exact |
| --- | ---: | ---: | ---: | ---: |
| Pass | 59 | 64 | 90 | 105 |
| Model refusal | 1 | 28 | 0 | 12 |
| Equal values, numeric-type mismatch | 29 | 24 | 0 | 0 |
| Different values/order | 9 | 3 | 9 | 3 |
| FUNCTION_NOT_ALLOWED | 15 | 0 | 16 | 0 |
| STATEMENT_NOT_ALLOWED | 6 | 0 | 5 | 0 |
| OVERLOADED | 0 | 1 | 0 | 0 |
| No output | 1 | 0 | 0 | 0 |

Residual C3 refusals: lg16/lg26/lg27 on all seeds, lg10 on two seeds,
lg09 on one. All three value failures are lg39. They remain failures.

Security tracks are separate. ERP C3 permission/injection scores are 1/1
on every seed; C0 injection is also 1/1. C0's seed-42 permission trial fails
with `STATEMENT_NOT_ALLOWED` for a lateral JSON query; it is not counted as
a permission denial, and no rows were returned. Other ERP C0 permission
trials pass. Demo permission/injection scores are 2/2 and 1/1 for both arms
on every seed. Model refusals alone do not prove Gateway enforcement.

## Regression and installed acceptance

- `cargo check --workspace --locked`: pass.
- `cargo fmt -p oo-builder -p oo-cli -- --check`: pass.
- `cargo test --locked -p oo-builder -p oo-cli -p oo-eval`: 207, 190 and
  two passed respectively; three CLI tests remain ignored.
- `python3 -m unittest discover -s benchmarks/ab -p 'test_*.py'`:
  39 passed, including exactness, i64 limits, money precision, order,
  duplicate rows, presence states and invalid-input rejection.
- Configured pg-builder Python discovery: 392 tests, 15 skipped.
  Uses the isolated evidence venv with jsonschema and
  `OO_LIBRARY_BUILDER_BIN=target/debug/oo-builder`.
- oo-test Python discovery: 186 tests, one skipped.
- Spec, plan and generated support-matrix checks: pass.
- Six independent live dictionary probes: pass on PostgreSQL 17.11,
  including the production SELECT-only adapter's query form.

Release-coordinator has 104 passing tests and 12 ignored tests.
`live::private_postgres_activates_only_a_test_scope_candidate` fails because
`OO_A359_PG_PORT` is absent. Running that exact test on base `1ba328ee7`
produces the identical error; `base-failure-proof.json` and both raw logs
retain the proof. No test is weakened or marked passed.

Installed ERP: MySQL 8.4.7 source, PostgreSQL 17.10 serving, native macOS
arm64 client and Linux arm64 runtime. Warm install-to-first-answer is
209.841 seconds. Installed demo uses PostgreSQL 17.10.

`python3 evidence/v3-i11-20261112/demo.py` runs the installed-package harness
and fresh demo captures. **QS-01..05 local harness checks all pass**, with
warm install-to-first-answer 14.162 seconds. The denied example has independent
audit readback; 120 source versus 60 analyst orders proves the fixture RLS.
All three generated MCP configurations complete readiness, query and
answerability. Before/after Docker inventories match for this run.

## Retained failures, recovery and boundaries

The first setup rejects the unsupported lane name before publication.
The next installed attempt exposes the leading-CTE admission bug; commit
`0eb6e10a3` wraps it in SELECT without changing the read-only guard.
The third setup publishes successfully, but its verification helper
incorrectly applies the evaluator's 1 MiB input cap to the larger operator
review archive. The corrected helper uses the existing bounded operator
archive scope; product/evaluation budgets are not loosened.

No A/B trial ran in those failed setup attempts. All failures and nonzero
Builder budget/resume exits remain recorded. Recovery restores only the
owned serving databases and Gateway from the same retained volumes, with
the original publication/data-vector binding. Source/model secret mounts
are omitted from serving services; the source database is not restarted.
The same Pack is verified before fresh captures. Serving processes stop
between seeds. At most one lite stack and one source database run at a time.

Cleanup verifies zero owned containers, volumes and networks. Only exact
owned projects/resources are removed; lanes m/h/t are not touched. Labeled
runtime images and ignored evidence remain. No shared-file patch is requested,
and Cargo.lock is unchanged.

Remaining limits: 12 C3 refusals and three value failures; no independent
held-out model campaign, cold download/three-platform install measurement,
real desktop-agent session or customer acceptance. Provider seed requests
do not establish determinism. `source_time=unmeasured` and per-table,
non-atomic source consistency remain explicit.

## Evidence digests

SHA-256, relative to `evidence/v3-i11-20261112/` unless noted:

| Artifact | SHA-256 |
| --- | --- |
| measurement-final.json | 47f86c36d3132320837f39d6c500bd311ef6a32ffcfeb0c00ff48297dcba985e |
| numeric-cases.json | 54e3761976d188788ee52736215d151c8fe87a72785ba1853d088f5d763099d3 |
| capture-audit.json | 9abfc054f4d213f864f6181c88d5c3a0a63297e078b2869306520b4a1152759e |
| final-cleanup.json | cc133e3b377be4779f015e4444c73c48b2282c2f9cfc3948dd7e14862ccd2699 |
| ../v3-i11-qs-20261112/result.json | db47818e57c8166b16c4fe61906f163328ec1b40003b513ee1f264aa74a0360f |
