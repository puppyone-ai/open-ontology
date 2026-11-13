# V3-I-10: ontology quality diagnosis and variance measurement

STATUS: **partial: measured improvement, +15% target not met**.
Measurements: 2026-11-11; recovery verification: 2026-11-12.

This is a synthetic ERP experiment, not customer or production acceptance.
Raw evidence and reproducible local drivers are under
`evidence/v3-i10-20261111/`. The installed source run is
`evidence/v3-i10-mysql3-20261111/` (candidate) and
`evidence/v3-i10-mysql4-20261111/` (final restored guidance).
Resume evidence is under `evidence/v3-i10-resume-20261112/`.

## Changes and boundaries

- `f949a0b32`: retain closed Gateway failure codes, without remote error text.
  Operational errors cannot count as permission-track successes.
- `c5946931f`: limit semantic-gap guidance to definitions needed by the
  question; explicit question filters/windows do not require certification.
  Do not infer business meaning from structural types or containment.
- `0212e5762`: explicitly version the reader-context experiment. C3 receives
  exact readable knowledge pages, not the full compiler archive. Historical
  manifests retain their original prompt format.
- `dd2d57af3`: unsupported answer types are failures, not a reason to abandon
  other questions. Gold preflight remains strict; no numeric coercion is added.
- `c8e17cdf2`: revert the unsuccessful wording candidate to the exact
  109004a40 compiler guidance after the controlled ablation below. The
  reader-context experiment remains explicitly selectable, not the default.

Questions, gold, source CSVs, roles, reviewed demo inputs and Cargo.lock are
unchanged. Only compiler-derived snapshots/demo Pack hashes are regenerated.
`frozen-inputs.json`, `source-row-hashes.json` and `regeneration.json` bind
these checks. No question-specific hints, new dictionary meanings, answer
patches, comparator relaxation, model judge or nested agents are used.

## Historical diagnosis

`diagnose.py` verifies every captured prompt hash and retains per-question
prompts, SQL and Gateway receipts for all four historical rounds. The
`*-questions.json`, `*-cases.json` and Pack diffs cover differences across
both arms and runs, not only headline regressions.

| Historical C3 run | Pass / 40 | Model refusals | Correct values, wrong numeric type | Extra projections | Other values | Lost Gateway failure |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| I-08 first | 18 | 10 | 8 | 3 | 0 | 1 |
| I-08 after 109004a40 | 24 | 7 | 8 | 0 | 1 | 0 |
| I-09 seed 42 | 16 | 16 | 8 | 0 | 0 | 0 |
| I-09 seed 7 | 15 | 16 | 8 | 0 | 1 | 0 |

The old recorder discarded non-policy Gateway failures: C0 lost 13, 11, 6,
and 8 envelopes respectively. They cannot be reconstructed as historical
facts. Current re-execution is recorded separately and explicitly labelled;
new captures retain closed codes.

Observed C3 transitions:

- I-08 first to second: fixes lg05, lg06, lg07, lg09, lg10, lg16; no losses.
- I-08 second to I-09 seed 42: loses lg05, lg07, lg09, lg10, lg11, lg12,
  lg16, lg17; no gains.
- I-09 seed 42 to seed 7: loses lg06; no gains.

### Same-source compiler comparison

`rebuild-packs.py` recompiles the identical retained I-09 review input,
adjudication and confirmation using the runtime compilers bound to
109004a40 and 1f8419feb. Input SHA-256:
`54bb755d72d275dc2fdb1d2455832684d672c8ca609fff135c17c3e4ac2ce4cc`.
The I-09 rebuilt artifact equals the historical published artifact exactly.

Only README guidance, availability wording and the derived manifest change.
There are no changed table types, dictionary facts, metric definitions or
join paths. Same-source Pack digests:

- 109004a40: `27391e9cb39acfe911b5566c9be60420b2be2e915bc58842ba12a9231c3a8853`
- 1f8419feb: `36859f4f05007bf3429c160ff755e1dd332918312b53f89076ccfa6a92d890e9`
- c5946931f: `5b834b87822c0605a8b1e50c9f287802318bc70a25af9388b9c81be3e93b6af0`

Fresh Self-build changes ten entity display names from unqualified to
schema-qualified names, plus their derived hashes; it adds no business facts.
`fresh-draft-metadata-diff.json` records the exact leaves. Therefore the live
compiler ablation recompiles all three versions from the *fresh publication's
same reviewed command*, and asserts that the current output equals the
published Pack. This separates compiler wording from fresh-draft variation.

### Cause categories

- **Wrong Pack facts:** no changed business facts explain the historical
  regression. Structural classifications are not business definitions.
