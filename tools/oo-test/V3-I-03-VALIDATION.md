# V3-I-03 model acceptance and merged-tree regression

Status: **partial**. Date: 2026-11-11.
Base: `e259053b4`. Raw evidence: `evidence/v3-i03-20261111/`.
No model credentials were opened, copied, or passed as values. No product
source, dependency, lockfile, shared document, or acceptance threshold changed.

## Rebuilt runtime and fresh-package QS acceptance

Rebuilt arm64 runtime:
`sha256:de6131a20e0fe91c16bf2a414da4b942330551fad17ef441d6d4174ec644517f`,
labelled `oo.lane=v03`.

The standard bake stalled downloading Rust 1.98.1 components and was stopped
after more than ten minutes. The successful fallback used the pinned base's
Rust **1.94.0**, offline `cargo vendor --locked`, and the exact tracked product
sources at the base commit. `build-source-provenance.json` records the
comparison: only the isolated context's toolchain setting, Cargo source
transport and vendor inclusion differed. No lockfile changed. This is the
same class of compiler deviation disclosed by V3-I-02, not a normal release
build. The first fallback's missing vendored `.envrc` checksum failure is
preserved in `image-build-msrv.log`; it was fixed only in the temporary build
context's ignore rules. The successful log is `image-build-msrv2.log`.

The current native CLI and MCP bridge were staged with source notices and the
new image pin, then freshly packed and installed into a private clean HOME:

```sh
python3 tools/oo-test/test_quickstart_live.py \
  --package evidence/v3-i03-20261111/package \
  --run-id v3-i03-qs-20261111
```

**QS-01 through QS-05 pass.** PostgreSQL **17.10** reports 120 source orders
versus 60 for the analyst. All three golden queries, the private-column
refusal and independent audit readback pass. Generated Claude, Cursor and
Codex configs each complete real MCP readiness, query and answerability.
No model environment variables are supplied.

| Stage | Seconds |
| --- | ---: |
| Fresh npm pack (outside the first-answer timing) | 4.444 |
| Offline installation | 0.931 |
| Deployment/readiness | 17.079 |
| First answer | 0.235 |
| Installation to first answer | **18.245** |
| Down | 0.846 |

These are single-run warm-image demo measurements, not MySQL/model-backed
timings, cold-download timings or percentiles. Raw outputs are in
`evidence/v3-i03-qs-20261111/`. The package is private and darwin-arm64-only.
Before/after Docker inventories are identical. Temporary build/package/cache
artifacts and cleaned deployment credentials were removed after verification;
their package hashes and native manifest remain alongside the raw outputs.

## Own-database acceptance: blocked before model execution

The authorized file references exist as an owner-provided mechanism; this is
**not** a claim that credentials were missing or that the model was unreachable.
The complete MySQL 8.4 flow could not be safely driven through this tree:

- `SecretStore` supports in-place `file:` references, but the lite deployment
  consumer does not. `deploy/lite/lifecycle.py` accepts only bare secret names
  and copies their bytes into the deployment directory. Its `file:` rejection
  was reproduced with synthetic references, mocked Docker inventory, and an
  assertion that no secret read occurred: `SECRET_NAME`, zero secret reads.
  Using bare names would copy the authorized model files into this worktree,
  contrary to the task card, so that alternative was not used.
- The TTY model prompts in `admin/wizard.rs` still offer import, not an
  in-place reference: `@PATH` calls `import_file`, which calls `write_new`.
  Real credentials were not supplied to those prompts.
- Direct non-TTY `oo init --engine mysql --host 127.0.0.1 --database logistics
  --tables-recommended --json` fails with `TABLE_CATALOG_REQUIRED` before a
  connection. It requires `--catalog`; real discovery is wired to the
  interactive path. An exported real catalog could address this selection
  gap, but cannot address the deployment credential-copy boundary.

`blockers.py` in the evidence directory reproduces these observations. The
freshly rebuilt CLI also returned `TABLE_CATALOG_REQUIRED` in a real
`zh_CN.UTF-8` PTY sized 120 columns by 40 rows; the transcript is preserved.
This is a rejection-path transcript, **not** a completed five-screen TUI run.

No MySQL fixture was created after the credential boundary was demonstrated.
Discovery, load, model-backed Self-build, adjudication, graph confirmation,
publication, profile enrollment, readiness, three queries and answerability
therefore remain unmeasured. Their stage durations and time to first answer
are **null**, not zero. `blockers.json` records only the short rejection-probe
durations; they are not onboarding performance measurements.

## Live A/B: unavailable in this runner

`oo-eval` delegates to `benchmarks/ab/run.py`. Its actual CLI offers frozen
response replay only: no live adapter, endpoint/key file inputs, authenticated
publication binding, or live SQL execution. This was checked in source and
through `ab --help`; no replay output was relabelled as a model measurement.

| Dataset | C0/C3 live calls | Relative change | Fixed / broken | Health gate |
| --- | ---: | --- | --- | --- |
| Own OO-Logistics published Pack | 0; Pack not produced | Unmeasured | Unmeasured | Not run |
| Published demo Pack | 0; live adapter absent | Unmeasured | Unmeasured | Not run |

