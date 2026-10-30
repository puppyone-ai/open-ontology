# Library Self-build operator entry

For the resumable observation -> full semantic draft -> reviewed YAML/Pack path,
use `scripts/library_build.py --config /private/build.json --output /private/new-build`.
`--through observation|draft|package` exposes explicit checkpoints; `--resume`
verifies existing artifact hashes. The default business delivery profile requires
named assets in all seven semantic collections. Operator-pinned column/semantic
reviews are not an authenticated ledger or an owner signature. See
`../../../docs/self-build-complete-rebuild-20261022.md`.
The draft contract is now `oo.library.semantic-draft/v3`: examples and glossary
arrays are mandatory alongside datasets and metrics. Existing v2-only receipts
are historical evidence, not valid new full-build output.

Business packaging also requires explicit `release_versions` with `ontology`,
`catalog`, `policy` and `knowledge` tokens. Both OOS and visibility use the same
values; changing them does not update a running Gateway or mint credentials.
`scripts/measure_reconciliation.py` provides same-statement, exact local arithmetic
checks for a declared single-key table, explicit scope, optional date period and
linear numeric measure. It never claims source completeness or Gateway snapshot
equivalence and does not certify a business metric.

`python3 scripts/library_selfbuild.py --config /private/library-build.json --output /private/new-build`

This entry consumes the existing Agent DB and the LIB `oo.library.manifest/v1`
inventory. It does not import, publish, grant SQL privileges, or start a Gateway.
Use an operator-approved local copy, never customer source credentials. All SQL
observations run in READ ONLY transactions and return metadata or aggregates.
The output directory must be new. Customer artifacts belong outside Git.

Minimal installation configuration (all names are anonymous):

```json
{
  "psql_command": ["psql", "service=local_builder_copy"],
  "ddl_directory": "/private/ddl/tables",
  "library_manifest": "/private/library-manifest.json",
  "max_tables": 4096,
  "statement_timeout_ms": 30000,
  "max_relationship_checks": 1024,
  "min_enum_containment": 0.5
}
```

S0 preserves SHOW CREATE TABLE column comments, primary keys and DDL digests.
The actual physical mapping and source/generation vector come from Agent DB,
not a fabricated uniform source. Empty/missing profile observations are not
reported as measured. `s0_input` can reuse a retained S0 artifact; `facts_input`
can reuse immutable facts. These are trusted operator inputs, not client uploads.
With a manifest, all non-excluded table identities must be covered exactly.
Without one, the entry covers the available imported inventory, not a promise
that every source table was imported or that exclusions were approved.

`facts.json` is deterministic for the supplied observations. S3 considers all
tables before batching; exact distinct and row-weighted containment are separate
signals. Validation has an explicit observation budget; `budget_unmeasured`
remains a candidate, not a successful check. Increase that budget to measure
more of the graph. S4 retains partial dictionary matches and source-comment
label matches as *unapproved candidates*, including competing discriminators.
Empty codes are excluded from the overlap denominator, but remain in facts and
review evidence. No meaning is certified from overlap alone.

## Declared dictionary adapter

An optional `declared_enum_adapter` object supplies `controls`, `headers` and
`dictionary` datasets and column-role maps; see the project example
`../../../config/declared-enum-adapter.json`. These names are configuration,
not a hardcoded YuJia schema in the product. The selected inventory must include
the three metadata datasets and intended target datasets. Column roles resolve
through `source_name`, including connector-encoded names such as `refEnum`.

`schema_aliases` and nonempty `dictionary.scope` are mandatory. Discovery joins
control ID **and entity key**, bounds each result to `max_rows` (default 500,
maximum 5000, overflow fails), and records missing targets, conflicting types
and labels. Scope and legacy schema aliases remain pending review.

