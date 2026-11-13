# V3-I-09 residual validation

STATUS: **partial**. Date: 2026-11-11.

The installed Oracle flow and native/offline and emulated demo checks complete,
including a real emulation defect fix. **lg02/lg08 are not closed.** The two
new A/B rounds are worse for C3; wording changes are not an accuracy fix.
Oracle rejects the requested literal `NUMBER(p>38)` declaration. A valid
wide, negative-scale NUMBER is tested separately, not substituted silently.

Implementation commits:

- `1f8419feb`: distinguish dictionary lookup from enum certification.
- `dbc8e9a82`: explicitly select the pinned image's platform for demo commands.

Primary evidence: `evidence/v3-i09-20261111/`. All paths below are relative
to `evidence/`. No public release, production acceptance, customer access,
push, PR, or nested agent is claimed.

## Dictionary diagnosis and A/B

`baseline-diagnosis.json` reconstructs and verifies the captured I-08 C0/C3
prompt hashes. The two C3 responses refuse **before Gateway execution**.
All five dictionary columns are visible, but the published Pack has no
dictionary discriminator binding, effective-interval definition, or exported
enum. The source catalog has empty table/column comments; the new Self-build
draft explicitly says the column business meanings are unknown.

The compiler previously described every empty certified-asset collection as
a readiness gap. Its guidance now distinguishes visible stored dictionary
rows from enum certification, and application role codes from authorization
grants. Compiled-Pack tests check that distinction while retaining the
prohibition on invented meanings. Derived snapshots/demo Packs are refreshed;
ERP questions, gold, original CSVs, demo seed/roles and reviewed inputs remain
unchanged (`gold-preserved.json`, `regeneration.json`).

This repairs misleading guidance, **not the missing business bindings**.
The model's internal reason is unobserved; absent definitions are an evidenced
gap, not a proven explanation of its hidden reasoning. No gold-derived
dictionary meanings, question edits, answer patches or relaxed comparisons
were introduced to manufacture a pass.

`python3 evidence/v3-i09-20261111/mysql.py` installs a fresh private npm package,
seeds the unchanged public ERP data through an admin connection, completes
Self-build/adjudication/confirmation/publication, then runs the real
`benchmarks/ab/run.py ab --arms c0,c3` twice. Model: `gpt-6.1-sol`;
source dataset seed: 42; model seeds: 42 and 7.

| Model seed | C0 accuracy | C3 accuracy | Relative change | Permission C0/C3 | Injection C0/C3 |
| --- | ---: | ---: | ---: | --- | --- |
| 42 | 20/40 | 16/40 | -20% | 1/1 / 1/1 | 1/1 / 1/1 |
| 7 | 20/40 | 15/40 | -25% | 1/1 / 1/1 | 1/1 / 1/1 |

- Seed 42 fixed: `lg14, lg24`; broken: `lg03, lg05, lg07, lg08, lg09, lg11`.
- Seed 7 fixed: `lg14, lg24`; broken: `lg04, lg05, lg06, lg07, lg08, lg10, lg11`.
- C3 refuses both lg02 and lg08 in both rounds. C0 fails lg02 and passes lg08
  in both new rounds. Thus lg02 disappearing from a broken list is not a fix.
- No-output counters are zero for both arms in every track.
- `v3-i09-mysql-20261111/ab-{42,7}/scored/report.json` each equals its
  `replay-{42,7}/report.json` byte for byte. `ab-summary.json` records hashes.

Source: MySQL **8.4.7**; serving PostgreSQL **17.10**. Install to first
answer: **216.118 s**, warm images. Query receipts remain publication-pinned.
`source_time=unmeasured` and per-table/non-atomic consistency remain explicit.

## Oracle 23 live onboarding

Pinned `gvenzl/oracle-free:23.5-slim`, native arm64, Oracle **23.5.0.24.07**.
Run: `v3-i09-oracle5-20261111`; installation and initial attempts are retained
in `initial-result.json`/`result.json`, successful continuation separately in
`recovery-result.json`. The same fresh installed package and profile complete
discovery, real model Self-build, adjudication, graph confirmation, publication,
profile enrollment, `oo check`, three queries and MCP `check_answerable`.

The earlier failures are not relabelled successful:

- The Oracle adaptation must allow NULL for an empty MySQL SET string.
- A harness-only version-suffixed engine was corrected to canonical `oracle`.
- Source trace identifies `ORA-00904` on
  `DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER`: the fixture reader lacked an SCN
  source. The separate fixture admin grants **SELECT on SYS.V_$DATABASE**;
  resumed product loading succeeds without broad table/write privileges.

| Stage | Seconds | Result |
| --- | ---: | --- |
| npm install | 1.281 | pass |
| recommendations / explicit all-table selection | 3.474 / 0.634 | pass |
| lite up | 9.215 | pass |
| initial build before SCN grant | 6.595 | blocked |
| resumed build phases | 98.390 / 126.978 | budget/confirmation pauses retained |
| model requests, 12 calls | 205.417 total request time | real model |
| first request to final completion | 209.052 | measured usage timestamps |
| adjudication after review input | 3.378 | file timestamp interval, includes cut recheck |
| graph / confirm | 0.109 / 0.105 | pass |
| publish, enrollment and readiness | 8.686 | pass |
| separate `oo check` | 0.401 | ready |
| KH / DD / SK queries | 0.342 / 0.264 / 0.275 | exactly 200 / 2000 / 1802 rows counted |
| wide-number query | 0.270 | byte-exact decimal result |

