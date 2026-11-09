# V3-M-01 data/runtime sign-off (partial)

Base: `d941238aa`. Host: macOS arm64. Runtime: locally built lite
`sha256:1d2d3dabbee677e28f88c0f8d34973fab4d179be37619705905f49254a31303a`.
No image push, public pull, model call or customer-system access.

## Executed checks

Raw command outputs: `evidence/v3-m-20261109/`.
Final database run: `evidence/v3-m-20261109-live-6/`.

| Command / check | Result |
| --- | --- |
| `cargo check --workspace --locked` | pass on base and final tree |
| `cargo fmt -p oo-cli` | completed; only touched crate |
| `cargo test -p oo-cli --locked` | 96 passed, 0 failed, 3 existing opt-in tests ignored |
| `npm test --prefix packages/cli` | 44 passed, 0 failed, 0 skipped |
| `python3 -m unittest discover -s deploy/lite -p test_demo.py -v` | 8 passed; real compiler recomputation included |
| `python3 specs/scripts/check_spec.py` | pass before and after docs changes |
| `python3 specs/scripts/check_plan.py` | pass before and after docs changes |
| `python3 specs/scripts/check_v02_plan.py` | pass before and after docs changes |
| `python3 deploy/lite/test_demo_live.py --run-id v3-m-20261109-live-6 --oo target/debug/oo --image <runtime digest above>` | local fixture and native teardown pass |

Ignored CLI tests are `live_declared_catalog_export`, `live_child` and
`owned_gateway_first_token`. They were not run or counted as passes.
No pre-existing failing test exemption was needed. Cargo.lock is unchanged.

The native demo integration test performs four real CLI/SDK calls against a
scripted TLS oracle. It checks gold SQL order, exact results, expected refusal
and null audit/generation fields. This is **transport contract testing, not
a real Gateway, publication or audit-collector acceptance run**.

## Engine evidence and requested QS exits

PostgreSQL 17.10, Linux aarch64 image, on the macOS arm64 Docker host:
10 tables / 20,522 rows; all ten keys and ten declared relations checked.
The source reader connects with TLS verify-full, reads all 120 orders and
cannot INSERT, including after disabling its read-only transaction default.
Both demo roles return their three exact gold values, see only their assigned
order region and cannot read the denied contact column or write.

- QS-01 local: **not passed**. `target/debug/oo quickstart --yes --json`
  exits 2; its delegated `init --demo` is not implemented on this lane.
- QS-02 no model: **not passed end to end**. With model environment variables
  unset, `oo init --demo --yes --json` exits 2 for the same missing entry.
  Fixture startup and both curated Pack compilations make zero model calls,
  but this does not establish a published demo.
- QS-03: **not passed**. `oo try` with no provisioned demo token exits 1
  (`passed:false`, authentication failures). The TLS oracle and PostgreSQL
  denials are separately passing checks, not a real Gateway deny/audit pair.
- QS-05 local: **passed** through `oo quickstart --down --state ... --json`.
  Repeated teardown is safe. Own containers/volumes/network are gone.
  All 181 pre-existing containers (including 8 running), 62 networks and
  102 volumes remain. Images were not removed.

Final fixture startup took 5.821 seconds with warm local images. This is
neither install-to-first-answer timing nor a p50/p95 result. Other engines,
platforms, public distribution and QS-04 were not tested.

Runs `live-1` through `live-3` failed startup and remain on disk. `live-3`
captured PostgreSQL refusing the root-owned mode-0600 HBA file. The new demo
script now gives that file to postgres; `live-4`, `live-5` and `live-6` pass.
Every failed run also cleaned up its own Docker resources.

## Evidence hashes

| Artifact | SHA-256 |
| --- | --- |
| `v3-m-20261109-live-6/result.json` | `67de146bf736e256e05e060adb293f9df75bad089ed549482864a8c097bc59f6` |
| `v3-m-20261109/cargo-check-final.log` | `f724a3f2283e354d90cb8102d5313be016b02eb5d8d753fd5951d9dd87ae1c35` |
| `v3-m-20261109/cli-tests-final.log` | `5aee3cc17c48b4409b12c117a621238497e0d595a46ddc026636a0dc2f50d560` |
| `v3-m-20261109/npm-tests.log` | `c57a9b421661251b04340b974be99b4b48a4a3a55bc10c5014c8cc0d97afdcd5` |
| `demo/manifest.json` | `2c5193ed76c88bafa597c1c2a6de2858dc2798ca7b94a91dadcbcc055007547f` |

## Integration work still required

Connect the isolated fixture to the real Agent DB load/publish path, install
both Gateway roles, enroll CLI profiles, then let the TUI confirm through
`oo init --demo`. Implement the model-backed `--rebuild` path there. Do not
replace publication or confirmation with `fixture_ready`.

`oo_core::api::QueryResponse` currently has query_id/freshness but no audit_id
or published_generation. The demo does not invent either field or alter that
shared API. Real Gateway/audit acceptance needs the coordinator's contract and
integration work.

The graph hash serialization is specified in this directory's README. Before
integration, the coordinator should align the adjudication producer, TUI and
confirmation verifier on that exact byte recipe. No shared contract, root
manifest, lockfile, migration, connector engine or support matrix was changed.

Requested coordinator clarification (not applied here):

```diff
--- a/execution/v0.3/contracts/adjudication.md
+++ b/execution/v0.3/contracts/adjudication.md
@@ -3 +3,5 @@
 The Builder writes `adjudication.json` beside the reviewed-community input. The TUI reads it, and so does the non-TTY `oo ontology graph`.
+For the shipped demo, input_sha256 hashes exact input file bytes.
+summary_sha256 hashes UTF-8 JSON {"graph": graph, "items": items} with
+recursively sorted object keys, preserved array order, compact separators,
+unescaped Unicode and no final newline.
```
