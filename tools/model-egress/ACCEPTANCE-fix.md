# A-S0S1-fix acceptance summary

Run: `a-s0s1-fix-20261104`. Card exits complete with the explicitly authorized
pg-builder baseline-test exception. This is not full product acceptance.
Prior `ACCEPTANCE.md` and its failed evidence remain unchanged.

- S0-M-04: bounded YAML/JSON share strict types and semantic checks. YAML
  dependency admission: `supply-chain/stack-yaml-audit.json` (31 checksum-verified
  package archives and license sets, including inherited notices). Existing
  `ci/deny.toml` used unchanged. Registry API returned HTTP 403; publication
  timestamp comes from the official sparse-index cache, read after index refresh.
- S0-A-01: coordinator dependency patch is present. Admin contract tests pass;
  init normalizes YAML to owner-private JSON for downstream egress. Other admin
  commands remain explicit skeleton refusals; no deployment claim.
- S1-A-01: finish frames plus usage complete without upstream DONE. Downstream
  receives normalized DONE. Truly truncated streams still fail; missing usage
  has its own refusal. Exact model ID wins; otherwise every matching channel is
  recorded and each budget takes the minimum. Unknown limits use recorded stack
  or 32k/4k fallbacks. pg-builder's explicit credential-free mode is committed.
- Secret stores canonicalize ancestors, including macOS temp/config aliases,
  while refusing private directory/file symlinks and wrong modes/owners.

## Validation

- `cargo test -p oo-stack -p oo-cli --locked`: 48 passed (7 stack, 41 CLI).
- `cargo check --workspace --locked`: passed before each commit.
- `cargo fmt -p oo-stack -p oo-cli --check`: passed. Both touched pg-builder
  files formatted with rustfmt, edition 2024.
- `python3 -m unittest discover -s tools/model-egress/tests -v`: 15 passed on
  Python 3.14.7, OpenAI-compatible and Anthropic synthetic wire replays.
- `cargo audit`: passed, cargo-audit 0.22.2, 445 locked packages.
- `cargo deny --config ci/deny.toml check advisories licenses`: passed,
  cargo-deny 0.20.2. Admission shape and all recorded license hashes verified.
- Rust toolchain: 1.98.1, macOS arm64. Minimum-toolchain rebuild not repeated.
- `cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --lib
  egress_tests`: 2 passed. Full `--lib`: 43 passed, 3 failed. Retained baseline
  `evidence/a-s0s1-20261104/builder-baseline-tests.txt`: 41 passed, same 3 failed:
  `document_profile::tests::shipment_catalog_keeps_weight_and_the_nineteen_approved_columns`,
  `source_contract::tests::derived_source_bound_projects_native_notes_without_a_forged_grade`,
  `source_contract::tests::oracle_batch_and_ts_keep_real_caveats_and_omit_grade`.
  Missing historical evidence is not fabricated; those tests are unchanged.

## Live check

Authorized `live_check.py`: passed, one models request and four completion
calls, no retries. Real short completion and streaming through a newly created
loopback listener both pass. Matched `cb/gpt-6.1-sol` (1,050,000 context) and
`cx/gpt-6.1-sol` (272,000 context), both 128,000 max output. Selected budgets:
272,000 context and 128,000 output. Minimal-prompt overhead: 2,490 tokens.
Metadata is not a boundary test; overhead is total minimal-prompt usage, not
isolated injection. One-token JSON probe establishes parameter acceptance,
not valid JSON output or enforcement of requested token limits. Anthropic
has replay coverage only, not a live provider check.

## Integration and boundaries

No shared-file patch requested. Cargo.lock adds only serde-saphyr to the local
oo-stack dependency list; no new registry version/checksum. Include/interpolation
features remain off. Raw YAML must pass `oo init` normalization before the Python
egress, which deliberately remains a JSON-only, standard-library consumer.
Supply-chain executor self-review is recorded; coordinator integration review
is still required. Database engines were not exercised. No containers created
or left running; all created HTTP listeners closed. No existing services,
customer systems or other worktrees changed. Initial failed compiler attempts
are retained alongside successful reruns in the run directory.

## Raw evidence SHA-256

| File relative to run directory | SHA-256 |
| --- | --- |
| `final-rust.txt` | `e18b1334574f8281a4b37470b4e55d0453da4df702c3e0f3fc43f452ccb40c4a` |
| `final-check.txt` | `c982bdeb61196f9ea79aa28075cbdee613064263dd175cf4bc707df0ec435476` |
| `final-python.txt` | `222cca8826fd97dbbc307df1f02a804b08f7116fc08193aa2eeeadab40867256` |
| `final-fmt.txt` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `live-check.txt` | `31c5b297d80fa6db26bc8d550cec6eb5e1d9ea21845bdbdc62352f0a0ec392c2` |
| `live/probe.json` | `98140f7a9f5d2dcfaee645e48481ff812a12b372fa0aaf0a8a67d619ce7fe325` |
| `live/usage.jsonl` | `ac543583e989a3928a0f8371c17ea3f2e4d4665cae035b915b64739751d3fea3` |
| `live/result.json` | `63813fcca407c0b56744564c690ec6175f3c5d2ba52a998a46bf8c96f9dbee9c` |
| `builder-tests.txt` | `740ff4593c9afaaf858295eb00498ab05f448e36e2f3ad8fa10e8353a3f6b6a1` |
| `builder-egress.txt` | `1716effbbf099e57c98d12d0fc787f8c24c1d0d89c661043de9a28ec9dc162cd` |
| `cargo-audit-final.txt` | `109366eaed1a6ecc741e217739f6c1adad0f31eedeb4dd80bc3e498463746ae7` |
| `cargo-deny-final.txt` | `bf7521abd50709197910a2137cc4ff6db7495f708ab5ca4619bf2ea54dd0017c` |
| `yaml-record-check.txt` | `52febe73488e7ef408ccfb34d705764d1a5eff9a3f8c7d9a060a89dc18566a17` |
| `yaml-index.json` | `f828bbc759337eb661170feda5fc5ea0301fb4bcd202baa908bfba2ad7231656` |
