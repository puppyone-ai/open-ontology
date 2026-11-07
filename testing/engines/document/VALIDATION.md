# F-S5 resume validation (2026-11-05)

S5-F-02 remains partial. No T0/T8 or product MongoDB C1 acceptance.

| Engine | Version | Run | T1 | T2 C5 | Cleanup | Summary SHA256 |
|---|---|---|---|---|---|---|
| mongodb | 8.0.34 | f-s5-mongo-20261105d | pass | 3 cycles pass | clean | `c803af36b8837a6febe7fd99a7116a123f591b6a9918aacb3a09a8562c379215` |
| opensearch | 3.9.0 | f-s5-os-20261105b | pass | 3 cycles pass | clean | `c99637d4d8951ff9db46d31c4007f95f7fad7cd28ef767ea92a9494eb152d45c` |
| elasticsearch | 9.2.0 | f-s5-es-20261105b | pass | 3 cycles pass | clean | `386813bf0dc00c748d2eea9fde85eb0695478630e1894bfcf8ca0d1be755b4dc` |

Commands: `python3 testing/engines/document/run.py --engine ENGINE --run-id RUN`.
Explicit index rows are beside each raw summary and are bound to its SHA256.
All live runs used linux/arm64, isolated authenticated sources and PostgreSQL Agent DB.
T1 used a concurrent admin writer after the first committed target chunk; comparison
is against the pinned source cut, not the later live state. C5 compares every cycle;
cycle 2 asserts zero chunk payload, target write bytes, range replacements and generations.

Validation logs: `evidence/f-s5-resume-20261105/`.
- Workspace locked checks pass. All three spec/plan integrity checks pass.
- oo-test Python unit tests: 110 pass.
- Connector feature unit tests: 127 pass.
- Pipeline library tests: 260 pass, 15 ignored.
- oo-load feature tests: 4 library, 33 binary, 2 preflight pass; live tests ignored.
- cargo deny (standalone oo-load, document-sources, ci/deny.toml): all categories pass.
- cargo audit (standalone lock): exit 0, no vulnerabilities reported.

Retained failed attempts: f-s5-mongo-20261105 (runner log race),
f-s5-mongo-20261105b (retained recipe type mismatch), f-s5-mongo-20261105c
(mongosh export framing), f-s5-os-20261105 and f-s5-es-20261105
(new scheduled-sync dispatch guard rejected C5). None was overwritten.

Gaps: product doctor, measured source connections, full conformance dataset,
persisted-range growth splitting, actual valid-token oplog eviction, transactional
MongoDB C1/alignment/replay. Shared PgSync comparator patch remains unapplied.
Client-side digests still transfer full documents during idle C5.
Source tests are not substitutes for these missing exits.
Root Cargo.lock unchanged; standalone oo-load lock gains optional MongoDB dependencies.
