# V3-I-07 wide decimals and installed acceptance

STATUS: **partial**. V3-I-07 is **not done**. Date: 2026-11-11.
ERP A/B cannot produce a valid comparison with the frozen bank and the
existing Gateway SQL contract: mandatory gold preflight rejects 23/40 queries.
Neither the bank nor the SQL policy was changed to avoid those refusals.

## Commits and product changes

- `127479529`: Decimal256 chunk target shape, COPY rendering and C5 digest;
  fix MySQL wide-coefficient carry propagation; Oracle typed mapping/handoff;
  exact Gateway JSON tests; owned MySQL/PG round-trip harness.
- `255e00fd5`: correct misleading calendar Pack text and reviewed demo month
  metadata, with compiled-content regressions and regenerated compiler snapshots.
- `fe2392340`: reuse validated model receipts on an explicit bounded resume.
  The six-call-per-attempt limit is unchanged.
- `a421ec242`: carry adjudication to host review, apply evidence decisions in
  the compiler instead of accept-all, verify confirmation handoff, and wire
  own-source profile/readiness and the actual wizard deliver screen.
- `8fe3cd0dd`: read facts/reviewed registries with the existing 64 MiB review
  bound when scheduling; credentials/configuration retain their 1 MiB bound.
- `95ec6877d`: keep private source file references through doctor/loader and
  preserve a validated CA file reference in the wizard.

No Cargo.lock change, shared patch request, source-data alteration, customer
access, push, PR, nested agent, or policy/gold relaxation. The pre-existing
untracked dispatch script remains untouched.

## Decimal evidence

The original `V3-I-06-shared.patch` is applied. Target column selection and
COPY rendering also needed Decimal256 support; C5 content digests use these
same functions. PG already produced Decimal256 through its common numeric
mapping. Existing exact numeric encoders/catalog admission/Gateway rendering
were retained and tested, not replaced with floats.

The first live test exposed a separate corruption bug: MySQL's multiply-add
and two's-complement loops shifted the old carry instead of the computed
wide intermediate. Maximum 40/65-digit values arrived as coefficient 255.
Both loops are fixed. The failed run remains
`evidence/v3-i07-wide-live-20261111/test.log`.

| Test | Result | Run |
| --- | --- | --- |
| MySQL 8.4.7 -> PostgreSQL 17, DECIMAL(40,12)/(65,30) | Five rows byte-exact: positive/negative maxima, zero, minimum fractional values, NULL | `v3-i07-wide-live2-20261111` |
| PostgreSQL 17.11 -> PostgreSQL 17, same columns | Five rows byte-exact | `v3-i07-wide-pg-20261111` |
| Chunk COPY/C5, precision 40/65/76 | Exact coefficients, both signs, zero, maximal scale, negative scale, duplicate-sensitive digest | `v3-i07-20261111/pipeline-final.log` |
| MySQL carry regression | Independent Arrow i256 oracle, byte-boundary values and maximal coefficients | `v3-i07-20261111/connectors-final.log` |
| Oracle typed mapping/hashing | Precision 40/65/76 and signed/max-scale cases pass | same connector log |
| Gateway exact JSON | Coefficient strings preserved at precision 40/65/76 | `v3-i07-20261111/workspace-touched-final.log` |

Live commands:

```sh
python3 tools/oo-load/test-wide-decimal.py --run-id v3-i07-wide-live2-20261111
python3 tools/oo-load/test-wide-decimal.py --engine postgres --run-id v3-i07-wide-pg-20261111
```

Oracle coverage is mapping/unit coverage, **not a live Oracle acceptance**.
The successful live harness compares source text with target numeric text,
not two conversions through the same encoder. Product source connections are
read-only; setup uses the separate fixture admin connection.
Negative-scale COPY remains exact, but a target typmod expanded beyond
precision 76 is still refused by the existing Gateway admission ceiling;
no broader numeric-policy relaxation is claimed.

## q16 diagnosis and fix

Reconstructing the prior C0/C3 prompts reproduces both captured prompt hashes
(`q16-diagnosis.json`). C0 generated `month_no = 1`; C3 generated
`EXTRACT(MONTH FROM day_date) = 1`. The latter is independently refused by
Gateway with `FUNCTION_NOT_ALLOWED`, argument types `[text,date]`; the former
validates. This is an execution-contract failure, not a wrong numeric answer.

