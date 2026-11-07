# H-S5 validation (2026-11-04)

Status: partial. Both TODO exits remain open. Implemented source audit,
P0 matrix/index generation, local launcher and nightly CI definition.
No product T1/T2, full nightly success, GitHub execution or three-night
stability is claimed.

## S5-H-01: audit plumbing verified, product acceptance blocked

Command:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/oo-test/audit_smoke.py \
  --targets mysql:5.7.44,mysql:8.0,mysql:8.4,mysql:9.x,postgres:13,postgres:14,postgres:15,postgres:16,postgres:17,postgres:18,oracle:23ai,oracle:21c,oracle:11g
```

Exit 0; all 13 reduced fixtures pass actual-version and reader-grant
verification, positive read capture, and negative denied-DELETE capture.
This is not concurrent snapshot/incremental acceptance.

Evidence: `h-s5-audit-smoke-e3c105d4-eaba-49d5-a3de-78b06e657ea1`.
Root SHA256SUMS hash:
`eb9934813a266b2b29603e6ac5988cc8751895921457cb5d0f55b549f42e53d6`.
Every nested manifest entry was rehashed successfully.

| Engine | Observed versions | Audit smoke | Execution |
|---|---|---|---|
| MySQL | 5.7.44 | pass | amd64 emulated |
| MySQL | 8.0.46, 8.4.11, 9.7.2 | pass each | native |
| PostgreSQL | 13.23, 14.23, 15.18, 16.14, 17.10, 18.4 | pass each | native |
| Oracle Free | 23.5.0.24.07 | pass, unified | native |
| Oracle XE | 21.3.0.0.0, 11.2.0.2.0 | pass each, standard extended | amd64 emulated |
| Oracle EE | 19c | not run; no licensed image lock | unavailable |

A real PostgreSQL 17 run with two deliberately no-op hooks returns 1 and
T7 fails for zero observed hook activity:
`h-s5-20261103T222710359298Z-079bd2a4-7ce1-46e2-8235-5c73627a9652`.
The hooks' zero exits are not product acceptance.

Preserved failures include deprecated Oracle 23ai standard-audit restart,
unavailable unified flush API, empty 21c mixed-mode unified capture,
SQL*Plus metadata classification, and 21c denied DML missing from
`AUDIT ALL STATEMENTS` alone. Explicit standard statement/DML auditing
fixes the last failure; source-side negative probes were retained.
Oracle standard audit can omit parse-time errors. Unknown SQL fails
closed; the classifier is fixture-specific, not a general SQL firewall.

## S5-H-02: three local attempts, consistently blocked

`PYTHONDONTWRITEBYTECODE=1 python3 ci/nightly_matrix.py --repeats 3`
returns 1. Evidence:
`h-s5-nightly-20261103T223508Z-a789df30-a67d-420a-90d8-10e652d76a8d`.

- Each attempt covers 14 P0 targets and emits 140 suite/variant rows.
- Every attempt fails on missing product hooks or the Oracle 19c lock.
- `repeatable_outcomes=true`, `stable=false`, `consecutive_nights=false`.
- All 420 combined index rows validate against the coordinator generator;
  each row's target manifest hash was recomputed successfully.
- Combined index SHA-256:
  `e6f59772a4b6b51bd8458e7ed4feae7f28380398b2b0cd39b157d1097bd8f144`.
- Generated candidate levels do not claim verified capabilities. Shared
  matrix/index files remain unchanged.

The nightly workflow reports failures in its job/step summary and uploads
evidence. No external recipient, scheduler or LaunchAgent was installed.

## Other checks

- `python3 -m unittest discover -s tools/oo-test -v`: 25 pass.
- `python3 -m unittest discover -s ci -v`: 39 pass.
- `cargo check --workspace --locked`: pass before each commit.
- `cargo xtask gates --check-manifest`: `manifest_coherent`, exit 0;
  `ci_passed=false`, no acceptance closed.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py --self-test`: pass.
- No Rust crate touched; Rust formatting/crate tests not applicable.
- No Cargo.lock changes or shared-file patch requested.
- No lane-h containers or volumes remain; cached images are retained.

Build/test logs are under `h-s5-build`. Coordinator follow-ups: supply
real product hooks (including doctor/resource tests), license/lock 19c,
review and ingest evidence, run GitHub CI and three consecutive nights.
P1/P2 audit backends are not implemented or claimed by this P0 runner.
