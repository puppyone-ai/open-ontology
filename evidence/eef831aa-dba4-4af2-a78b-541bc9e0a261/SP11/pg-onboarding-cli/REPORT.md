# SP11-W06 PostgreSQL onboarding CLI

Session `eef831aa-dba4-4af2-a78b-541bc9e0a261`. Fixed product archive `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368`. The owned tool is `tools/pg-onboarding`. This report covers that CLI and this evidence directory. It does not accept a parent unit, M1-I01, M1-I02, or a release.

## Result

The live proof status is `ready_for_trusted_query`.

Inspect sealed a captured draft. `source_cut` is 48, `target_fence` is 1, and `mapping_run_generation` is 48. The config identity is `a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618`, recomputed from the non-secret import fields. Apply ran the reviewed compilers and a per-role `query-pg` smoke. `analyze_published` is false. `estimate_is_exact` is false. `database_rollback` is false. `database_release` is false. Authentication, semantic publication, natural language, and harness mount are not claimed.

Clerk provision effect is `installed`. Peek provision effect is `replay`. Hidden column `shop.customer.private_note` has reason `classification.absent`.

The proof oracle is `tools/pg-onboarding/tests/live_chain.rs`. Chain attempt 2 wrote `live/attempt-2/work/oracle-ok.txt` with `ready_for_trusted_query`. `linux/chain-attempt-2/live.exit` is 0. `linux/chain-attempt-2/chain.exit` is 0.

## Tool commits

- `567d621e2e52b7e0011bdb025dc9337e92ecc5a6` adds the standalone crate.
- `1b314d73d5a105898921e453ef0382fdad96d069` sets the live analyze statement timeout to 30000 ms. The chain mounted this commit. `linux/chain-attempt-2/tool-commit.txt` records it.

Dependencies stay on the exact pins `serde` 1.0.229 and `serde_json` 1.0.151. No `cargo update`. No product crate edit. No root manifest or root lock edit.

## Builders

Linux arm64 Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. New builders used 6 GiB and 1 CPU, with every capability dropped. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 17.10, 1 GiB, 1 CPU.

| Run | Exit | Record |
| --- | --- | --- |
| smoke | 101 | `linux/smoke`. Missing `OO_A234_*` compiler binaries. |
| smoke-attempt-2 | 0 | Ten compiled CLI suites. |
| postedit | 101 | `Value` compared with `&Value` in `check_install`. |
| postedit-attempt-2 | 101 | Tests passed. Clippy rejected four lints. |
| postedit-attempt-3 | 101 | Tests passed. Clippy rejected two useless `format!` calls. |
| postedit-attempt-4 | 0 | 3 tests passed, `live_chain` ignored, Clippy clean. |
| chain | 101 | Inspect sealed. `analyze-pg` exit 3, stderr `analyze-pg: bound`. |
| chain-attempt-2 | 0 | Fresh cluster. Live oracle passed. |

Post-edit toolchain text is `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

## Live fixture

Private network `oo-a261-eef831aa-net`. Alias `oo-a261-pg`. Databases `oo_a261_src` and `oo_a261_dst`. Schema `shop`, tables `customer` and `order_doc`. Roles `oo_a261_reader`, `oo_a261_runtime`, and shared login `oo_a261_login` for slugs `clerk` and `peek`. Amount values are the int8 extremes and zero. Autovacuum was off. TLS used a private CA. Passwords stayed in files under `/tmp/oo-a261-eef831aa-dba4-4af2-a78b-541bc9e0a261/secrets` and were not placed on argv.

The host watcher granted `oo_pipeline_runtime` to the runtime login after init created that role, then wrote the ready file. The CLI does not issue that grant. It waits for the ready file.

Chain attempt 1 left a bootstrapped target. Attempt 2 removed only the a261 container `oo-a261-pg` and volume `oo-a261-eef831aa-pgdata`, then created a new cluster. The first run directory remains at `live/run`.

## Caller

The config field `caller` must be `trusted_local_admin`. This is an operator config on a local machine. It is not a public authentication check.