Standalone and control S1 use the same `observe_facts` entry. Evidence carries
configuration, inventory and per-binding hashes plus the generation vector.
Retained facts must have exactly the corresponding declared candidates; missing,
extra or changed bindings fail. Inventory identity does not establish one
transaction snapshot across the observation queries. S5 receives bounded labels,
and review queues retain complete evidence. No adapter result automatically
approves labels, opens dictionary columns or certifies a financial measure.

### Stored report definitions

`report_definition_adapter` optionally captures existing local report metadata;
the project example is `../../../config/report-definition-adapter.json`.
Configure one dataset, selected source columns, search columns, literal search
terms, and an explicit nonempty scope. Limits default to 200 rows / 1 MiB;
the maximum supported bounds are 1000 rows / 4 MiB. Row/byte checks happen in
the same SELECT before JSON transfer. Overflow is an error, not silent sampling.

S1 retains these definitions with configuration/inventory/evidence hashes.
S5 and compile reject drift. The review queue points to the private facts;
raw report SQL and UI configuration are **not** sent to the model or published
in a role Pack. Stored SQL is untrusted evidence, never executed. Duplicate
versions, blank bodies, inactive definitions and scope limitations require
review; a report title alone does not establish a metric's implementation.

### Metric generation and reviewed input

S5 now uses `oo.library.semantic-draft/v2` and requires a `metrics` array.
Candidates include `dataset` plus OOS metric definition fields, but never model-
supplied certification, owner, tests or control totals. They survive merge,
effective OOS and generated YAML. `suggestions.json` exposes the bounded candidates
at the existing authenticated candidates endpoint. Old description-only caches
cannot satisfy a new S5 run; historical replay remains distinguishable in audit.

An operator may install `metric_review_input: {"path": "...", "sha256": "..."}`.
The pinned JSON uses `protocol: "oo.library.metric-review/v1"`, `project`,
`facts_sha256`, and a `reviews` object keyed by metric identifier. Each entry has
`candidate_sha256`, `reviewer`, and the complete reviewed OOS `definition`.
The definition must preserve every semantic field of the candidate; only explicit
certification fields may be added. OOS validation and role visibility still apply.
This is an operator input adapter, not an online owner-authentication or ledger
certification API. It executes neither metric SQL nor reconciliation SQL, and
does not assert `business_owner_signed`. Protect review files outside Git.

`include_candidate_metrics: true` explicitly publishes visible candidate definitions
as NOT CERTIFIED, not business answers. It does not enlarge table/column grants.
`required_metrics: ["actual_metric_id"]` fails compilation when the **compiled role
Pack** lacks any required active certified metric. Candidate-only or hidden
definitions do not satisfy it. `metrics/availability.json` is the role-scoped
machine-readable inventory; compilation still does not prove runtime or business
readiness. See `../../../docs/metrics-pipeline-recovery-20261022.md` for the audit
format, installation boundary and remaining business release gates.

### Event-level reconciliation

Use `scripts/event_reconciliation.py --config /private/event-check.json --output
/private/new-event-check` for bounded, read-only aggregate reconciliation. A
private configuration includes `psql_command`, `ddl_directory`, and:

```json
{
  "reconciliation": {
    "period": {"start": "2026-09-01", "end": "2026-10-01"},
    "left": {
      "dataset": "app.payment_header",
      "keys": ["tenant_id", "company_id", "event_id"],
      "scope": {"tenant_id": "101", "company_id": "1001"},
      "amount": "amount", "time": "paid_time"
    },
    "right": {
      "dataset": "app.payment_detail",
      "keys": ["tenant_id", "company_id", "event_id"],
      "scope": {"tenant_id": "101", "company_id": "1001"},
      "amount": "amount", "time": "paid_time"
    }
  }
}
```

Both sides require equal explicit scope, included in their composite keys.
Each side is aggregated **before** the full join to avoid duplicated amounts.
One SQL snapshot records unmatched events, multirow events, null keys/amounts
and per-event amount differences. Exact numeric totals are strings; empty SUM
remains null. Only date / timezone-free timestamp clocks are supported; a
timestamptz needs a separately specified business-zone contract, not a guessed UTC
conversion. Missing dates and cross-period timing differences remain limitations.
The result is pending evidence, not a customer-receipts total or approved metric.
No source DB, row-level exports, SQL extracted from reports, or model API is used.

