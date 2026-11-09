# C0 / C3 paired evaluation

Repository-local CLI and deterministic scorer for V3-H-01. Python 3 (standard
library only) orchestrates the existing Rust benchmark programs. No product
workspace dependencies or lockfiles change.

## Run

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

`--dataset oo-logistics` selects the same **three-question public demo subset**,
not the full OO-Logistics bank. Report identity and coverage do not claim full
coverage. A new output directory under this worktree's ignored `evidence/` is
required. Existing evidence is never overwritten. Exit 0 means the pipeline
completed, not model quality, health, publication, or security acceptance.
Exit 2 is configuration/execution failure with a closed error code.

## What runs

1. Verify SHA-256 pins for schema, Pack, frozen bank, demo gold and responses.
2. Validate the public synthetic bank with `question-bank`; match its questions
   and gold to the manifest. The bank's review label is fixture metadata, not
   fabricated independent human sign-off.
3. Construct C0 from table/column names alone and C3 from the exact shipped
   analyst Pack. Both use identical question and instruction text. Gold,
   expected refusal and track labels never enter prompts. Each response is
   bound to its prompt hash, the same model id, seed and dataset hash.
4. Replay each SQL-bound PG protocol transcript through `sql-runner`,
   `query-result-adapter`, and `result-comparator`. Matching SQL text is **not**
   the scoring criterion: typed complete execution results are compared to
   frozen gold. Decimal precision, duplicate rows, nulls and ordering use the
   existing comparator. Truncated matching prefixes fail. This replay adapter
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
named model or live PG. They deliberately fix q02 and break q16 to test both
directions; they do not estimate an ontology improvement. The permission and
injection fixtures test reporting only, not a real Gateway's enforcement.
Internal bootstrap diagnostics (including raw counts) stay in ignored
evidence; the public report contains no headline absolute accuracy.

## Frozen manifest / BIRD handoff

`--dataset path/to/manifest.json` accepts a local manifest with the same shape
as `fixtures/demo.json`. Paths are relative to the manifest and must resolve
inside this worktree. All inputs are hash-pinned. Supported dataset identities:
`oo-logistics-demo-subset`, `oo-logistics`, `bird-mini-dev`. Only PostgreSQL is
accepted. Questions have:

- `id`, `question`, `track` (`accuracy`, `permission`, `injection`);
- `expected_kind` (`rows`, or `denied` on a security track);
- for rows, `row_comparator` and comparator-contract typed `gold`.

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

## Remaining acceptance

The demo assets explicitly describe compilation/curation, not authenticated
publication. Reports therefore mark publication verification unmeasured.
No authorized in-worktree model `secret_ref` was supplied; this executor does
not inspect token files or try an alternate environment-key transport. Live
model capture via the existing secret import, live read-only PG execution,
runtime publication binding, full OO-Logistics and BIRD runs remain follow-ups.
The fallback is useful pipeline evidence, not a completed real-data A/B study.
