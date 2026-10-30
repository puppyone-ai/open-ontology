# CLI retrieval and answer evaluation (development harness)

No new dependencies. Python 3.10+ orchestrates isolated questions; Node 24+ imports the **installed** Demo Pi packages from `demoDir`. The runtime uses the Demo's Pi Agent, sequential tools, `streamSimple` OpenAI transport, medium reasoning, no provider retries, and one `oo` tool (no shell, direct DB tool, legacy fallback or oracle exposure). It does not run inside the production sandbox and does not reproduce Demo's final grounding correction turn: provenance is scored independently. This is a development evaluation adapter, not the production `eval.*` service in docs/13.

## One Gateway + one Pack

Keep configuration, questions and output **outside Git**. Configuration example (all names generic):

```json
{
  "demoDir": "/opt/demo",
  "cliCommand": ["/opt/oo/bin/oo"],
  "gateway": "http://127.0.0.1:8080",
  "transport": "local-development",
  "tokenFile": "/private/role.token",
  "bearerFile": "/private/model-bearer",
  "model": "devin/claude-sonnet-5-5",
  "modelEndpoint": "http://127.0.0.1:8317/v1"
}
```

For relative-date business questions, supply both `currentDate` (valid
`YYYY-MM-DD`) and `timezone` (IANA zone) in this private config. They enter the
system clock context, not the user's question. No table names, chosen metrics
or oracle results are added to the prompt.

An optional operator-confirmed scope avoids repeated organization questions:

```json
{
  "businessScope": {
    "tenant_id": "101",
    "company_ids": ["1001"],
    "coverage": "local_imported_snapshot_only"
  }
}
```

These example IDs are not defaults for other users. Only these three keys,
numeric-string identities and the local-snapshot coverage are accepted.
The scope does not grant access or approve metrics; Gateway policy still governs
every query. It is currently evaluation context, not a production session feature.

The default `queryAccess: "aggregate_only"` remains a trace-privacy restriction.
An authorized operator may explicitly set `queryAccess: "gateway_readonly"` for
bounded dictionary/detail lookups needed by business questions. This removes only
the evaluator's aggregate-only filter: all queries still use the configured CLI,
token and Gateway SQL/role enforcement. It does not confer administrator rights,
permit writes, or authorize querying unrelated personal records. Such traces may
contain sensitive values and must remain outside Git. The selected mode is pinned
in the run's configuration digest and injected into the system prompt.

Set `gatewayManifestSha256` to the exact SHA-256 of the live `manifest.json` text
for release acceptance. The adapter authenticates and checks it before starting
the model and again before returning its answer. Drift or access failure is
`GATEWAY_BINDING_DRIFT`; no model assertion or local preview substitutes for that
check. `runtime_binding` is separate from the model's tool trace and does not count
as a model knowledge lookup or a successful data query.

## Explicit Private Skill Binding

Generic evaluation remains the default; it has no customer-specific path or
Skill dependency. A private operator config can explicitly select a Skill:

```json
{
  "cliCommand": ["/opt/runtime/bin/oo"],
  "gatewayManifestSha256": "<64 lowercase hex characters>",
  "agentSkill": {
    "source": "/private/approved/SKILL.md",
    "delivery": "/private/delivery/SKILL.md",
    "installed": "/private/installed/SKILL.md",
    "sha256": "<SHA-256 of the exact approved UTF-8 Skill bytes>",
    "cli": {
      "declaredPath": "/opt/customer/bin/oo",
      "sha256": "<SHA-256 of the actual /opt/runtime/bin/oo file>"
    }
  }
}
```

These fields extend the normal config; they are not a standalone runnable
configuration. All paths must be absolute and normalized. All three Skill
files must be readable, nonempty UTF-8 regular files (at most 1 MiB) and match
the same pinned SHA. With an explicit read-only mount they may name the same
file; this records a mount, not an independent packaging/install receipt.
The loader injects the **installed bytes verbatim**, after generic guidance.
The selected Skill governs business behavior; the original JSON output,
real `query_ids`, privacy and scoring requirements remain in force. Tool
mapping is explicit and never rewrites the original Skill.

Explicit selection fails closed for null/unknown fields, missing files,
permissions, nonexecutable CLI files, path errors and hash drift. There is no
fallback to the generic policy. Source, delivery, installed and CLI files are
rechecked before tool execution and after the answer. The Python runner also
rechecks the evaluator before and after every question, and the Node caller
compares the prepared prompt binding with the Python preflight.

