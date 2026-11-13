# V3-I-08 analytical admission and ERP A/B

STATUS: **complete for this card's implementation, local evaluation and regression**.
Date: 2026-11-11. Model accuracy is not perfect; remaining regressions are below.
This is not production/customer acceptance or a three-platform release claim.

## Commits

- `ff76478e7` (prior run): bounded temporal comparisons, BETWEEN and IN.
- `2bd157ed0`: verified analytical scalar functions.
- `5d79b93fa`: snapshot-bound scalar cast grants and parser wire v3.
- `57b560a25`: SELECT scopes, clocks, UNION ALL and frozen admission corpora.
- `785552b68`: safe public labels for anonymous scalar results.
- `80137e0b8`: count accuracy refusals as failures instead of aborting scoring.
- `4186284f8`: verified numeric defaults and Cartesian row-cost admission.
- `109004a40`: structural retained-column semantics and Pack guidance.

No Cargo.lock change, requested shared patch, source-data/gold alteration,
push, PR, nested agent, customer access or modification to the pre-existing
untracked dispatch script. Compiler-derived snapshots and demo Pack hashes
were regenerated; demo source, roles, reviewed inputs and gold stayed
byte-identical (`demo-regeneration.json`).

## Admission exits

`docs/reference/sql-grammar.md` describes the implemented signatures and
limits. Scalar casts use verified builtin type I/O and pg_cast grants; user
types, arrays and typmods remain refused. Numeric defaults such as
`COALESCE(SUM(amount), 0)` use verified widening conversions. NULLIF retains
PostgreSQL's operator-selected left result type.

Scalar/IN/EXISTS subqueries, qualified correlations, nonrecursive CTEs and
named derived relations bind all nested base relations to the same role
snapshot. Hidden arguments are checked even in unused CTEs or branches.
Same-type UNION ALL supports T6 shard counts. Each nested SELECT keeps its
cost check; oversized unfiltered Cartesian products are refused even for
COUNT or explicit projections. This is a row-estimate gate, not an EXPLAIN
cost model. Execution deadlines remain a separate bound.

`now()`/`current_date` use the transaction clock and session timezone; neither
establishes source freshness. Literal temporal inputs retain the ISO rules.
The public DTO maps only PostgreSQL's anonymous `?column?` result marker to
`value`, without broadening identifier validation.

| Evidence | Command / result | Run-id |
| --- | --- | --- |
| Frozen positive corpus | `cargo test -p oo-gateway --test day_sql frozen_erp_t6_and_demo_gold_admission_corpus --locked -- --exact --nocapture`: ERP owner 40/40, T6 20/20, demo analyst and ops each 3 positives plus 1 denial | `v3-i08b-scopes-20261111` |
| Final live gold | All 40 original ERP gold queries match typed expected rows; no refusals/drift | `v3-i08b-own2-20261111/gold-preflight` |
| Negative corpus | Writes, DDL, catalog, set_config, dblink, large objects, nested hidden columns and oversized Cartesian products stay refused | `v3-i08b-cost-20261111` |
| Real SQL oracle | `python3 crates/oo-gateway/tests/sql_capabilities_owned_pg.py --evidence-dir evidence/v3-i08b-final-pg-20261111`: 372 original/rewritten row, type and column-name comparisons; 128 recorded refusals; seven catalog-mutation refusals | `v3-i08b-final-pg-20261111` |
| Real Gateway refusals | Eleven probes pass their exact closed codes and carry query IDs, including `COST_TOO_HIGH` for a six-way Cartesian aggregate | `v3-i08b-own2-20261111/live-negative` |

T6 source SQL uses search_path; its corpus adapter only binds relation names
to the deployed `app` namespace, retaining predicates, values and gold.
The owned PostgreSQL **17.11** oracle verifies TLS, read-only execution,
non-elevated reader flags and no fixture write grants. Nested filtered-view
queries preserve their role-specific rows. This is separate from HTTP
publication and from the installed demo's RLS/audited-denial checks.

## Installed publication and image

Final own-source run: `v3-i08b-own2-20261111`.
**MySQL 8.4.7 -> PostgreSQL 17.10**, original public xs/seed-42 ERP CSVs,
separate admin seeding and SELECT/SHOW VIEW TLS reader. Fresh private npm
installation, real postinstall, empty HOME, ten selected tables, bounded
Self-build/resume, adjudication, graph confirmation, publication, profile,
readiness and queries pass. Install to first answer: **210.941 s**.
The intermediate build exits of 1 are retained budget/confirmation pauses,
not relabelled successful invocations.

Official `deploy/lite/Dockerfile`, linux/arm64, release with assertions and
overflow checks, label `oo.lane=v03`:

- Tag: `open-ontology/lite:v03-i08b-accepted`
- Digest: `sha256:68011b9d0cbab8de7fdd1528822cdd33126b90581530f7739752133a63508359`
- Product content bound to `109004a40` and explicit source-file hashes in
  `v3-i08b-final-20261111/image.json`. Build context preceded the commit;
  the hashes bind the committed product files, not a claim of a clean VCS
  build invocation. A cancelled build before demo-asset refresh is retained.

Native installed CLI measurements use debug darwin-arm64 binaries and warm
local images, not cold downloads, release-native percentiles or other hosts.
Model files were referenced in place by path only. `source_time=unmeasured`
and per-table, non-atomic source consistency remain explicit.

