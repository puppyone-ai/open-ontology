# V3-I-02 local end-to-end sign-off

Status: **partial**. Date: 2026-11-11. The installed no-model demo is accepted
on this macOS arm64 host; MySQL model-backed publication and live A/B are not.
Raw evidence is ignored under `evidence/v3-i02-resume/`,
`evidence/v3-i02-qs-final3/`, and `evidence/v3-i02-init-final/`.

## Installed package acceptance

The actual package staging tool included the current native CLI, MCP bridge,
source notices, and runtime image companion. `npm pack` and an offline global
install used a new private prefix and initially empty HOME for each run.
Only PATH, HOME, LANG, TMPDIR and npm cache configuration were supplied.
There were no model variables, credentials or model-egress service.

This is a private, partial darwin-arm64 package, not a registry release or a
four-platform release artifact. It does not include the optional host MySQL
helper; the lite runtime does include the current helper.

```sh
python3 tools/oo-test/test_quickstart_live.py \
  --package "$PWD/evidence/v3-i02-resume/package3" \
  --run-id v3-i02-qs-final3
python3 tools/oo-test/test_quickstart_live.py \
  --package "$PWD/evidence/v3-i02-resume/package3" \
  --run-id v3-i02-init-final --entry init
```

| Entry | Install | Deploy/readiness | First query | Install to first answer |
| --- | ---: | ---: | ---: | ---: |
| `oo quickstart --yes --json` | 0.896 s | 15.259 s | 0.235 s | **16.391 s** |
| `oo init --demo --yes --json` | 0.933 s | 14.262 s | 0.231 s | **15.426 s** |

Both runs pass all five QS checks:

- **QS-01:** real publication, enrolled `demo` profile, ready Gateway, and
  first answer. Timings start before installation, after packing. Images are
  warm; these are individual observations, not p50/p95 or cold-download claims.
- **QS-02:** no model environment or key is needed. The shipped confirmation
  binds the curated graph hash; the real compiler and publisher replay it.
- **QS-03:** analyst queries return exactly `60`, `3517.5000`, and `31`.
  The source administrator sees 120 orders, independently confirming that the
  analyst result is filtered. The private-column query returns
  `COLUMN_NOT_ALLOWED`. The runner independently reads its exact audit ID from
  the control DB and verifies the stored denial code (`deny-audit.stdout`).
- **QS-04:** each generated Claude, Cursor and Codex config launches its actual
  installed command and completes `check_readiness`, the order-count query,
  and `check_answerable`. This tests the real bridge/Gateway, not the desktop
  agent applications or an HTTPS fixture.
- **QS-05:** down takes 0.877 s / 0.869 s. Before/after container, volume and
  network inventories are identical. No owned resources remain, and unrelated
  containers are untouched.

The observed database version is **PostgreSQL 17.10**
(`17.10 (Debian 17.10-1.pgdg12+1)`). The synthetic source is a separate database
on the control server, keeping the deployment within two database containers.
Product capture uses SELECT-only credentials; fixture administration is separate.
RLS is installed on the serving replica before publication, without replacing
sealed view definitions or changing gold results.

The demo-only collector copies real Gateway WAL records to
`oo_demo.audit_event` and reads them back. Audit IDs identify persisted records,
not invented query receipts. Generation is matched to the authenticated
freshness publication receipt. `source_time: unmeasured`, grade B and
non-atomic cross-table consistency remain explicit. This is not a general
production audit-collector or source-real-time acceptance.

## Runtime provenance and preserved failures

Installed image companion:
`sha256:0b614287ad4969c95ed2e5362b206c48398b794e6fb672cf0f49e8808293b992`.
The image is arm64 and labelled `oo.lane=v03`.

The requested normal bake first rebuilt the rebased tree successfully,
including current MySQL discovery. Rebuilding after the refusal-audit fix
hit crates.io timeouts; a subsequent Rust 1.98.1 toolchain download stalled.
The successful local runtime rebuild used the same locked sources, an offline
`cargo vendor` tree, and the pinned base image's supported Rust **1.94.0**.
Host Cargo check/test/clippy used **1.98.1**. No dependency versions or lockfiles
changed. The exact alternate build recipe is
`evidence/v3-i02-resume/Dockerfile.msrv`; outputs are in
`image-build.log`, `image-build2.log`, `image-build-offline.log`, and
`image-build-msrv.log`. This compiler deviation must not be represented as a
standard release build.

Earlier failures remain intact. They exposed and fixed exact source-type
binding, clean-HOME Compose discovery, fixture source preconditions, replica
RLS lock privileges, the reviewed-document filename, missing guard-refusal
WAL, WAL/credential permission-contract differences, and the MCP query's
missing required `format`. In particular, `v3-i02-qs-final` failed audit
collection and `v3-i02-qs-final2` failed MCP; neither is relabelled a pass.
The stopped predecessor's two owned database containers were identified by
their worktree mounts and owner record, then cleaned before new runs.

## Remaining model-dependent exits

- **MySQL 8.4 own-database publication: not run.** No authorized in-worktree
  endpoint/key `secret_ref` pair was supplied. External token stores and
  inherited agent-transport credentials were not repurposed. Model reachability
  is **untested**, not "unreachable." Prior catalog-discovery passes were not
  repeated, as requested; they do not establish own-database publication.
- **Live C0/C3 A/B: not done.** The current runner only supports recorded
  responses/transcripts, and no model request was made. Live relative change,
  fixed/broken lists and health counters are unmeasured.
- The replay regression was run with
  `oo-eval ab --arms c0,c3 --dataset demo --output evidence/v3-i02-resume-ab-replay`.
  It reports `model_measured=false`, null headlines, fixture-only fixed `q02`
  and broken `q16`, and zero no-output among three planned accuracy questions
  per arm. These are **not live model numbers**. Its original Pack was frozen
  beside the recordings rather than rebinding old responses to new prompts.

## Validation

All Cargo commands used `CARGO_BUILD_JOBS=4` and `nice -n 19`.

| Command / scope | Result |
| --- | --- |
| `cargo check --workspace --locked` | Pass |
| `cargo test --locked -p oo-cli` | 182 passed, 3 ignored |
| `cargo test --locked -p oo-gateway` | 312 passed, 50 ignored |
| `cargo test --locked -p oo-eval` | 2 passed |
| `cargo test --locked --manifest-path tools/mcp-gateway/Cargo.toml --target-dir target/mcp` | 9 passed |
| All-target clippy for those crates, `-- -D warnings` | Pass |
| Rustfmt on touched Rust files | Pass |
| `packages/cli`: `npm test` | 48 passed, 1 skipped |
| Python demo/lifecycle/provision/status suites | 46 passed |
| `benchmarks/ab/test_ab.py` | 17 passed |
| Shared hash vector, both Pack recompilations, tampered confirmation | Pass within demo suite |
| `check_spec.py`, `check_plan.py`, `check_v02_plan.py` | Pass; structural checks only |
| Staged secrets/private paths and 11 configured private terms | Zero hits |
| Generated credentials compared against raw outputs | Zero hits |

Ignored or unrun tests are not passes. No connector, builder, TUI, migration,
coordinator-owned document, root dependency or lockfile was changed.
No shared-file patch is requested. Public distribution, other platforms,
cold images, representative timings, ops enrollment and `--rebuild` remain
separate follow-ups. The host adapter still requires Python 3.

No task containers, volumes or networks remain. Local images and private
ignored evidence are retained.