The selected-Skill mode supports either a single absolute executable or
`["/absolute/path/docker", "exec", "<full 64-hex container ID>", "/absolute/oo"]`.
For Docker, source/delivery are host files; installed is a container path.
The runtime reads the installed Skill with `cat`, checks regular-file and
read/execute permissions, and hashes the actual container CLI with `sha256sum`.
The host launcher is separately hashed. The container must supply those tools.
Shell wrappers, mutable container names, extra argv and remote-exec prefixes
are rejected in this mode; legacy generic argv prefixes remain supported.
An executable-file hash does not attest to its transitive dependencies or an
image signature. Freeze the runtime/image separately for release acceptance.

`run-binding.json` records only prompt/Skill hashes, paths, byte counts and CLI
mapping metadata, never Skill/prompt bodies or credentials. Each trace records
`prompt_binding`, before/after `runtime_binding`, and wire-level system-prompt
hashes from the actual model payload callback. A missing or altered wire prompt
cannot pass selected-Skill evaluation. `evaluator_sha256` covers evaluator code,
all injected files, config, prepared prompt, selected files and observed CLI
inputs. `runtime_evaluator_sha256` additionally binds the before/after runtime
and hashes of actual query/freshness receipts (`runtime_data_binding`), including
failed receipts. Existing trace SQL/results remain private and unchanged.

Local binding inspection (no model, credentials or Gateway request):

```sh
python3 tools/eval-harness/harness.py inspect-binding \
  --config /private/approved-eval.json --output /private/new-binding.json
```

Docker inspection **does** invoke the configured Docker daemon to read/hash
runtime files; it is not schema-only inspection. It does not invoke `oo`.
Do not run it before runtime access is authorized. `run` uses this same loader,
not a separate parameter-only path. `replay --config ...` is required for
selected-Skill traces and revalidates current files, evaluator and receipt
hashes; it cannot turn old traces into a new live run.

The legacy runtime guards pin manifest text plus policy/knowledge versions.
Known cross-version query receipts cannot be combined, partial results cannot
pass full-answer scoring, and structured errors never count as zero/success.
Those legacy pins alone do not detect every data-only change. Enable F22
explicitly for the additional authenticated publication/data-vector contract
below. Source-time freshness and cross-table consistency remain independently
measured (or unmeasured).

Private Skills are mounted/delivered by the operator, never copied into public
npm packages by this harness. CLI `agent-skill` still exports its generic
contract; it must not be represented as shipping the selected private Skill.

Local regressions, including an optional **installed Pi Agent** roundtrip with
a synthetic loopback SSE server and synthetic CLI (no real model or source):

```sh
node --test tools/eval-harness/test_*.mjs
python3 -m unittest discover -s tools/eval-harness -p 'test_*.py'
EVAL_OFFLINE_DEMO_DIR=/absolute/installed/demo \
  python3 -m unittest discover -s tools/eval-harness -p test_real_agent_offline.py -v
```

The last test skips unless explicitly given the installed SDK directory. It
checks the actual wire system message on all three model-fixture turns and
retains real synthetic query IDs through scoring; it is not real-LLM acceptance.

## F22 Whole-Question Publication Binding

This opt-in is independent of private Skill selection; omitted configuration
keeps generic legacy evaluation unchanged:

```json
{
  "publicationBinding": {
    "mode": "f22",
    "allowWholeQuestionRetry": false
  }
}
```

Only `mode: "f22"` and the optional boolean are accepted. Null, misspelled keys,
unknown modes and nonboolean retry settings fail before model/key loading.
The default is **no automatic recomputation**. No operator-supplied live
configuration is auto-migrated or silently enabled.

The first per-question runtime check sends `--expected-publication-id current`
without a vector: `knowledge cat manifest.json` when a manifest hash is pinned,
otherwise `freshness --summary`. It requires the authenticated successful
CLI JSON `publication_binding` with `publication_id` (`pub1_` plus 64 lowercase
hex characters) and `data_vector_sha256` (64 lowercase hex characters).
Every subsequent query, tables, describe, knowledge cat/search, validate and
freshness call, including final runtime verification, sends BOTH exact
`--expected-publication-id ID --expected-data-vector-sha256 VECTOR`.
Every successful read must acknowledge the same pair. Missing, malformed or
mismatched receipts fail closed, including old servers. The model cannot
override these values. Offline `--help` is exempt; `status` is unsupported in
F22 mode rather than issuing an unbound read.