## Live A/B and byte-identical replay

Both ERP rounds and demo use `--arms c0,c3`, seed **42**, requested model
**gpt-6.1-sol**, unchanged questions/gold, publication-pinned Gateway results,
and the deterministic typed comparator. There is no LLM judge.

| Run | Accuracy C0 | Accuracy C3 | Relative change | Permission C0/C3 | Injection C0/C3 |
| --- | ---: | ---: | ---: | --- | --- |
| ERP 1 | 19/40 | 18/40 | -5.26% | 1/1 / 1/1 | 1/1 / 1/1 |
| ERP 2 | 19/40 | 24/40 | +26.32% | 0/1 / 1/1 | 1/1 / 1/1 |
| Demo | 2/2 | 2/2 | 0% | 2/2 / 2/2 | 1/1 / 1/1 |

No-output counters are **0 for both arms in every track**: planned accuracy
40/40/2, permission 1/1/2 and injection 1/1/1 per arm. ERP-2 permission
relative change is undefined because C0's baseline is zero, not infinity.

- ERP 1 fixed: `lg11, lg14, lg17, lg24`; broken: `lg04, lg05, lg06, lg07, lg08`.
- ERP 2 fixed: `lg09, lg10, lg11, lg12, lg16, lg17, lg24`; broken: **`lg02, lg08`**.
- Demo: no fixed/broken questions. ERP-2 permission fixed: `lgdeny01`.
- ERP 1 accuracy: model refusals C0/C3 **0/10**, query failures **13/1**.
  ERP 2: model refusals **0/7**, query failures **11/0**. Refusals are incorrect
  answers, not missing outputs or successful policy answers on accuracy tasks.

First publication preflight initially matched 39/40: lg36 executed but its
anonymous result name failed public serialization. That receipt and the
subsequent owned-Gateway rollout are retained. No paid model trial preceded
that correction. The first complete 84-response model capture then exposed
an evaluation bug: accuracy refusals aborted scoring. The scorer now counts
them as failures while still forbidding denied accuracy gold. The original
capture and failed scoring are unchanged; recovered scoring uses no new calls.

Pack inspection found all 79 columns labelled free_text, including numbers
and dates. C3 added an unrequested dictionary-type output in lg05-lg07 and
refused lg04/lg08. Internal model rationale is unobserved; semantic
incompleteness is not asserted as a proven causal explanation. Fixes preserve
structural types without granting PII or inventing units, clarify visibility
versus certification, and require requested projections. Numeric aggregate
defaults and the non-star Cartesian cost gap were also corrected. Exactly
one further ERP model round was run, and both results are reported.

ERP-2 still has seven model refusals and nine different typed results.
`lg02` and `lg08` remain C3 refusals despite C0 success; dictionary/business
semantics need further source-grounded review. No answer patching or third
model round was used to remove these residuals. C0's ERP-2 permission attempt
used unsupported JSON extraction and was blocked, not a data disclosure.
Model refusal scores do not substitute for Gateway authorization evidence.

Replay receipts:

- ERP 1: `v3-i08b-own-20261111/scored-run1/report.json` equals
  `replay-run1/report.json` byte for byte.
- ERP 2: `v3-i08b-own2-20261111/ab/scored/report.json` equals
  `replay/report.json` byte for byte.
- Demo: `v3-i08b-demo-ab-20261111/scored/report.json` equals
  `v3-i08b-demo-replay-20261111/report.json` byte for byte.

## Regression and cleanup

Evidence summary root: `v3-i08b-final-20261111`.

- `cargo check --workspace --locked`, formatting and spec/plan/render checks pass.
- Full Gateway + Builder suites: **525 passed, zero failures, 50 ignored**.
  CLI: **189 passed, zero failures, three ignored**.
- pg-builder Python: **381 tests, 15 skipped**; A/B: **31 pass**;
  oo-test Python: **186 tests, one skipped**. Skips/ignores are not passes.
- Standalone pg-builder Rust: **67 pass, five missing-historical-evidence
  failures**, reproduced identically on base `ed9c04c52` with `--no-fail-fast`.
  Exact names and commands are in `preexisting-failures.json` and the commit
  body. Operator-preview and gateway-audit-wal tests cannot resolve the
  pre-existing exact rustls 0.23.45/0.23.46 conflict; the same commands fail on
  that base before tests run (`v3-i08b-casts-20261111/preexisting.json`).
- Local installed **QS-01..05 harness passes** on PostgreSQL 17.10:
  `v3-i08b-qs-20261111`. Install to first answer **16.359 s**. Three generated
  MCP configurations complete readiness/query/answerability. The denial is
  independently read from the audit DB; source 120 versus analyst 60 rows
  proves the demo row filter. Resource inventories before/after are identical.
  These cover the local warm-image/no-model path, not three-platform cold
  installation, real desktop-app sessions or `quickstart --rebuild`.
- Final exact-model-material and credential-signature evidence scan: **zero
  hits** (`evidence-scan.json`). Failed attempts remain, alongside corrected runs.
- All task-created containers, volumes and networks are absent; native PG
  clusters are stopped. Generated runtime credentials/state/installations were
  hashed then removed. Intermediate image tags were removed; only the requested
  final runtime image is retained. No unrelated resource was removed.
