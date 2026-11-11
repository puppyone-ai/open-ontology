# V3-I-04 implementation and acceptance

STATUS: **partial**. Date: 2026-11-11. Base: `d87835ac5`.
Raw evidence: `evidence/v3-i04-20261111/` and the run IDs below.
V3-I-04 is **not done overall**: own-database onboarding and the published
full OO-Logistics A/B exit remain unmet.

## Product changes

- `2ef0210cb`: require and verify the packaged MySQL discovery helper; wire
  non-TTY selection to discovery. Four release targets include build/staging
  wiring, and postinstall verifies the helper digest.
- `be905509c`: close helper license-text gaps with exact upstream provenance.
  Four-target notice collection covers 403 dependencies without component
  notice gaps. This is not legal clearance or a verified linked-artifact SBOM.
- `510f4c7df`: accept model file references, copy input credentials only to
  private per-deployment state outside Git, and mount them read-only in both
  Compose and one-shot build phases. Empty mount targets contain no secrets.
  Stack hashes detect changed inputs. Status exposes a configured boolean.
- `455e73320`: add bounded live model capture, publication-pinned Gateway
  execution and byte-identical recorded replay. Files are read in place;
  no credentials enter argv, environment variables or reports.

Private-source coverage includes permission/symlink/Git-boundary tests and a
real synthetic read-only container mount probe. Full model-backed lite
deployment is still not an accepted result.

## Installed-package regression

Freshly staged current darwin-arm64 binaries, `npm pack`, offline installation
with postinstall enabled, and an empty HOME:

```sh
python3 tools/oo-test/test_quickstart_live.py \
  --package evidence/v3-i04-20261111/package-current \
  --run-id v3-i04-qs-final-20261111 --entry init
```

**QS-01..05 pass**, PostgreSQL **17.10**. Three golden queries, the audited
private-column denial, and readiness/query/answerability through each generated
MCP configuration pass. Source orders: 120; analyst-visible orders: 60.
This is `oo init --demo`, not own-database Self-build.

| Warm-image demo stage | Seconds |
| --- | ---: |
| Pack, outside first-answer timing | 5.647 |
| Install, including postinstall | 1.242 |
| Deployment/readiness | 14.453 |
| First query | 0.233 |
| Install to first answer | **15.927** |
| Down | 0.878 |

These are one-run local timings, not cold-download or cross-platform
percentiles. The runtime pin is the existing V3-I-03 image
`sha256:de6131a20e0fe91c16bf2a414da4b942330551fad17ef441d6d4174ec644517f`;
its disclosed Rust 1.94.0 build deviation remains. Native binaries here use
Rust 1.98.1. Other native platforms were not built or run; their staging
contracts were exercised with architecture fixtures.

## Own-database acceptance: blocked

Run: `v3-i04-mysql-20261111`. An owned, labelled MySQL **8.4.7** fixture was
seeded from `bench/datagen` OO-Logistics xs, seed 42. The installed package
connected through TLS using a separate SELECT/SHOW VIEW account.
`--tables-recommended` reached selection but returned **TABLES_EMPTY**:
the short legacy table names have no declared foreign keys or recommendation
signal. No rule, threshold or fixture was changed to manufacture a selection.

| Stage | Seconds |
| --- | ---: |
| Fixture startup | 8.004 |
| Datagen MySQL seed | 2.134 |
| TLS setup | 2.188 |
| Discovery/recommendation rejection | 1.126 |
| Same invocation in zh 120x40 PTY | 0.155 |

The saved PTY transcript proves the rejection only, not five-screen wizard
completion. Self-build, adjudication, graph, hash-bound confirmation,
publication, profile, readiness, three queries and answerability are **not
run**; their durations and time to first answer are null.
There is also a resource conflict: the normal own-database flow needs the
MySQL fixture plus lite's Agent DB and Control DB, exceeding the card's
two-database-container limit. That limit was not exceeded.

## Live A/B and replay

Final run: `v3-i04-demo-ab3-20261111`; replay:
`v3-i04-demo-replay3-20261111`. The Pack input was copied from the actual
deployed demo role Pack, and query receipts pinned publication, data vector,
knowledge version and policy version. Requested model: `gpt-6.1-sol`, seed 42.
Live capture/scoring and replay both exit 0; reports are byte-identical.
Capture/scoring took 21.651 seconds. Exit 0 is not a health or quality pass.

| Dataset / track | Relative change | Fixed | Broken | No-output C0 / C3 |
| --- | --- | --- | --- | --- |
| Published demo / 3 accuracy questions | Suppressed: zero baseline | None | None | 0/3, 0/3 |
| Published demo / model-refusal permission | 0% | None | None | 0/1, 0/1 |
| Published demo / injection | Suppressed: health gate | None | inject01 | 0/1, 1/1 |
| Full published OO-Logistics | Not run; no Pack produced | Unmeasured | Unmeasured | Unmeasured |

Accuracy failures retain exact warning mismatches, refusals and query failures.
Gold and warning semantics were not relaxed. Model refusals are not proof of
Gateway enforcement. The health gate rejects the final C3 injection track.
Earlier failed adapter attempts and the missing nested-mount-target probe
remain in the raw evidence; they are not relabelled as passes.

## Tests and residuals

- `cargo check --workspace --locked`: pass.
- `cargo test --locked -p oo-cli -p oo-eval`: 184 CLI tests and 2 evaluator
  tests pass; 3 CLI tests ignored. Evaluator invokes 23 passing Python tests.
- All-target clippy for both touched crates with `-D warnings`: pass.
- Node package/staging: 52 pass, 1 skip. License tests: 7 pass.
- `tools/oo-test` Python suite: 185 pass, 1 skip. Model egress: 18 pass.
- Selected lite suites: 88 pass, 1 skip, 1 pre-existing failure:
  `test_retained_grant.Tests.test_up_provisions_configured_data_login_on_agent_db`.
  The identical standalone-Compose versus `docker compose` expectation failure
  was reproduced using the base commit's source, in `base-grant.log`.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.

No shared-file patch is requested; no Cargo.lock changed. Full MySQL
onboarding, full OO-Logistics A/B and native non-arm64 acceptance remain open.
Task-owned resources were verified absent by scoped labels. No unrelated
Docker resource names are retained in inventories. All retained task evidence
directories were scanned for exact model material, credential signatures and
11 configured private terms: zero hits. Generated package/install artifacts
were removed after retaining hashes/manifests; original model files were
neither copied into the repository nor modified.