The sole automatic recomputation signal is the fixed CLI flat error:
process exit 2, `class: "gateway"`, `code: "CONFLICT"`, `exit: 2`,
`retryable: false`, hint tokens `reason=PUBLICATION_DRIFT` and
`action=RECOMPUTE_WHOLE_QUESTION`, with NO rows or observed binding.
The Agent aborts; its final answer and every intermediate result are excluded
from answer delivery. Original query IDs, exit status, failed receipts and
partial traces remain forensic evidence, never inputs to the next attempt.
`retryable: false` is respected by the CLI/SDK: no SQL is transparently replayed.

Only when `allowWholeQuestionRetry: true`, Python may start **one** new Node
subprocess / cold Pi Agent with the original question and unchanged frozen
config/Skill/CLI/prompt. It reacquires `current` and reruns ALL component
queries. The model receives no previous conversation, partial sums or error
history. Two attempts is the hard maximum. Any authentication, input, timeout,
protocol or other failed call in the attempt prevents automatic recomputation;
hint text by itself is insufficient. Model failures, changed Skill/CLI/prompt
and manifest/semantic pin mismatches also cannot authorize retry. Existing
per-attempt tool/time budgets are unchanged, not silently raised.

Each F22 question writes immutable `0001-attempt-01/{trace,binding}.json` and,
if authorized, `0001-attempt-02/{trace,binding}.json`. The second binding links
the first trace SHA. `0001-trace.json` is written once with only the selected
attempt's answer/trace plus attempt references; it never merges query results.
`results.json` reports recomputation count and ALL attempt failure categories
even if the final cold recomputation passes. Output roots must still be new.
Attempt creation and every evidence write are exclusive, not overwrites.

`binding_checks` retains acquisition/final-check receipts, exit codes, expected
pins and full-output hashes, but not the manifest body. The evaluator digest
covers the F22 policy/code; the runtime digest binds actual ID/vector and
receipt evidence. Replay of F22 evidence requires `--config` and checks the
same per-read binding chain, just as selected-Skill replay checks prompt pins.

Offline data-only drift tests (installed Pi, synthetic loopback SSE and CLI):

```sh
EVAL_OFFLINE_DEMO_DIR=/absolute/installed/demo \
  python3 -m unittest discover -s tools/eval-harness -p test_f22_pi_offline.py -v
```

These prove the consumer contract and cold orchestration, not real Gateway/PG
atomicity or business acceptance. Real integration requires a newly owned,
ready Gateway/PG fixture and operator-frozen config. A stopped owner fixture,
old public Gateway, or an earlier real-model smoke is not F22 acceptance.

## Model And CLI Runtime

In generic mode, `cliCommand` may be an operator-owned argv prefix such as `docker exec <gateway> /opt/oo`. Model arguments never become shell commands. Credential contents are loaded only inside the runtime for authentication, never printed or included in traces. Use HTTPS + `caFile`/production transport outside loopback development. Existing Gateway authorization remains the security boundary.

For an external compatible model endpoint, set `modelEndpoint`, `model`, and
`apiKeyEnv` (for example `"GEMINI_API_KEY"`) instead of `bearerFile`. A missing
named environment variable fails closed; no credential fallback or automatic
credential persistence occurs. `modelContextWindow` and `modelMaxTokens` are
operator-configured limits, defaulting to 128000 and 8192. These declarations do
not verify a provider's actual context capacity or upstream model identity.
Traces record assistant stop reasons, token counts and bounded failure categories,
not raw provider errors or reasoning. Configuration and model-list evidence are separate from
successful tool use and business-answer acceptance.

The product provider probe (`../pg-builder/scripts/model_provider.py`) can
add `require_tools: true` in its private **snake_case** provider config to test
a synthetic non-streaming tool call and tool-result roundtrip, not just "OK".
This is separate from this harness's **camelCase** config and actual streaming
CLI conversations. Neither probe alone proves long-session reliability.
The evaluator now preserves its own `AGENT_TIMEOUT` / `TOOL_BUDGET` reason
when an abort produces a later model event; old generic failure receipts remain
unchanged and must not be retrospectively treated as known root causes.

