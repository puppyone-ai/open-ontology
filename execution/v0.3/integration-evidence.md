# V3-I-01 integration checkpoint

Status: partial. Run: `evidence/v3-i-20261109/`. No customer systems,
existing profiles, services or containers were changed. No database
containers were created. This is not quickstart acceptance.

## Delivered

- Curator, asset verifier, builder and CLI agree on the contract preimage.
  The Python, builder and CLI tests consume the same checked-in hash vector.
  CLI graph serialization preserves null fields. The bundled demo and its
  manifest were rebound; no business gold values changed.
- The delivery adapter calls the real profile, prompt, configuration and
  readiness modules. Credential references must be supplied after issuance;
  an unprovisioned preview cannot read an unrelated existing profile.
  Readiness cannot turn green after a profile save failure.
- MCP snippets use the actual `oo mcp --stdio --profile NAME` entry.
  The installed npm CLI successfully launched all three generated configs
  and performed authenticated readiness and query calls against a local
  HTTPS fixture. This is transport integration, not a real Gateway proof.
- The demo graph now comes from the curated demo, not a CLI test fixture.
  Preview stages no longer claim successful execution. The build renderer
  recognizes the actual `complete` status and does not overlook a pending
  substage based on iteration order.

## Acceptance probes

Local arm64 debug binaries were staged with the existing image pin, packed
using `npm pack`, and installed into a private temporary prefix. The staged
package is marked private/partial, not a release artifact. The image pin
was not pulled or exercised.

| Exit | Result |
|---|---|
| QS-01 | Not passed. Installed `oo quickstart --yes --json` exits 0 in 3.916 s but only returns `oo.init-tables/v1`; no deployment, publication or answer. |
| QS-02 | Not passed. Same probe without model/API-key variables reaches selection only. |
| QS-03 | Not passed. Installed `oo try --json` exits 2 in 0.114 s without an enrolled profile. No deny audit row exists from this run. |
| QS-04 | Not passed against Gateway. All three generated configs pass readiness/query against an authenticated HTTPS fixture (`mcp-installed-verified.log`). |
| QS-05 | Not passed as teardown acceptance. Down exits 0 in 0.171 s with `status: not_started`; no demo resources existed. Final lane resource scan is empty. |

Time to first answer: unmeasured; no first answer was produced. No p50/p95
or clean-machine claim. Both real PTY runs traverse all five preview screens
and exit 0: `pty-en-verified.transcript`, `pty-zh-verified.transcript`.
They are previews, not deployment or publication evidence.

## Validation

All Cargo build/check/test commands used `CARGO_BUILD_JOBS=3` and `nice -n 10`.

- `cargo check --workspace --locked`: pass (`acceptance-check.log`).
- `cargo test --locked -p oo-cli -p oo-builder -p oo-gateway -p oo-stack`:
  680 passed, 53 ignored, zero failed (`acceptance-suites.log`). Ignored
  database/runtime tests are not passes.
- `cargo test --locked --manifest-path tools/mcp-gateway/Cargo.toml`:
  8 passed (`verified-mcp-suites.log`).
- Clippy, all targets, `-D warnings`: pass for those four workspace packages
  and separately for mcp-gateway (`acceptance-clippy.log`,
  `verified-mcp-clippy.log`).
- `cargo fmt` on touched crates and mcp-gateway: run.
- `packages/cli` npm tests: 48 passed, 1 skipped (`npm.log`).
- `python3 -m unittest discover -s deploy/lite -p test_demo.py`:
  9 passed (`python-demo-final.log`).
- `tools/oo-test/test_mcp_config.py`, with `OO_TEST_BIN_DIR` pointing at
  the installed package: 1 test, three agent subtests, pass.
- `check_spec.py`, `check_plan.py`: pass.
- Private-term scan of both new files: zero hits across 11 configured private
  terms. Generic IP scan flags only the deliberate loopback fixture.
- Earlier failed vector/snapshot/fixture/lint attempts remain in the run
  directory. No tests, gold values or lint rules were disabled.

## Remaining card exits

V3-I-01 remains not done. Item 2 is done. Items 1, 3 and 4 are partial:
production enrollment/publication is not wired into the five-screen flow,
remaining wizard/decision copy needs localization, and the real build chain
does not stream all five product stages. Item 5 (PG/MySQL/Oracle catalog
discovery and PG17 integration) is not implemented or tested. Item 6 is
not accepted for the reasons above. Item 7's listed local suites were run,
but ignored live tests and absent engine-version runs remain gaps.

No shared-file patch requests. No Cargo.lock changes. No containers left
running by this task.