The captured Pack said "The effective document has no calendar" despite
exposing `demo.calendar`, and called `month_no` a quantity without defining it.
The compiler now distinguishes missing business-calendar configuration from
stored calendar attributes. Reviewed demo metadata defines month numbers
1..12 and direct filtering, rather than addition or invented fiscal semantics.
These are observable misleading inputs; no claim is made about the model's
internal reasoning. Question text, gold SQL, gold answers and source rows are
unchanged. Compiled-content tests assert the definitions and runtime-function
caution. Live reruns generated the stored-month predicate and passed q16.

## Installed own-database acceptance

Fresh npm pack/install with real postinstall, empty HOME, owned MySQL 8.4.7,
original xs/seed-42 ERP fixture, SELECT/SHOW VIEW TLS account, original model
files referenced in place, requested model `gpt-6.1-sol`.
Agent/control databases are PostgreSQL 17.10.

Successful non-TTY flow: `v3-i07-own3-20261111`. Eight recommended tables
are recorded first; the two eligible maybes are then explicitly selected so
the frozen bank retains all ten tables. Self-build, adjudication, text graph,
hash-bound confirmation, publication, `erp` profile, readiness, three count
queries and MCP `check_answerable` pass. **Install to first answer: 213.924 s.**
The overall script is failed because its subsequent ERP A/B preflight fails.

| Stage | Seconds | Result |
| --- | ---: | --- |
| Fixture start / seed / TLS | 7.330 / 2.262 / 2.154 | pass |
| npm pack / install | 5.634 / 1.195 | pass |
| Recommendations / explicit selection | 1.875 / 0.159 | pass |
| Provisioning | 7.317 | pass |
| First build invocation | 111.717 | six-call budget pause; evidence retained |
| Explicit resume | 83.003 | Builder complete; confirmation required |
| Text graph / confirmation | 0.137 / 0.121 | pass |
| Publish through delivery | 7.824 | pass |
| Readiness | 0.315 | ready, ten visible tables |
| Three queries | 0.257 / 0.230 / 0.231 | pass |

Durable-stage observation (50 ms sampling, not internal spans):
load 1.863 s; candidate 0.523 s; Builder 108.820 s then 82.378 s;
confirmed review 0.060 s; publisher 0.644 s; Gateway 0.231 s;
token 0.059 s; share prompt 0.060 s. Model calls are 6 + 5, not just the
last-attempt count of 5. Failed attempts are not relabelled passes.

The **zh 120x40 full wizard** reaches a ready deliver screen and exits zero:
`v3-i07-own2-20261111/pty-evidence6/`. It selects ten tables, references model
files without importing them, explicitly authorizes bounded resume, confirms
the graph and saves profile `erp-zh`. Independent readiness is true and a
query returns customer count 200. Raw transcript SHA-256:
`e21f2f47a4387745f6c0809c8fdf90e0d025f1318393d7b735548cbd79726e22`.
Launch-to-first-answer is bounded by 297.974 s, including operator observation
time at the deliver screen; query execution is 0.276 s. It is not a clean
installation latency benchmark. PTY load/candidate: 1.892/0.523 s; Builder:
110.620 + 86.856 s; publish: 0.638 s; Gateway: 0.317 s. Faster transitions
below the observer interval are not assigned invented durations.

Earlier PTY attempts and failed onboarding receipts remain. One PTY attempt
lost its pinned image after this executor retagged the build; the final
attempt retained an explicit image tag until cleanup. Another failed at
file-reference preflight; the product fix and successful rerun are separate.

## A/B and the remaining blocker

Demo captures `v3-i07-demo-ab-20261111` and `v3-i07-final-ab-20261111` use
authenticated published Packs, seed 42, both arms and unchanged gold.
Both replay to byte-identical reports.

| Track | C0 | C3 | Relative change | Fixed / broken | No-output C0/C3 |
| --- | ---: | ---: | ---: | --- | --- |
| Demo accuracy | 2/2 | 2/2 | 0% | none / none | 0/0 |
| Demo permission | 2/2 | 2/2 | 0% | none / none | 0/0 |
| Demo injection | 1/1 | 1/1 | 0% | none / none | 0/0 |
| ERP accuracy | not run | not run | unavailable | unavailable | unmeasured |