Wire diagnostics now include `model_requests`: per-request relative timestamps,
HTTP status when the transport response hook runs, message roles/counts/byte sizes
and SHA-256 digests, tool-schema digest, streaming and token-parameter shape.
`model_events` links to the request index and records safe finish-reason categories,
error digests and text/thinking byte counts, never raw reasoning or provider errors.
`stream_incomplete` distinguishes streams missing their final finish reason.
Message counts and stable prefix digests can test whether history was sent; usage
token counters alone cannot establish context loss. A missing HTTP status is
unknown, not a fabricated network status. Existing SQL/results in `trace` remain
sensitive: keep the entire output outside Git even though these new fields omit
message bodies and headers. Freeze a new evaluator before any new business run;
old traces and their failure classifications remain immutable.

Freshness tools now require a CLI supporting `oo freshness --summary`. This
explicit view preserves all warnings, source-time status, versions and unknown
fields, replacing known evidence arrays with counts and state counts. It marks
omitted details and never claims source recency or cross-table consistency.
The original `oo freshness` remains the complete audit view. This prevents a
whole-library evidence dump from consuming an Agent's context budget.

```sh
python3 tools/eval-harness/harness.py run \
  --config /private/eval/config.json --questions /private/eval/questions.json \
  --output /private/eval/run-001 \
  --table-threshold .90 --answer-threshold .80 --refusal-threshold 1.0
```

Sequential (concurrency 1), at most 60 tool calls / 10 minutes per question; after runtime/model failure waits 65 seconds, no automatic retry. Output directory must be new. Exit 2 means a threshold failed or is unmeasured; thresholds never relax. `results.json` has scores and failure distributions; `summary.md` is a Chinese-labelled summary; numbered traces contain complete tool inputs/outputs and final structured answers, **not thinking**. Queries are restricted to conservative simple aggregate projections and grouping keys before execution. Unsupported expressions are rejected locally (`EVAL_AGGREGATE_ONLY`), distinct from Gateway refusals. The guard is a privacy precaution, not a SQL authorization parser. Only use trusted, role-scoped Pack content without source-row examples when recording customer traces.

Question format:

```json
{"schema_version":1,"questions":[
  {"id":"Q01","text":"How many rows?","kind":"numeric",
   "expected_tables":["app.items"],"expected_rows":[[3]],
   "oracle_sql":"SELECT COUNT(*) FROM app.items"},
  {"id":"Q02","text":"Explain freshness","kind":"freshness",
   "required_terms":["source_time","unmeasured"]}
]}
```

Kinds: numeric, grouped, refusal, no_data, prediction, freshness. Legacy question sets must be enriched with explicit kinds/expected table sets; free-text expectations are not gold answers. Oracles/expectations never reach the Agent. Numeric comparison normalizes decimal strings, preserves nulls and duplicate groups, ignores row order, and requires cited nontruncated Gateway result rows to support every submitted result. A missing oracle is **unverified**, never pass. Top-N must retain the grouping-column order used by its oracle. Empty result arrays are distinct from refusal.

`kind: "business"` is an exploratory cold-question track for unqualified
industry language. Keep the original question unchanged. Its score separates
successful query receipts, model-declared resolution and business delivery.
Clarification, semantic gaps, partial results and safe refusals remain blocked;
a numeric result without a reviewed business contract and independent oracle
remains unverified, even if its number appears in a query. This track never
auto-certifies semantics. After independent contract review, author a numeric
or grouped acceptance case with an oracle, retaining the original business
wording and the failed exploratory traces.

The Agent now distinguishes `clarification_needed`, `semantic_gap`,
`execution_failure`, and `partial` from actual `no_data`. These are not new
Gateway error codes. Legacy refusal/no-data scores remain proxies for old
technical cases, not business acceptance. Structured CLI errors invalidate
receipts even when process exit is zero.

The aggregate privacy guard admits single-column `COUNT(DISTINCT column)` as
well as plain counts. It still rejects raw projections, multi-column DISTINCT,
DISTINCT expressions and `SUM(DISTINCT ...)`. This local guard is not a claim
that the running Gateway accepts every admitted shape; validate the complete SQL.

Retrieval recall measures expected identifiers **returned** by knowledge/tables, not SQL guesses; first-hit counts all previous calls. Extra returned identifiers measure retrieval noise; unrelated describes measure active exploration. A full tables listing can trivially give high recall: always inspect both noise metrics. Discovery is lexical and requires schema-qualified names; aliases/Markdown references need human review.

