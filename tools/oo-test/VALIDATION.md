# H-S0 validation summary

Card status: partial. All 13 requested versions pass real fixture T0; literal native Oracle shape coverage remains incomplete. No product T1-T8 or doctor acceptance is claimed.

## Commands and results

Each matrix command used `tools/oo-test/oo-test matrix --engine <engine> --versions <versions> --suites T0 --baseline evidence/h-s0-d7687655-7c28-4a68-81c5-3a7a24703a65/mysql-5.7.44/gold-answers.json`. Defaults: 2,000,000 events and 200 small tables. Each run verified all 231 named table counts, 300 columns, view totals and reader privileges.

| Engine | Actual server version | Execution | T0 / all 20 gold | Evidence run-id |
|---|---|---|---|---|
| mysql | 5.7.44 | emulated | pass / identical | `h-s0-698b5508-a8cc-40f2-9746-89a39826411b` |
| mysql | 8.0.46 | native | pass / identical | `h-s0-698b5508-a8cc-40f2-9746-89a39826411b` |
| mysql | 8.4.11 | native | pass / identical | `h-s0-698b5508-a8cc-40f2-9746-89a39826411b` |
| mysql | 9.7.2 | native | pass / identical | `h-s0-698b5508-a8cc-40f2-9746-89a39826411b` |
| postgres | 13.23 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| postgres | 14.23 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| postgres | 15.18 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| postgres | 16.14 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| postgres | 17.10 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| postgres | 18.4 | native | pass / identical | `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` |
| oracle | 23.5.0.24.07 | native | pass / identical | `h-s0-98fce515-d0f3-4858-b4e0-004dbe2ee395` |
| oracle | 21.3.0.0.0 | emulated | pass / identical | `h-s0-98fce515-d0f3-4858-b4e0-004dbe2ee395` |
| oracle | 11.2.0.2.0 | emulated | pass / identical | `h-s0-98fce515-d0f3-4858-b4e0-004dbe2ee395` |

All 13 source answer files have SHA-256 `8d3f1066bcfd814a5770df766e95eff9c6b4af65911c1faa38c27908531e44a8`.

- `python3 -m unittest discover -s tools/oo-test -v`: 16 tests pass (with PYTHONDONTWRITEBYTECODE=1).
- `cargo check --workspace --locked`: pass before each commit; no Rust crate touched and no Cargo.lock changes. Rust formatting/crate tests are not applicable.
- `python3 specs/scripts/check_spec.py --self-test`, `check_plan.py --self-test`, `check_v02_plan.py --self-test`: pass; `python3 execution/v0.2/render_todo.py --check`: pass.
- `docker buildx imagetools inspect`: 14 locked matrix image records match, including experimental PG12 (pin only, not T0). Evidence: `h-s0-pin-verify`.
- Remote stub exits 2 before Docker/SSH operations. Missing T1 hook exits 1 with `not_implemented`; its reduced smoke fixture is not scale acceptance. Evidence: `h-s0-build`, `h-s0-5aca2cf0-a39a-485b-8ee8-fd85ba777a19`.
- SHA256SUMS verified for every final matrix evidence file. No lane-h containers or volumes remain; pinned images remain cached.

## TODO disposition and native gaps

- S0-H-01: not fully done. Neutral generator and three-engine full-size loads pass, but Oracle uses binary VARCHAR PK plus NLSSORT unique index rather than a collated PK. Oracle 11g XE has no native partitioning; JSON is CLOB (validated on 21c/23ai). These substitutions are explicit in seed and runtime shape reports.
- S0-H-02: done. All 20 answers are source-computed and identical across all 13 runs.
- S0-H-03: done for card T0 scope. Standalone runner, source-read-only accounts, comparison API, suite hooks, failure evidence, resource ownership cleanup and checksums validated.
- S0-H-04: done. Pins and lock verified; 23.5 deliberately used for 23ai rather than the newer 26ai image line.
- S0-H-05: done. MySQL 5.7 and Oracle 21c/11g pass with automatic amd64 emulation and longer timeout; remote remains an explicit stub.

Owned Oracle 11g capability probes independently reject native column COLLATE syntax (ORA-00907) and range partitioning (ORA-00439). Both expected unsupported outcomes are recorded under `h-s0-oracle11-capabilities`; these are not successful native feature tests. Coordinator must accept the documented compatibility coverage or retain the S0-H-01 gap. No shared-file patch is requested.

## Preserved failures

Earlier evidence retains the reserved MySQL UTC_TIME identifier failure, loopback account mismatch, transient registry TLS failures, Oracle password length limit and SQL*Plus long-line rejection. Fixes are in the generator/runner and regression tests; failed runs were not converted to passes. One signed registry download query was redacted in place without changing its failure/exit, then its checksum regenerated.

## Evidence manifest hashes

| Evidence run-id | SHA-256 of SHA256SUMS |
|---|---|
| `h-s0-698b5508-a8cc-40f2-9746-89a39826411b` | `4c20005d73feba2440b5c9856c5c6156534438409f77d1e2c05b714fe0a91028` |
| `h-s0-1cb89481-0a1a-47df-834b-c37dd9835fc0` | `7c828461b5ebbb0966ef27ac75d5083bfe8f40b0e263cc6a73e2a05106dbd600` |
| `h-s0-98fce515-d0f3-4858-b4e0-004dbe2ee395` | `4907b397cdb5a78272329518f564567fc78f9296a8296d1ea9028b775465e087` |
| `h-s0-pin-verify` | `46335fe3f0f36e40c2b06f90b33c7a5a4d3e71115d4a919641b31549108942b6` |
| `h-s0-oracle11-capabilities` | `1fe5356bc162e36ccd5678b42db701f3d83ac72cf7c6e56c7af8ee8980eeb3b9` |
| `h-s0-build` | `614659338577ae10969e411c98b5c6f71f7c9362490fe94dd1f03f6e055d2ac0` |