- **Misleading wording / refusals:** refusals occur before Gateway execution.
  The broad unknown-meaning warning coincides with eight lost questions, but
  captured output does not reveal internal model reasoning. Paired transitions
  are observations, not proof that a particular sentence caused a refusal.
- **Missing semantics:** the source catalog has no business comments and the
  Pack has no reviewed dictionary discriminator/validity bindings or certified
  metrics. Coverage and key uniqueness cannot supply those meanings.
- **Missing join paths:** no missing or changed path explains the historical
  compiler delta. Existing relationships remain available; arbitrary
  containment is not evidence of a business definition.
- **Metric/result contract:** eight historical C3 queries return the right
  values as PostgreSQL numeric while gold requires int64. They remain failures.
  Blanket bigint casts would be a benchmark-specific, overflow-prone workaround.
- **Pack noise:** the I-09 envelope is 88,937 bytes for lg01; readable Markdown
  and SQL account for 23,453 bytes of its 74,590 content bytes. Archive hashes,
  compile-time status and graph/navigation indexes dominate the remainder.
  The new read set preserves exact page text and availability warnings.
- **Gateway admission:** failures are distinguished from semantic refusals;
  no Gateway rule, authorization check or supported SQL set is loosened.

## Measurement protocol

`measurement-plan.json` fixes model seeds 42, 7 and 2026, both arms, the
unchanged 40 ERP questions and demo, before the new capture. Source seed is 42.
Model: `gpt-6.1-sol`. C0 remains the existing table/column-name-only baseline,
not full DDL or an interactive agent. C3 uses `knowledge-pages-v1`.

All selected ERP pages are read through authenticated, publication-pinned
`oo knowledge cat`; their path/text must equal the model-facing pages.
This fixed read set is independent of the question. It is not adaptive
retrieval or an interactive MCP conversation.

Compiler ablation uses seed 42, C3, full-artifact context and identical fresh
review input for all three compilers. Older Packs are diagnostic
counterfactuals, not falsely labelled as the current published Pack.
Provider seed requests do not promise deterministic generation.

The target is C3 >= C0 on each seed and arithmetic mean of the three paired
relative changes >= 15%. No rerolling a failed seed or selecting the best run.

### Controlled ablation and unsuccessful candidates

Seed-42 compiler-only ablation, same fresh reviewed input and serving
publication, full-artifact context:

| Compiler | Accuracy / 40 | Refusals |
| --- | ---: | ---: |
| 109004a40 | 22 | 9 |
| 1f8419feb | 18 | 14 |
| c5946931f candidate | 18 | 12 |

`ablation/summary.json`, `ablation/results.json` and
`compiler-ablation-transitions.json` retain all SQL/results. The first change
loses lg06, lg07, lg09 and lg10, with no gains. The next fixes lg07 and loses
lg11. This controls the compiler/source confound; one model trial per prompt
does not prove a deterministic causal effect or reveal internal reasoning.

Neither reduced prompt size nor the first wording candidate met the target.
Both negative experiments remain in the record:

| Context / compiler | Seed 42 C0/C3 | Seed 7 C0/C3 | Seed 2026 C0/C3 | Mean paired change |
| --- | --- | --- | --- | ---: |
| Reader pages / c5946931f | 19/16 | 19/18 | 21/16 | -14.95% |
| Full artifact / c5946931f | 19/18 | 19/20 | 21/17 | -6.35% |
| Full artifact / restored guidance | 19/22 | 19/20 | 21/22 | +8.60% |

The final experiment is explicitly amended in `restored-guidance-plan.json`;
it is not relabelled as the original reader-pages protocol. Removing archive
noise alone did not help in these measurements. No sentence-level causal
allocation is supported. No question, gold value or comparator was changed.

### Final three-seed measurement

Final source run: `v3-i10-mysql4-20261111`, MySQL 8.4.7 fixture and PostgreSQL
17.10 serving image, native darwin-arm64 client / linux-arm64 runtime.
Installed Self-build, confirmation, publication and 40/40 unchanged typed
gold preflights complete. Install to first answer: 280.263 seconds, warm
images. Budget/confirmation pauses remain recorded as nonzero build exits.

`image-final.json` identifies the reused I-08 runtime image, not a claim that
all runtime binaries were rebuilt at HEAD. Its compiler source equals
109004a40 and the final compiler; `fixed-source-comparison.json` verifies
the exact rebuilt/published artifact. Native client hashes are recorded
separately. Authenticated publication-pinned knowledge reads match every
Pack file. The final ERP lg01 prompt is 88,104 bytes in artifact context.

