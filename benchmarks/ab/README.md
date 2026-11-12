# C0 / C3 paired evaluation

Repository-local CLI and deterministic scorer for V3-H-01. Python 3 (standard
library only) orchestrates the existing Rust benchmark programs. No product
workspace dependencies or lockfiles change.

## Run

### Live capture

Supply a hash-pinned manifest whose `publication` is an object containing
`publication_id`, `data_vector_sha256`, `knowledge_version`, and `policy_version`
from an authenticated query receipt. Its `pack` input must be the published
role Pack, not the historical `pack-recorded.json` fixture. Select the matching
installed CLI profile with `oo use` before running:

```sh
oo-eval ab --arms c0,c3 --dataset evidence/run/input.json \
  --output evidence/run/capture --profile demo --oo /path/to/installed/oo \
  --model-endpoint-file /private/model-endpoint \
  --model-key-file /private/model-key --model gpt-6.1-sol
oo-eval ab --arms c0,c3 --dataset evidence/run/capture/dataset.json \
  --output evidence/run/replay
```

The model files are read in place through the shared egress private-file
checks. They are never imported. Defaults: two concurrent calls (maximum
four), 90-second transport timeout (maximum 120), and at most two transient
retries. Authentication errors are not retried. SQL travels through a temporary
0600 file into the installed CLI; all execution remains subject to the Gateway's
read-only policy and pinned publication/data vector. The requested profile must
already be active; the runner does not change the user's selection.

Capture writes private model responses, Gateway receipts, a replay manifest,
and `scored/report.json`. Replay of that manifest reproduces the report bytes,
including captured live provenance; it is not a second live measurement.
The same per-arm 5% no-output health gate applies. Explicit model refusals are
not evidence that the Gateway enforced a denial. Unsupported answer types count
as failed answers without coercion or aborting other trials; unsupported gold
types still stop preflight. The comparator supports exact integers, decimals,
booleans and text.
Publication receipts bind execution, while the Pack file hash is declared by
the operator: verify it against the deployed Pack before claiming published-Pack
acceptance. These instructions alone do not establish a completed live run.

For new reader-context experiments, set `"context_format": "knowledge-pages-v1"`
in the input manifest. C3 then receives the exact path/text of every Markdown
and SQL page plus `availability.json` and `metrics/availability.json`, as
available through `oo knowledge cat` / MCP `read_knowledge`. This is a fixed
read set, independent of the question; it preserves certification warnings
and does not summarize or add facts. Archive hashes, the compilation manifest,
and machine navigation/graph indexes are not eagerly injected as prose.
Verify the selected text against the authenticated knowledge endpoint.
This tests a bounded reader context, not interactive tool use or retrieval.
C0 is unchanged. Omitting the field retains the original full-artifact prompt,
so historical recordings remain replayable. The manifest and prompt hashes
bind the chosen format; never compare formats without reporting the change.

### Frozen fixtures

From the repository root:

```sh
for name in question-bank sql-runner query-result-adapter result-comparator trial-statistics; do
  cargo build --locked --manifest-path benchmarks/$name/Cargo.toml
done
cargo run --locked -p oo-eval -- ab --arms c0,c3 --dataset demo \
  --output evidence/my-ab-run
cargo test --locked -p oo-eval
```

The integration test `scorer_health_and_p2_t_replay` runs the Python unit suite
and `test_p2_t_reproducible_replay`. Build the five benchmark binaries first;
missing binaries are failures, not skipped tests. The CLI currently requires
this source checkout and Python; it is not a standalone installed product.

`--dataset demo` retains two accuracy questions and the q02 permission case. Bare
`--dataset oo-logistics` refuses instead of silently substituting demo.
Use a publication-bound manifest for the ERP bank below.
A new output directory under this worktree's ignored `evidence/` is
required. Existing evidence is never overwritten. Exit 0 means the pipeline
completed, not model quality, health, publication, or security acceptance.
Exit 2 is configuration/execution failure with a closed error code.

## What runs

1. Verify SHA-256 pins for schema, Pack, frozen bank, demo gold and responses.
2. Validate the public synthetic bank with `question-bank`; match its questions
   and gold to the manifest. The bank's review label is fixture metadata, not
   fabricated independent human sign-off.
3. Construct C0 from table/column names alone and C3 from the frozen
   `fixtures/pack-recorded.json` analyst Pack used by these recordings.
   The live demo can evolve without silently rebinding historical responses
   to a different prompt. Both use identical question and instruction text. Gold,
   expected refusal and track labels never enter prompts. Each response is
   bound to its prompt hash, the same model id, seed and dataset hash.
4. Replay each SQL-bound PG protocol transcript through `sql-runner`,
   `query-result-adapter`, and `result-comparator`. Matching SQL text is **not**
   the scoring criterion: typed complete execution results are compared to
   frozen gold. Correctness ignores aliases, warnings and freshness; metadata
   remains in evidence. Numeric precision, duplicate rows and nulls use the
   existing comparator. Order matters only when `order_required: true`
   explicitly reflects the question. Truncated matching prefixes fail. This replay adapter
   cannot execute SQL or connect to a database.
5. Compute paired `trial-statistics` separately for accuracy, permission and
   injection. Fixed/broken lists and health stay separate. A missing response
   counts as no-output and a failure; planned denominators never shrink.
   Explicit policy refusal is output, not transport failure.