Security A/B outputs are model refusals, not Gateway-enforcement evidence;
the separate QS audited-denial test covers enforcement.

Both own-database publications fail live ERP preflight with
`accuracy_not_answerable` before model trials. A separate exhaustive
40-query run identifies **17 exact gold matches and 23 policy refusals**:

- `lg01`..`lg08`, `lg28`..`lg31`: date/text comparison signatures.
- `lg25`..`lg27`, `lg32`..`lg35`, `lg37`..`lg39`: unsupported general casts.
- `lg36`: unsupported subquery.

Raw results: `v3-i07-own3-20261111/gold-preflight/`; independent comparison
also retained under own2. Frozen bank and Gateway policy hashes are identical
to the starting commit (`erp-contract-binding.json`). Replay exits 2 because
there is no valid ERP capture. No arm scores, health headline or byte-identical
ERP replay are claimed. Completion needs a separately reviewed SQL-capability
extension or owner-approved bank correction; this run does neither.

## Regression, image and cleanup

Evidence root: `evidence/v3-i07-20261111/`.

- `cargo check --workspace --locked` before every code commit: pass.
- Full tests for connectors, pipeline, Gateway, Builder, CLI and eval:
  **1,401 pass, zero failures, 141 ignored**. Ignored is not passed.
- Connector library with `mysql,oracle`: 274 pass, three ignored.
- oo-load: 47 pass, eight ignored; mysql-doctor: one pass.
- Exact CLI/compiler adjudication: pass, including hidden-column application
  and changed-byte refusal. Retained Builder: 12 pass; budget: 14 pass.
- Lifecycle: 24 pass; demo assets: ten pass; eval: 30 Python tests pass.
- oo-test Python regression: 186 tests, one skipped. npm/staging:
  52 pass, one skipped. Formatting, relevant clippy, and spec/plan checks pass.
- No pre-existing-failure exemption was needed. Initial compile/test/harness
  failures remain in evidence alongside the successful corrections.

The lite image is rebuilt with the checked-in Dockerfile, release profile,
runtime assertions/overflow checks, linux/arm64 and `oo.lane=v03`. Local npm
binaries are debug-native darwin-arm64. Measurements use warm local images,
not cold downloads, release-native CLI performance, percentiles or other hosts.
`source_time=unmeasured` remains explicit; no customer or chatbot acceptance.

Final image `open-ontology/lite:v03-arm64`:
`sha256:55a7c4a97868beede4d0fe0994e4489d679869eb4d3d081fbf64c0d3d540e038`.
`image-final.json` binds it to product commit `95ec6877d`, the Dockerfile hash,
platform and lane label. No Rust-build substitution was used in this rebuild.

Final isolated installed run `v3-i07-accept-qs-20261111` passes **QS-01..05**
on PostgreSQL 17.10. Install 1.224 s; deployment 14.623 s; first query 0.224 s;
install-to-first-answer **16.071 s**; down 0.877 s. The three generated MCP
configurations perform readiness, query and answerability. The expected
denial is independently read back from the audit database. Resources before
and after are identical. An earlier overlapping run failed that isolation
comparison while another owned fixture changed; it remains recorded as such.

Final live demo `v3-i07-accept-ab-20261111` again has 2/2 accuracy, 2/2
permission and 1/1 injection in both arms, 0% relative change, no fixed/broken
questions and zero no-output events. Capture takes 20.721 s; replay
`v3-i07-accept-replay-20261111` is byte-identical. q16 passes both arms.

All task-created source/lite/demo containers, networks and volumes are absent.
Retained volumes were separately ownership-checked before removal. The
requested rebuilt image remains; the intermediate acceptance alias was
removed. Generated credentials, runtime state, npm installations/caches and
PTY tooling were hashed then removed, preserving public-synthetic receipts,
failures, transcripts and reports. `final-owned-cleanup.json`,
`source-resource-absence.json` and `final-demo-resource-absence.json` record
scoped checks without unrelated resource names.

Final `scan-evidence.py` uses the shared private-file reader for exact model
material and scans credential signatures and private terms: **2,069 files
in 16 task evidence directories; zero hits**. Original model files were
referenced in place and never copied or modified.
