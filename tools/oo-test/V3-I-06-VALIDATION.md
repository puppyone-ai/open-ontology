# V3-I-06 installed model acceptance

STATUS: **partial**. V3-I-06 is **not done**. Date: 2026-11-11.

## Delivered

- `4c9cb8084`: move demo q02 from accuracy to permission. Its question,
  gold SQL and exact typed answer remain unchanged in the manifest.
  Historical SQL recordings remain failures on that permission case.
- `fe59b5e1e`: mount explicit private credential file sources read-only in
  place. Do not copy them into the clean HOME. Named credentials still require
  an outside-Git snapshot and retain the explicit-rotation check.

Only task-owned resources were used. No source fixture or ERP gold was changed.
No shared engine files, Cargo.lock, original model files or customer systems
were modified. The pre-existing untracked dispatch script was left alone.

## Own database: reproducible load blocker

Fresh partial darwin-arm64 npm package, real postinstall, empty HOME, owned
MySQL **8.4.7**, original xs/seed-42 OO-Logistics ERP fixture and a separate
SELECT/SHOW VIEW TLS account. Both model inputs were file references; the
requested model was `gpt-6.1-sol`.

The initial installed run refused with `LITE_MODEL_STATE_IN_GIT`. After the
file-source fix, fresh installation and lite provisioning passed twice.
The final attempt is `v3-i06-mysql3-20261111`:

| Stage | Seconds | Result |
| --- | ---: | --- |
| Fixture startup | 7.965 | pass |
| Admin fixture seed | 2.060 | pass |
| TLS setup | 2.938 | pass |
| npm pack | 5.314 | pass |
| npm install/postinstall | 1.296 | pass |
| `oo init --tables-recommended --json` | 1.043 | eight recommendations |
| `oo init --tables all --json` | 0.165 | ten eligible tables |
| `oo up --config ... --json` | 6.854 | retained-ready foundation |
| `oo build --config ... --json` | 1.574 | **BUILD_LOAD_FAILED** |
| `oo down --config ... --json` | 3.923 | pass |

The two eligible maybes were explicitly included because the frozen ERP bank
requires all ten tables; this does not claim that they were recommended.
An owned, identically configured `oo-load` diagnostic returns:

```json
{"error":"source error: catalog fact=unsupported_target_type","status":"failed"}
```

The fixture's `kh.hlje decimal(40,12)` maps to Arrow Decimal256 in
`crates/oo-connectors/src/mysql/map.rs`, but the coordinator-owned
`crates/oo-connectors/src/engine/shape.rs` only maps Decimal128.
`V3-I-06-shared.patch` is the exact proposed shared-file change, **not applied**.
An isolated rustc probe of the current mapping reproduces the error; the
proposed mapping returns `numeric(40,12)` and its isolated test passes.
This is not a patched runtime or full-crate acceptance.

There is no demonstrated workload-preserving workaround within this card:
changing the shared mapping is prohibited, and dropping or rewriting the
fixture column would avoid rather than test the supplied fixture. Additionally,
`native_c1_apply::cell_text`, used by the chunk loader, has no Decimal256 arm;
the mapping patch alone must not be called an end-to-end fix.

Self-build, adjudication, graph/confirmation, publication, profile, own-database
readiness, three queries and answerability were **not reached**. Own-database
time to first answer is **unmeasured**. The zh 120x40 full-wizard run and its
deliver-screen transcript were **not run**. ERP live A/B and ERP replay remain
**not run**, because there is no published ERP Pack. None is counted as passed.

## Demo live A/B

Both captures use authenticated published demo Packs, seed 42, pinned model
`gpt-6.1-sol`, two arms, unchanged questions/gold, and successful load-time
gold preflight. Each capture's replay produces byte-identical report bytes.
Replay reproducibility does not mean independent live calls are deterministic.

| Capture | Track | Relative change | Fixed | Broken | No-output C0/C3 |
| --- | --- | ---: | --- | --- | --- |
| `v3-i06-demo-ab-20261111` | accuracy (2) | 0% | none | none | 0/0 |
| same | permission (2) | 0% | none | none | 0/0 |
| same | injection (1) | 0% | none | none | 0/0 |
| `v3-i06-finaldemo-ab-20261111` | accuracy (2) | **-50%** | none | q16 | 0/0 |
| same | permission (2) | 0% | none | none | 0/0 |
| same | injection (1) | 0% | none | none | 0/0 |

Capture durations: 21.841 s and 19.487 s. All per-arm health gates pass.
Final C3 q16 is `query_failed`, not missing output. All security responses are
model refusals, **not proof of Gateway enforcement**. The independent QS-03
test supplies the Gateway denial/audit evidence. No question was tuned.

## Regression and cleanup

Main evidence: `evidence/v3-i06-20261111/`.

- `cargo check --workspace --locked`: pass before each commit.
- `cargo test --locked -p oo-eval`: two Rust and 30 Python tests pass.
- `cargo test --locked -p oo-cli`: 188 pass, three ignored, zero failures.
- `cargo fmt --check -p oo-cli -p oo-eval`: pass.
- `python3 -m unittest discover -s deploy/lite -p test_lifecycle.py`:
  23 pass, including permissions, symlink refusal and in-place mounts.
- Final fresh installed-package QS-01..05: all pass on PostgreSQL **17.10**,
  `v3-i06-finaldemo-qs-20261111`. Install 1.185 s, deployment 14.092 s,
  first query 0.279 s, install-to-first-answer **15.556 s**, down 0.839 s.
- Both demo captures and both replays exit zero. No pre-existing-failure
  exemption was needed.

These are warm-image local debug-native measurements, not release/cold-download
or cross-platform acceptance. The existing runtime digest is
`sha256:de6131a20e0fe91c16bf2a414da4b942330551fad17ef441d6d4174ec644517f`;
it was not rebuilt and retains the previously disclosed Rust-build deviation.

Task containers and networks are absent. Normal own-database down retained
four volumes; all four were separately ownership-checked and removed.
Generated installation, credential and runtime-state artifacts were hashed
then removed after preserving failure receipts. Scoped resource checks contain
no unrelated resource names. Retained evidence was scanned using the shared
private-file reader for exact model material, credential signatures and eleven
private terms: zero hits. Original model files were neither copied nor changed.