Refusal scoring checks structured conclusion, an evidence tool call, and absence of result rows; freshness checks required phrases plus a freshness call. These are **proxies**, not semantic proof. Natural-language numeric claims, JOIN correctness, metric semantics, precise permission/refusal rationale, dictionary translation and unsupported numerical paraphrases require independent human review. Error feature categories are heuristics, not confirmed root causes. Do not equate structured-answer accuracy with audited end-to-end business accuracy.

## Candidate generation

```sh
python3 tools/eval-harness/harness.py generate \
  --oos /private/eval/effective-oos.json --pack /private/eval/published-pack.json \
  --output /private/eval/candidates.json
```

The required `--pack` is the exported role Pack (`files` with `path`, `text`, and
`sha256`). Generation intersects effective metadata with published columns and
exact relationship endpoints, verifies file digests, and records `pack_digest`.
Both `relationships.md` and compiler-produced `relationships/*.md` pages are
read; a navigation link alone never counts as a published edge.
Pending effective relationships never become published JOIN questions. Operators
must separately verify that this artifact matches the live Gateway.
Generated JOIN prompts name the published relationship to disambiguate multiple
roles between the same tables. They test execution, not blind key discovery;
blind discovery requires separately authored prompts without table/key hints.

Optionally add `--metadata /private/eval/metadata.json` to merge `tables[].column_facts` (name, data_type, enum_ref) into **already role-visible** OOS columns; hidden columns are never added. Declared dataset domains label cross-domain joins. Consumes effective OOS `datasets` (schema-qualified dataset, columns) and `relationships` (`from`/`to` dataset + columns). Produces deterministic drafts for counts, date ranges, declared enum distributions, two-table joins, directed three-table chains, Top-N, unknown-data, permission and prediction cases. It never invents a relation. SQL uses quoted identifiers. Import paths from README/domain/entity/example assets are not yet parsed; dictionary semantics and cross-domain business questions still need review. Generated questions are drafts, **not approved gold** (docs/13 §9.2). Independent oracle execution and normalized metadata mapping are described below.

## Tests

```sh
python3 -m py_compile tools/eval-harness/harness.py
node --check tools/eval-harness/agent.mjs
python3 -m unittest discover -s tools/eval-harness
node --test tools/eval-harness/test_agent.mjs
```

### Independent aggregate oracles

Business contracts can use `local_snapshot_reconciliation` instead of a
`source_report`/`finance_report` when only the existing local import is in scope.
This is not a source-access requirement or permission to import again.
`context.coverage` must be `local_imported_snapshot_only`; `oracle.local_snapshot`
must carry SHA-256 fields `import_manifest_sha256`, `oracle_execution_sha256`,
`definition_query_sha256`, and a different `independent_query_sha256`, together
with `same_snapshot_verified: true` and `source_completeness_claimed: false`.
Existing approved-definition, Pack-read, context, freshness, query-receipt and
exact-result gates remain. Hashes and flags are trusted examiner inputs, not
proof of authenticity or independent reasoning. Audit the actual executions.
Do not assert same-snapshot merely because both queries reached one database.
This can certify scoped local arithmetic, not full source-history coverage.

```sh
python3 tools/eval-harness/harness.py oracle \
  --questions /private/eval/candidates.json \
  --metadata /private/eval/oracle-mapping.json \
  --config /private/eval/oracle-config.json --output /private/eval/questions-with-oracles.json
```

Oracle configuration: `{"psqlCommand":["psql","service=local_eval_copy"]}`. Use an operator-approved **local Agent DB copy**, never a source database. Credentials belong in the existing PostgreSQL service/credential mechanism, not argv or this repository. The command opens one `REPEATABLE READ READ ONLY` transaction, sets UTC and per-statement 10s timeout, obtains only aggregate JSON results and rolls back. It never logs stderr or source rows. It requires trusted single-SELECT aggregate oracle SQL; this is not an untrusted SQL sandbox. Source/Gateway snapshot identity must be frozen and recorded by the operator; a transaction alone cannot freeze the separate Gateway.

Optional normalized physical mapping:

```json
{"tables":[{"logical_table":"app.items","oracle_table":"_oo.replica_1",
             "columns":{"id":"c_1","category":"c_2"}}]}
```