The prescribed model remains `gpt-6.1-sol` and the replay harness pins seed 42.
Neither constitutes evidence of a live call. The passing 17-test A/B scorer
suite checks fixture/replay behavior only.

## Regression

All Rust commands used `CARGO_BUILD_JOBS=4` and `nice -n 19`.
Host toolchain: Rust 1.98.1. Commands, exit codes and durations are recorded
in `regression.json`; individual raw logs retain failures and ignored tests.

| Scope | Result |
| --- | --- |
| `cargo check --workspace --locked` | Pass |
| `cargo test --locked -p oo-cli` | 183 passed, 3 ignored |
| `cargo test --locked -p oo-builder` | 201 passed |
| `cargo test --locked -p oo-gateway` | 312 passed, 50 ignored |
| `cargo test --locked -p oo-stack` | 19 passed |
| `cargo test --locked -p oo-connectors` | 238 passed, 24 ignored |
| `cargo test --locked -p oo-eval` | 2 passed, including 17 Python scorer tests |
| `cargo test --locked --manifest-path tools/mcp-gateway/Cargo.toml --target-dir target/acceptance-tools` | 9 passed |
| Same command for `tools/release-coordinator/Cargo.toml` | 104 passed, 1 failed, 12 ignored |
| Additional Rust pg-builder suite | 45 passed, 3 failed |
| All-target clippy, six workspace crates, `-- -D warnings` | Pass |
| MCP all-target clippy, `-- -D warnings` | Pass |
| Additional release-coordinator all-target clippy | Fails existing warnings; no formatting or suppressions applied |
| `npm test --prefix packages/cli` | 48 passed, 1 skipped |
| `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'` | 185 passed, 1 skipped |
| v0.3 pg-builder Python selection | 103 passed, 2 opt-in live tests skipped |
| v0.3 lite bundle/demo/deploy/lifecycle/provision/scheduler/status Python suites | 85 passed |
| `check_spec.py`, `check_plan.py`, `check_v02_plan.py` | Pass; structural checks only |

The v0.3 pg-builder selection is `test_adjudication`, `test_library_control`,
`test_library_declared_fks`, `test_oos_tree`, `test_relationship_live`,
`test_retained_build`, `test_retained_catalog`, and `test_table_review`.
It ran with an evidence-local Python environment supplying `jsonschema` and
the existing Builder binary. No dependency was added to the product.

### Base failures and preliminary setup failures

All failures below were observed before any tracked edits, on the card's base
commit. `base-commit.txt`, `base-status.txt`, `base-diff.txt` and
`base-recheck.diff` record that boundary. The following Rust failures were
also rerun explicitly, with identical outcomes:

- `live::private_postgres_activates_only_a_test_scope_candidate`:
  `OO_A359_PG_PORT` is required; no private PostgreSQL fixture was supplied.
- `document_profile::tests::shipment_catalog_keeps_weight_and_the_nineteen_approved_columns`:
  absent historical evidence.
- `source_contract::tests::derived_source_bound_projects_native_notes_without_a_forged_grade`:
  absent historical evidence.
- `source_contract::tests::oracle_batch_and_ts_keep_real_caveats_and_omit_grade`:
  absent historical evidence.

The extra lite test
`test_retained_grant.Tests.test_up_provisions_configured_data_login_on_agent_db`
fails identically on a second base run because it expects `docker compose`,
while this host resolves the supported standalone Compose executable.
The other two tests in that suite pass.

An initial broader, non-card-specific pg-builder Python discovery ran 378
tests and reported 1 failure, 18 errors and 20 skips. Its log is preserved:
two tests needed `jsonschema`; the full-fixture test needed
`OO_LIBRARY_BUILDER_BIN`; the access-profile tests deliberately require
output outside Git, incompatible with the task's in-worktree-only boundary.
The narrower v0.3 selection subsequently passed with its prerequisites.
The broader suite is not reported as passing.

## Follow-ups

Both retained evidence directories were scanned against 11 configured
private terms and API-key, Gateway-token and private-key signatures. No
secret-signature leak was found. The two raw Docker inventory files each
contain two private-term matches from pre-existing, unrelated resource names;
they remain under the 0700 evidence directory, are not reproduced here and
are not committed. The scan therefore is **not** reported as a blanket
zero-hit private-term pass. Generated fixture credentials were separately
compared against captured command outputs with zero leaks. No authorized
model file was read for these scans.

The coordinator must integrate end-to-end in-place model file transport
through TUI, lite deployment, runtime stack and model egress without copying
the authorized files, then rerun the MySQL and full Chinese PTY flows.
A live evaluator must bind both arms to the actual published Pack, execute
model-produced SQL read-only, retain deterministic scoring and apply the
existing no-output health gate. Neither gap is a model-quality result.

No shared-file patch or Cargo.lock change is proposed. No Rust files were
edited or formatted, including release-coordinator. No task containers,
volumes or networks remain; unrelated resources were not changed.