## Optional bounded S5 and Pack

Add these fields only when actual drafting is intended:

```json
{
  "draft_datasets": ["app.items", "app.units"],
  "max_draft_batches": 1,
  "model": "devin/claude-sonnet-5-5",
  "endpoint": "http://127.0.0.1:8317/v1",
  "bearer_file": "/private/model-bearer",
  "builder_binary": "/opt/oo/bin/oo-builder",
  "pack_ceilings_file": "/private/pack-ceilings.json"
}
```

Each batch contains at most 16 tables, global relation/enum context, and
neighbour key summaries. No sample values or physical mappings are sent. The
model can supplement descriptions and propose glossary terms, not overwrite
facts. Requests are sequential, deadline 600s, completion at most 40000, total
estimated budget 140000. Claude receives no temperature field. Complete JSON
fences are accepted; incomplete outputs are refused. Retryable failures have a
65s cooldown and bounded attempts. Explicit provider quota exhaustion,
insufficient quota, model cooldown and authentication failures stop new provider
work instead of repeating a known-unavailable request. Their failed receipts
remain; a resume does not silently reset a terminal provider failure or change
credentials/model. Completed batch receipts are resumable.

A trusted ceilings file may contain:

```json
{
  "max_datasets": 4096,
  "max_relationships": 32768,
  "max_enums": 32768,
  "max_entities": 4096,
  "max_entries": 16384,
  "max_file_bytes": 4194304,
  "max_total_bytes": 67108864,
  "json_scan": {"max_bytes": 67108864, "max_nodes": 4000000, "max_depth": 32}
}
```

The ceilings file is passed to the existing Rust compiler using
`oo-builder library-role-pack /private/pack-ceilings.json`. The old `role-pack`
command keeps its original limits. Per-file token budgets are still not measured
by a tokenizer; byte ceilings are not a token-budget proof.

External model credentials can use `"api_key_env": "GEMINI_API_KEY"` instead of
`bearer_file`. An explicitly configured but missing variable fails closed,
without credential fallback. The shared provider requires HTTPS outside
loopback and refuses authentication redirects. Before S5, check both model
listing and real generation:

```sh
python3 scripts/model_provider.py --config /private/model-provider.json
```

The private file contains `endpoint`, `model` and `api_key_env`; no key value.
The probe is bounded, does not retry, and reports sanitized error categories
including `IP_BLOCKED`. Readiness does not certify business answers or provider
model identity. Set `require_tools: true` for an additional synthetic tool call
and tool-result roundtrip. It sends no business data and executes no external
tool; non-streaming readiness is separate from a full streaming CLI cold test.

Compatible aliases may be callable without appearing literally in `/models`.
`require_model_listing: false` permits only a missing list entry or HTTP 404/405
to continue to actual generation; authentication, transport and generation failures
still fail. The report preserves the failed listing check rather than claiming it
was listed. `model_stream: true` enables bounded SSE drafting for long responses.
A terminal finish frame is mandatory; clean EOF immediately after it is supported
for compatible proxies that omit `[DONE]`. EOF without a finish frame is not a
successful draft. Provider response model labels are recorded, not independently
authenticated. Stored receipts keep the exact request/response hashes.
Column-coverage failures now return bounded field-name diagnostics to the next
attempt. The retry must still return the complete object; the builder never
silently fills omitted columns. Repaired receipts retain the validation feedback
and exact corrected request hash. Explicit, hash-pinned retained inputs preserve
older prompt receipts with a separate provenance audit instead of rewriting them.