SCN-grant recovery to first answer is **235.011 s**, not an uninterrupted
install-to-answer measurement. `query-exact.json` compares counts with the
unchanged source CSVs. The answerability tool completes, but reports
**not_covered / no_visible_semantic_match** for the English customer question;
successful SQL and tool transport do not establish business vocabulary coverage.

Oracle actually rejects `NUMBER(40,12)` with **ORA-01727 (precision 1 to 38)**.
The separate `NUMBER(38,-2)` row returns the identical decimal string through
the source and installed Gateway:
`1234567890123456789012345678901234567800`.
This proves that valid wide-value case, not an impossible p>38 declaration.

## Cold bundle and emulated installation

`serial-accept.py` runs the final checks serially. Earlier overlapping
acceptance runs correctly failed global inventory equality and are retained;
their QS-05 results are not counted as passes.

Final offline run: **`v3-i09-offline-serial-qs-20261111`**.
The real `deploy/lite/bundle.py` create implementation exports the locally
built runtime/PostgreSQL bundle; its temporary export-container label alone
is adapted to `oo.lane=v03`. The load CLI verifies the manifest and archives.
Only task-owned runtime tags are removed, and absence of the runtime image
identity is checked before loading.

Test-only Docker/Compose wrappers reject pull/build and impose `--pull=never`
on container creation/Compose up. They do not alter command responses, SQL,
comparisons or cleanup assertions. The fresh package pins the verified loaded
platform image identity rather than the build's different attested OCI index.

- Offline load **2.059 s**; install to first answer, including load, private
  package staging and npm pack: **24.658 s**.
- Timing is the sum of measured phases, excluding bundle creation and small
  inter-command bookkeeping. Shared base layers/PostgreSQL remain warm.
- QS-01..05 pass; the audit DB independently confirms the denial; source 120
  versus analyst 60 orders verifies RLS; all three generated MCP configurations
  complete readiness/query/answerability; resource inventories match.
- This is an offline local-image test, not registry download latency or a
  three-platform clean-machine result.

Final amd64 run: **`v3-i09-amd64-accepted-qs-20261111`**.
The full official linux/amd64 image build takes **1285.562 s**. Initial runs
expose Docker's implicit-emulation warning ahead of candidate-refusal JSON.
The fix inspects the pinned image's OS/architecture and passes `--platform`;
it does not discard arbitrary stderr or loosen the refusal gate.

The corrected release CLI is rebuilt under emulation and replaces only `oo`
in the previously built image. `amd64-final-image.json` binds this composition
to its base, fix commit and CLI hash. The final image is
`open-ontology/lite:v03-i09-amd64-final`. The client uses the Docker VM host
network and the host UID plus socket group.

QS-01..05 pass on PostgreSQL **17.10**, including unchanged before/after
resource inventories. Install to first answer: **32.914 s**. Initial build,
failed probes, fix, CLI rebuild and final serial acceptance finish in
**2181.406 s**, within 60 minutes.

## Regression, retained failures and cleanup

Evidence: `v3-i09-20261111`.

- `cargo check --workspace --locked` and formatting checks pass.
- Builder: **203 passed**; Gateway: **323 passed, 50 ignored**;
  CLI: **189 passed, 3 ignored**. Ignores are not passes.
- `deploy/lite/test_demo*.py`: **18 passed**, including both architectures,
  invalid platform refusal and unchanged candidate-error rejection.
- A/B Python: **31 passed**. oo-test Python: **186 tests, 1 skipped**.
- Full lite Python: **116 passed, 1 failed**. Release-coordinator Rust:
  **104 passed, 1 failed, 12 ignored**. The replay fixture-control Python test
  also fails. None of these aggregate commands is described as green.
- Base `187f42e99` reproductions are recorded for
  `live::private_postgres_activates_only_a_test_scope_candidate`
  (absent `OO_A359_PG_PORT`),
  `test_retained_grant.Tests.test_up_provisions_configured_data_login_on_agent_db`
  (standalone Compose path versus asserted argv), and
  `test_retained_controls.RetainedControls.test_scalar_and_composite_controls_review_and_compile`
  (pre-existing structural seed versus retained fixture).
- Spec/plan/render checks pass. No Cargo.lock changes or shared-file patches
  are requested. The pre-existing untracked dispatch script is untouched.

Generated runtime credentials/profiles and temporary source snapshots are
removed after retaining non-secret build/usage evidence. Own-source `oo down`
retains data volumes; ten task-owned volumes are explicitly ownership-checked,
checked for container references, and removed. Final task containers, volumes
and networks are absent. Retained text-evidence scanning finds no supplied
model material or credential signatures; binary/image payloads are outside
that text scan. Final images and the offline bundle remain as artifacts.

Remaining work: establish source-grounded dictionary bindings and rerun the
accuracy gate; address the Oracle English vocabulary gap. `--rebuild`, real
desktop-agent sessions, production acceptance and broader platform percentiles
remain untested.