6. Emit a stable report with input, harness and tool hashes, fixed seed (42),
   pinned model label (`gpt-6.1-sol`), fixed/broken ids and per-arm health.
   More than 5% no-output in either arm suppresses its track's headline.
   Exactly 5% is allowed. Empty tracks and a zero C0 baseline have no relative
   change. Relative change means `(C3 passes - C0 passes) / C0 passes`, not
   percentage points. No LLM judge is used.

**Every replay headline remains null**, regardless of health. The bundled
responses are hand-authored protocol fixtures, not captured outputs from the
named model or live PG. They retain the original q02 SQL recordings as permission failures and
break q16; they do not estimate an ontology improvement. The permission and
injection fixtures test reporting only, not a real Gateway's enforcement.
Internal bootstrap diagnostics (including raw counts) stay in ignored
evidence; the public report contains no headline absolute accuracy.

## Frozen OO-Logistics ERP bank

`fixtures/logistics-bank.json` contains 40 accuracy questions (eight each for
effective-dated code decoding, legacy names, undeclared joins, calendar rules,
and metric definitions), plus separate permission and injection questions.
The existing demo questions remain in `fixtures/demo.json` (q02 is now
a permission case, with its original gold retained): they use a
different publication and are not pooled into ERP denominators.

The bank targets the default Self-build `owner` role on ten MySQL ERP tables,
with no foreign keys, from oo-datagen xs/seed 42. It does **not** claim TMS or
cross-source coverage. SQL and typed complete results are frozen before model
evaluation. Integer-fen sums avoid floating-point arithmetic; business-day
questions explicitly mean Monday-Friday without holiday exclusions. The
independent SQLite freezer reads synthetic CSVs and records their hashes.
Live gold execution is mandatory: SQLite alone is not Gateway acceptance.

```sh
python3 benchmarks/ab/logistics.py --dataset evidence/run/logistics \
  --output evidence/run/regenerated-bank.json
cmp benchmarks/ab/fixtures/logistics-bank.json evidence/run/regenerated-bank.json
python3 benchmarks/ab/logistics.py \
  --published-pack evidence/run/published-owner-pack.json \
  --receipt evidence/run/authenticated-query.json --output evidence/run/bound
oo-eval ab --arms c0,c3 --dataset evidence/run/bound/input.json \
  --output evidence/run/capture --profile logistics --oo /path/to/installed/oo \
  --model-endpoint-file /private/model-endpoint \
  --model-key-file /private/model-key --model gpt-6.1-sol
```

Accuracy questions require an explicit role, gold SQL and required columns.
The role-visible Pack is checked before trials; live capture additionally runs
every gold through the same authenticated profile and verifies its frozen
result before any model call. An accuracy refusal/permission denial raises
`accuracy_denial`: it is a harness error, not an incorrect answer or a
permission-track success. Invalid runs have no quality headline.

Binding fails if any required column is hidden. Do not change questions, gold
or role policies after seeing arm results. Supply the actual published Pack
and authenticated receipt; the bank does not fabricate either.

## Frozen manifest / BIRD handoff

`--dataset path/to/manifest.json` accepts a local manifest with the same shape
as `fixtures/demo.json`. Paths are relative to the manifest and must resolve
inside this worktree. All inputs are hash-pinned. Supported dataset identities:
`oo-logistics-demo-subset`, `oo-logistics`, `bird-mini-dev`. Only PostgreSQL is
accepted. Questions have:

- `id`, `question`, `track` (`accuracy`, `permission`, `injection`);
- `expected_kind` (`rows`, or `denied` on a security track);
- for accuracy, `role`, `gold_sql`, `required_columns`, and comparator-contract
  typed `gold`; optional `order_required` defaults to false.

Inputs require `schema` (table -> column names), `pack` (reviewed Pack JSON),
and `responses`. `bank` is optional for non-synthetic data: the existing
question-bank contract explicitly rejects non-public-synthetic distributions.
Do not relabel BIRD as synthetic to satisfy that validator. Freeze BIRD PG gold
under the typed result-comparator contract and pin all source artifacts as
additional inputs. Partition BIRD runs by database/schema/role; do not mix
database contexts in one manifest.

Response keys are `c0:<id>` and `c3:<id>`. Responses have a `prompt_sha256` and
one kind: `no_output`, `denied` with `code: POLICY_DENIED`, or `sql` with
`sql`, `sql_sha256`, and `transcript` (query-pg JSONL records as a JSON array).
The top-level response binding hashes canonical sorted compact UTF-8 JSON of
the manifest without `inputs`, avoiding a circular reference. Missing responses
remain planned failures; extra/unknown ids, hash or prompt drift, and mismatched
model/seed are rejected.

No BIRD files, licensed PG snapshot, frozen BIRD results, separate full
OO-Logistics reviewed Pack, or authenticated publication receipt are supplied
on this branch. Bare `--dataset bird-mini-dev` fails explicitly rather than
substituting invented BIRD questions. The manifest path supports a later
operator-supplied frozen dataset; **actual BIRD integration is not tested**.

## Original fixture acceptance boundary

The demo assets explicitly describe compilation/curation, not authenticated
publication. Reports therefore mark publication verification unmeasured.
The frozen fixtures do not establish live model or database measurements.
The optional capture mode above reads model files in place and executes through
an installed Gateway profile. Full OO-Logistics and BIRD acceptance still require
their own published Packs and hash-pinned gold; the demo alias never supplies
that coverage.