Mapping is for **generator-emitted quoted SQL**, not arbitrary SQL rewriting. Omit it if the independent copy uses logical schema names. Native catalog/OOS adapters must be supplied/reviewed by the operator; no customer catalog structure is hardcoded. Raw columns, profiling samples, data-source credentials and hidden-role metadata must not enter generated public fixtures. `generate` emits drafts; `oracle` enriches them but does not approve their semantics. Entity/value/code translation and cross-domain coverage remain analyst review responsibilities.


### Offline replay

`replay` re-scores existing numbered traces without a model call or Gateway query. It can recover a trailing JSON answer after prose, without rewriting the original evidence:

```sh
python3 tools/eval-harness/harness.py replay \
  --questions /private/eval/questions.json --traces /private/eval/run-001 \
  --output /private/eval/replayed-results.json
```

All output subcommands reject paths inside this worktree. The output files are created exclusively (never overwrite evidence). Retrieval is macro-averaged over questions with expected tables; denominator counts and exploration averages accompany the summary. Prose without a recoverable structured answer remains unverified, rather than being silently interpreted as a correct result.

Numeric-looking grouping identifiers (for example `"001"` versus `"1"`) stay distinct. Numeric columns are inferred from native oracle numeric cells, or declared with zero-based `numeric_columns`; single-value `numeric` answers normalize their cells as decimals. If an oracle encodes a grouped decimal measure as a string, declare its column index explicitly. No arbitrary floating tolerance is assumed.

### Contracted business acceptance

`kind: business` without a trusted `business_contract` remains discovery-only;
safe refusal and a raw SUM are not business delivery. A private examiner-owned
question may include:

- `business_contract.approval`: approved status, reviewer, definition SHA-256.
- `context`: exactly metric, scope, period, unit, time_basis, coverage.
- `oracle`: kind (`source_report` or `finance_report`), evidence_sha256,
  context_sha256, rows_sha256 and `reconciled: true`.
- `knowledge_files`: required CLI knowledge paths and their body SHA-256 values.
- `expected_rows`, plus `numeric_columns` for string-valued grouped measures.

`business_contract.digest` defines the canonical context/rows hash. Keep monetary
values as exact decimal strings. The reference evidence must be independently
obtained and actually reconciled; these fields do not authenticate an approver
or validate a source report. Never manufacture approval flags or hashes.
The existing Agent-DB `oracle` command does not produce business certification.
No contract or reference answer is sent to the model.

The model's `business_context` must match. Required knowledge files are checked
against actual CLI body bytes; changed/missing files, stale/missing freshness,
wrong scope/unit/period, mismatched numbers, incomplete query receipts and forged
citations all prevent `verified_against_contract`. Human review of prose,
SQL semantics and evidence remains mandatory before release.

Business runs require 100% answer accuracy. Retrieval and refusal thresholds
apply only when corresponding cases exist; absent denominators remain null,
not passed. `run-binding.json` records the evaluator/prompt digest, question-file
digest and clock. Evaluator changes during a run fail with `evaluation_input_drift`.
Historical failed evidence is not rewritten by adding a contract afterward.

### Owned F22 consumer integration

`run_f22_owned.py` connects the actual installed Pi SDK and native CLI to a
new owned standalone Gateway/PostgreSQL runtime produced by
`crates/oo-gateway/tests/f22_runtime.py start`. The model endpoint is a local
deterministic SSE fixture, not a real model or a customer-source test.

From `product/`, use a new external output directory for each scenario:

```sh
python3 tools/eval-harness/run_f22_owned.py \
  --runtime /private/owned-f22/runtime.json \
  --output /private/new-consumer-evidence \
  --demo-dir /private/installed-pi \
  --skill /private/selected/SKILL.md --scenario once
```

Scenarios are `same`, `once`, `no_retry`, `repeated`, `auth`, and `input`.
The hook checks runtime ownership and only applies its bounded synthetic
data-only mutation; no arbitrary SQL or customer endpoint is accepted.
Two real query receipts must share the acquired publication/data vector.
The `once` case requires a cold whole-question restart, a new data binding,
unchanged semantic versions, and no prior query ID in the delivered answer.
Authentication and input failures never authorize a whole-question retry.
An ordinary SQL syntax error may remain repairable in the same bound question;
an incomplete final result must still fail scoring.

The runner leaves the owned Gateway/PG running for the operator's next test.
Use its `stop` hook after collecting results; preserve failed attempts and
do not treat a retained runtime JSON file as proof the processes remain alive.
