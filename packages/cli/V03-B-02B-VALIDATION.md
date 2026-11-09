# V3-B-02b sign-off

Status: partial. Run: `v3-b-02b-20261109`. Raw logs and fixtures remain
git-ignored under `evidence/v3-b-02b-20261109/`. This supplements, rather than
overwrites, the earlier lane handoff.

## Delivered

- Applied the release integration patch. The tracked-source Linux arm64
  Docker build exported both `oo` and `mcp-gateway`. Build stages carry
  `oo.lane=b`. Workflow YAML passed yamllint. The host yamllint executable
  had a broken interpreter; an evidence-local venv supplied the working lint.
- Mounted authenticated resolve, value-search and gap-feedback routes.
  Lookups select only Pack-visible columns, use the existing SQL guard,
  role session, publication binding and query audit, and return at most eight
  candidates. Composite entity keys retain their ordered key components.
  Multiple matches require confirmation. Hidden targets return a closed error.
- Gap reports enter a private, fsynced `.gap` inbox with a corresponding
  `gap` audit row. The audit contains a payload digest, not the description.
  Reports are untrusted pending-review inputs, not automatic ontology edits.
- Preflight indexes role-scoped graph entity names/aliases, table and column
  labels, approved column descriptions, certified metric names/synonyms,
  approved enum codes/labels, and glossary terms. English stemming, bilingual
  synonyms and conservative CJK segmentation are deterministic. Unknown terms
  produce a closed gap code and count, not hidden-name suggestions.
- Pack snapshots now also cover preflight and semantic tool routes across
  reloads. `covered` remains advisory vocabulary coverage: it does not prove
  executable joins, correct metrics, data completeness or freshness.
- CLI status projects explicit library-scoped review evidence into
  `held_by_review` and `review_state`, redacting pending-review details.
  Missing measurements remain null/unmeasured, including Gateway freshness.

## Validation

| Check | Result | Raw evidence |
| --- | --- | --- |
| `cargo check --workspace --locked` | Pass | `signoff-check.log` |
| `cargo test --locked -p oo-gateway` | 311 passed, 50 ignored | `gateway-accepted-tests.log` |
| `cargo test --locked -p oo-stack -p oo-cli` | 114 passed, 3 ignored | `p1-final-tests.log` |
| `cargo test --locked -p oo-gateway --lib http_gateway` | Pass | `http-final-tests.log` |
| `python3 crates/oo-gateway/tests/token_owned_pg.py evidence/v3-b-02b-20261109/pg-final --meta-only` | Pass, PostgreSQL 17.11 | `pg-final/meta-pg.log` |
| Tracked-source `docker buildx build --platform linux/arm64 --file ci/cli.Dockerfile --output type=local,... -` | Pass | `docker-build.log` |
| Workflow yamllint | Pass | `yaml-lint-retry.log` |

The owned PostgreSQL test covers real authenticated metadata and lookup
execution, hidden targets, strict feedback arguments, durable inbox/audit,
single-match keys, ambiguous matches, the eight-candidate cap and publication
drift. Product reads use the role session; setup and mutations use the separate
fixture administrator. No customer database was accessed.

Initial failures remain recorded: an invalid fixture mapping identifier,
missing query format, unsupported DISTINCT, and incomplete test metric/enum
definitions. These were corrected, not waived. Lookup grouping now uses the
guard-supported GROUP BY shape. No baseline failures were waived.

## Offline semantic evaluation

Prepare with:

```sh
python3 packages/cli/scripts/coverage-fixtures.py evidence/<run>/coverage
cargo run --locked -p oo-gateway --example coverage_eval -- \
  evidence/<run>/coverage/t6-input.json evidence/<run>/coverage/t6-pack \
  evidence/<run>/coverage/t6-questions.json
```

Repeat the second command with `demo` instead of `t6`. Output directories must
be new. The evaluator exits nonzero if either threshold fails.

The T6 questions are unchanged from `testing/engines/gold/questions.json`.
T6 metadata comes from the existing seed DDL and is compiled by the real Pack
compiler; it is an offline fixture, not a runtime publication. Demo Pack/gold
are read with `git show v0.3/m:deploy/lite/demo/...`; the exact branch revision
is saved in `coverage/demo-revision.txt`. No SQL results are fabricated.

Only `covered` counts as a pass; partial counts as negative.

| Set | TP | FN | FP | TN | Positive recall | Negative false-pass |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| T6 + 20 authored negatives | 19 | 1 | 0 | 20 | 95% | 0% |
| Demo + 3 authored negatives | 3 | 0 | 0 | 3 | 100% | 0% |
| Combined balanced set | 22 | 1 | 0 | 23 | 95.65% | 0% |

The demo's original denied question is an additional true negative, not one of
the three authored negatives. T6 Q20 (January/February shards) remains partial.
Raw final rows: `coverage/t6-graph-result.json` and
`coverage/demo-graph-result.json`. Initial lower-recall outputs are retained.
The small, authored evaluation is not a held-out general accuracy claim.

## Unfinished P1 and residuals

- `--tls local-dev` is not implemented. Source configuration and connector
  transport currently require a CA and verified TLS; the connector config/
  engine/session changes are outside this card's write scope, as is the init
  wizard. No insecure bypass or misleading accepted-but-unused flag was added.
- Gateway cannot yet report a measured review hold. The producer stores the
  library review reason in control-DB `jobs.lite_cycle_events`; Gateway reads
  the published Agent DB quality snapshot, which has no review-reason signal.
  `publication != live` is deliberately not mapped to `held_by_review`.
  A scoped producer/read contract is required; no speculative migration or
  control-DB privilege grant is proposed as a ready-to-apply patch.
- Lookup resolution uses the published primary dataset and display columns;
  cross-member instance-alias resolution is not implemented. Nontext display
  columns remain refused by the SQL guard rather than silently cast.
- Feedback is durably queued locally; automated review-queue consumption is
  not implemented. Literal lookup patterns reject wildcard/backslash input.
- No MySQL/Oracle engine run, Linux x64/Darwin build, real Agent client session,
  source-data refresh, or customer/business acceptance is claimed.
- No root or standalone Cargo.lock changes. No shared-file patch is applied
  beyond the explicitly granted Dockerfile/workflow. No containers were
  created by tests; all owned PostgreSQL processes were stopped.