Review is fail-closed by default. `auto_accept` enables the configured
high-confidence non-sensitive rules; it does not approve glossary or metrics.
`fixture_review: true` is **test only**: identities are `fixture/test` and
`business_owner_signed=false`. `include_candidate_relationships: true` explicitly
exposes unreviewed candidate relationships, labelled by the existing compiler.
`pack_datasets` can select a scoped test sample, without changing the facts input.
A changed facts hash refuses an unbound/legacy retained model draft, unless the
operator explicitly sets both `fixture_review` and `allow_fixture_rebase`; that
test-only rebase is audited. Dependency-bound S5 reuse is described below.
This is not a production business-owner approval or a replacement review ledger.
The control API's existing asynchronous S0–S10 scheduling is not wired to this
operator command in this change.

## Verification

```sh
OO_LIBRARY_BUILDER_BIN=/opt/oo/bin/oo-builder \
  python3 -m unittest discover -s scripts -p test_library_selfbuild.py
python3 scripts/library_fixture.py \
  --output /private/new-anonymous-fixture --builder /opt/oo/bin/oo-builder
```

The live fixture owns only `oo-wf-bld-fixture-pg` and its named volume, creates
72 anonymous tables from six sources (one 502-column table), runs S0–S4 against
the actual fixture PostgreSQL, drafts five batches through a labelled fixture
provider, tests cache resume without another call, and compiles one Pack.
The container has no network and is removed with its own named volume on exit.
It refuses pre-existing resource names. This does not test LIB publication.

S0 keeps both type bases: `data_type` is the actual PostgreSQL catalog type and
`source_type` is the DDL type. A source `datetime` must not be presented as a
PostgreSQL SQL type. The facts preserve the source comment separately from any
fixture-reviewed LLM supplement. Dense batches are split further by an explicit
context-byte budget (`max_batch_context_bytes`, default 240000), never by dropping
column facts. Native-safe types/declared keys can use configured automatic
classification; name-only text hints still require review. Observation errors are
recorded as unknown evidence, not certified zeroes or approved dictionary meanings.


## Authenticated library control entry (INT)

The standalone command above remains available. The control API now has a trusted
`library` project input and a controlled `library_control.py` phase entry. It reuses
existing asynchronous checkpoints, generated/curated overlay storage and authenticated
review-ledger decisions. Project registration remains installation-side; `createBuild`
queues the bound library build. S1–S4 share one deterministic global facts artifact.
For retryable errors, per-batch cooldown applies after the final failed attempt too. Retained S0 must
match its file digest and current per-table generation vector. Drafts must pass
the exact-facts legacy checks or the dependency-bound reuse checks below.

See ADR-030 and the INT verification record for the actual acceptance boundary. The
old statement that the standalone entry was not wired remains a description of BLD's
original delivery, not of this additive control entry. Glossary candidates remain drafts
unless explicitly approved; a description decision alone must not approve terminology.

## Parallel library control drafts

Select the model explicitly in the private build configuration; previous
Grok runs do not override the operator-selected provider. The control adapter accepts
`s5_concurrency` (default 1, valid integers 1–32). Increase concurrency only within
an explicit resource budget, especially after an OOM. Each batch owns its model-bound
cache/failure files; retryable 65-second cooldowns block only that batch. Explicit
quota/cooldown/authentication failures stop new provider calls for the run;
already-running calls retain their bounded outcomes. Results
are returned in batch-index order. Different model IDs never share cache paths.

S5 context retains strong incident relationships and at most five measured positive
candidates per table; empty/unmeasured weak candidates remain in deterministic facts,
not the model context. Enum context carries its reference, evidence, total code count
and explicitly truncated sample (up to 16 values), never a claim that the sample is
complete. Full observed values/dictionary candidates remain in facts for review and
compilation. Sixteen-table and context/request/completion budgets are unchanged.

A new controlled build may reuse SHA-pinned S0/facts artifacts only after comparing
all per-table source/target generations with the current Agent DB vector. That reuse
does not reuse a prior model's S5 output. Operator automation must report cached versus
recomputed stages, stage times, failed batches, review scope and actual compile result.