| Dataset | Seed | C0 | C3 | Relative change |
| --- | ---: | ---: | ---: | ---: |
| ERP | 42 | 19/40 | 22/40 | +15.79% |
| ERP | 7 | 19/40 | 20/40 | +5.26% |
| ERP | 2026 | 21/40 | 22/40 | +4.76% |
| Demo | 42 | 2/2 | 2/2 | 0% |
| Demo | 7 | 2/2 | 2/2 | 0% |
| Demo | 2026 | 2/2 | 2/2 | 0% |

ERP mean C0 is 19.667 (min 19, max 21); mean C3 is 21.333 (min 20,
max 22). The arithmetic mean of paired relative changes is **+8.60%**.
C3 >= C0 holds on all three seeds; the **+15% mean gate fails**.
Demo mean/min/max is 2 for both arms. ERP permission and injection are
1/1 for both arms on every seed; demo permission is 2/2 and injection 1/1.
One C0 accuracy response at seed 42 has no output and remains a failure;
all other no-output counts are zero.

All 42 C0 responses per seed were reused only for identical complete prompt
hash and seed; all proposed SQL was re-executed against the new publication.
Final C3 responses were fresh (no eligible ablation prompt hashes).
`ab-*/reuse.json` records 42 eligible / 42 reused per seed. These are paired
compiler comparisons, not six wholly independent baseline captures.

All six scored reports equal their saved replays byte for byte. On resume,
`python3 evidence/v3-i10-20261111/summarize-final.py` revalidated every prompt
hash and every report/replay equality without new model calls. The summary
and per-question prompt, SQL and receipt diffs are `measurement-final.json`,
`final-{erp,demo}-all-cases.json` and `final-*-sql-results.diff`.

### Stability and residual counts

Pass bits below are ordered seed 42 / 7 / 2026. Twenty ERP questions pass
in C3 on every seed, 18 fail on every seed, and two vary. C0 has 17 always
passing, 16 always failing and seven varying. Both demo questions have
C0/C3 bits 111/111.

| Question | C0 | C3 | Question | C0 | C3 |
| --- | --- | --- | --- | --- | --- |
| lg01 | 001 | 000 | lg21 | 111 | 111 |
| lg02 | 000 | 000 | lg22 | 111 | 111 |
| lg03 | 100 | 000 | lg23 | 111 | 111 |
| lg04 | 001 | 000 | lg24 | 001 | 111 |
| lg05 | 111 | 111 | lg25 | 000 | 000 |
| lg06 | 111 | 111 | lg26 | 000 | 000 |
| lg07 | 111 | 111 | lg27 | 000 | 000 |
| lg08 | 111 | 000 | lg28 | 111 | 101 |
| lg09 | 010 | 000 | lg29 | 111 | 111 |
| lg10 | 000 | 101 | lg30 | 111 | 111 |
| lg11 | 000 | 111 | lg31 | 111 | 111 |
| lg12 | 000 | 111 | lg32 | 000 | 000 |
| lg13 | 111 | 111 | lg33 | 000 | 000 |
| lg14 | 000 | 111 | lg34 | 000 | 000 |
| lg15 | 111 | 111 | lg35 | 000 | 000 |
| lg16 | 101 | 000 | lg36 | 000 | 000 |
| lg17 | 010 | 111 | lg37 | 000 | 000 |
| lg18 | 111 | 111 | lg38 | 000 | 000 |
| lg19 | 111 | 111 | lg39 | 000 | 000 |
| lg20 | 111 | 111 | lg40 | 111 | 111 |

Disjoint observed outcomes across 120 ERP trials per arm:

| Outcome | C0 | C3 |
| --- | ---: | ---: |
| Pass | 59 | 64 |
| Model refusal before Gateway | 1 | 28 |
| Correct values, wrong numeric type | 29 | 24 |
| Different values/order | 9 | 3 |
| FUNCTION_NOT_ALLOWED | 15 | 0 |
| STATEMENT_NOT_ALLOWED | 6 | 0 |
| OVERLOADED | 0 | 1 |
| No output | 1 | 0 |

No changed wrong fact, missing join path or metric definition is found in the
same-source compiler delta (zero changes in those categories). Missing
business definitions remain a product limitation, not a measured allocation
of all 28 refusals. Prompt noise and wording are experiment factors, not
additional disjoint failure counts. The operational OVERLOADED answer is
not retried away and is not an authorization success.

The observed final configuration tops out at 22/40 (55%), not a proven
theoretical ceiling; historical 24/40 was not reproduced as stable behavior.
Even removing its single operational failure would yield a mean paired
improvement of only 10.36%, still below target. Further progress requires
reviewed semantic bindings, general metric/output-type contracts and held-out
evaluation, rather than more benchmark-shaped prompt wording.