## Dependency-bound S5 maintenance

`library_selfbuild.py::run_drafts`, `library_control.py::drafts/retained_drafts`
and `library_build.py` preflight now share the existing batch/request path plus
`library_semantics.py`. This is not a second compiler or a publication framework.
The Rust control API supplies the project, role, principal and a digest of the
actual policy/membership to S5. Local configs may explicitly supply the same
`semantic_scope` object (`project`, `role`, `principal`, `permissions_sha256`).
Missing scope permits only generation-bound, work-directory-local cache use;
it does not authorize importing a dependency cache into another build.

- `facts_sha256` / `data_observation_version` retain the full, current observation
  binding. Never reuse S0-S4 merely because an S5 semantic digest is unchanged.
- `semantic_input_digest` binds each batch's full structure/types/DDL, source IDs,
  classification/permissions, full code sets, dictionary documents/declarations,
  all incident discovery results (including negative/weak results), and neighbor
  schemas. Hashes cover values omitted by the bounded prompt. Missing referenced
  inventory is a refusal, not an empty dependency.
- Counts, physical generations and observation timestamps do not enter the model
  request. Null/empty/text-shape, overlap acceptance/complete/unmatched states and
  parent uniqueness still do. Current full measurements continue through review,
  compilation and release gates. Unknown fact extensions are conservative inputs.
- Overlap-only dictionaries do not prove code/label equivalence. Their generations
  remain dependencies of consumers until full dictionary content is available.
  Complete declared dictionary values and pinned document contents can be compared
  semantically. This conservative case is not reported as data-only zero-S5.
- Cache identity also binds the exact semantic request, provider/model, optional
  `model_revision` / `semantic_rules_version`, rule implementation hashes and
  authorization scope. Changing any of these invalidates reuse. A provider silently
  changing an unpinned model revision is outside this local proof.
- `retained_drafts` still requires an exact file SHA. It preserves previous batch
  partitions so an unrelated new table does not reshuffle all old batches. Invalidated
  batches get normal bounded S5 work, and retain an audit entry. Old-format receipts
  are not silently upgraded: incompatible legacy batch/request hashes remain refused.
- A cache hit checks request, dependencies, output hash, schema and exact coverage.
  The original cache/model receipt is never rewritten. The current-use receipt keeps
  original request/response/batch hashes and records `applied_batch_sha256`, current
  facts, and a `reuse` receipt linking the retained receipt hash and origin facts.
  Reuse is not a new model request and does not transfer approvals.

Stored report SQL is deliberately not S5 input. Its proposals are re-derived from
current facts by the existing deterministic report path, with fresh facts/review
bindings. Column/metric/example approval pins, required business assets, YAML
review bindings, role closure and release gates remain enforced; candidate Pack
compilation is still `published=false`, not authentication or certification.

Local verification (no customer DB, provider call, container, or publication):

```sh
# Use a separate target directory; never replace a running installation's binaries.
CARGO_TARGET_DIR=/private/new-semantic-evidence/target \
  cargo test --manifest-path ../../Cargo.toml --locked --offline -p oo-builder --lib --tests
OO_LIBRARY_BUILDER_BIN=/private/new-semantic-evidence/target/debug/oo-builder \
OO_OOS_TREE_BIN=/private/new-semantic-evidence/target/debug/oos-tree \
PYTHONDONTWRITEBYTECODE=1 \
  python3 -m unittest discover -s scripts -p test_library_semantics.py
```

The semantic fixture mocks only the model transport and forbids DB queries. It
exercises the real S5 request/validation/cache runners, S5-S8 stage entry, review
projection, regular OOS YAML compiler and Pack digest equality against a fresh
build. Separate existing dictionary/metrics/assets/OOS tests cover pending and
tampered approvals. This is a deterministic local slice, not L1-05/L3-06 completion,
10k I/U/D acceptance, real-model evidence or the full business publication gate.