## lg02 / lg08 limitation and fix plan

These questions require a binding from a business concept to the dictionary
discriminator, code/label columns and effective interval. Column visibility,
a composite primary key and observed code coverage do not establish that
binding. The source contains no comments supplying it. A guessed discriminator
can return an empty or wrong result, while a blanket refusal also fails the
accuracy task. Neither is a product-quality resolution.

Required product follow-up:

1. Accept source-grounded, reviewed dictionary bindings: discriminator values,
   code/label columns, validity bounds and interval semantics, with provenance.
   Treat source metadata or approved business definitions as inputs, not gold.
2. Validate discriminator-scoped uniqueness, overlaps/gaps, historical intervals
   and reference coverage on the replica. Containment alone must not certify
   the meaning of a terse identifier.
3. Compile these bindings into role-visible lookup guidance and readiness gaps.
   Preserve visibility closure; never export hidden names or infer grants from
   application role codes.
4. Test repeated codes across dictionaries, historical label changes, interval
   boundaries and hidden columns on independent fixtures. Re-run the unchanged
   ERP bank plus held-out questions and fresh seeds.

The same provenance requirement applies to test/deleted flags, accounting
windows and metric definitions. Approved metric output types must be reconciled
without adding answer-specific casts to the evaluation prompt.

## Regression, failed attempts and cleanup

Retained passing regressions, not rerun after reboot:

- `cargo test -p oo-builder --locked`: 204 passed.
- `cargo test -p oo-cli --locked`: 189 passed, three ignored.
- `cargo test -p oo-gateway --locked`: 323 passed, 50 ignored.
- `cargo test -p oo-eval --locked`: two passed.
- `python3 -m unittest discover -s benchmarks/ab -p 'test_*.py'`:
  34 passed (`type-tests-final.log`, after the scoring fix).
- oo-test Python discovery: 186 tests, one skipped (`final-oo-test.log`).
- Builder/CLI formatting and spec/plan/render checks passed in the retained
  run. No Rust code changed during resume. Before the report commit,
  `CARGO_BUILD_JOBS=1 cargo check --workspace --locked` passes again
  (`v3-i10-resume-20261112/check.log`).

Resume command:
`python3 tools/oo-test/test_quickstart_live.py --package
evidence/v3-i10-20261111/package-final --run-id v3-i10-qs-resume-20261112`.
All **QS-01..05 local harness checks pass**, PostgreSQL 17.10, install to
first answer 14.851 seconds. Independent audit readback confirms denial;
120 source versus 60 analyst orders proves RLS; all three generated MCP
configurations complete readiness/query/answerability. Before/after Docker
resource inventories are identical. No model calls or source database were
needed in this resume; only one owned lite stack ran and was removed.

The earlier QS-02-named run directory (`v3-i10-qs2-20261111`) passed
QS-01..04 but failed QS-05 global inventory equality while unrelated
resources changed. That failure is preserved, not retroactively passed.
The host OOM/reboot is an environmental interruption, not evidence of a
product defect. The fresh successful run does not erase either failure.

No shared-file patch or Cargo.lock change is requested. Questions, gold,
demo seed and roles still match `frozen-inputs.json`. Final task-owned
container/volume/network absence and retained report hashes are recorded
in the resume evidence. Unrelated containers were not stopped or removed.

Initial failures are retained, not relabelled:

- Replacing only compiler binaries leaves the publisher's embedded compiler
  inconsistent. Publication refuses before any A/B call.
- The second harness accidentally selects that same staged package; image
  identity proves it. A complete official runtime rebuild and an explicit
  runtime-image assertion correct the harness.
- The first 67-response capture aborts on lg32's extra date projection, a valid
  Gateway result outside the comparator's types. The scoring fix counts it as
  a failed answer. Recovery reuses all 67 saved model responses, including a
  no-output response, and calls the model only for missing trials.
- Recovery first encounters an unavailable Compose plugin in empty HOME, then
  missing source-secret mounts after source cleanup. The standalone Compose
  binary and a serving-only configuration restore only the owned databases
  and Gateway. Unused model/source mounts are omitted; no source server or
  source connection is restarted. Publication/data-vector binding is unchanged.
- An initial stale compiler snapshot test and a new test's stale version field
  fail before their corrected passing runs; logs remain.

These are local warm-image, native macOS arm64 CLI / linux-arm64 runtime
measurements. They do not establish cold downloads, three-platform installation,
real desktop-agent sessions, `quickstart --rebuild`, production freshness or
customer acceptance. `source_time=unmeasured` and per-table/non-atomic source
consistency remain explicit.
