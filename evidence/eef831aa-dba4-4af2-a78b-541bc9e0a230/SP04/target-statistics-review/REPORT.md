# Target statistics review

Verdict: **APPROVE LIMITED** for `analyze_mapped_shadows` on product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a230`. Parent of the product commit `5af5db7e9bc27a32bacb4d617bd2393c8edc2cb6`. Tree `b3ca121831767fc2b717836889ef25d1e1a719a2`. Author evidence `02b84f4bf731f6a24dfc6aece454d7e875d86938`. Workspace HEAD when this evidence directory was opened is `source/repo-head-at-start.txt` (`bbc37839514a995100b176107549246605c49d36`). The overlay launch recorded a later workspace HEAD `49dc1ca62070a71057a09ba2bc7d4da722a032d5`. The archive under test stayed the fixed commit. Product files, `Cargo.toml`, `Cargo.lock`, and the ledger were not edited.

This review does not mark a parent card, M1-D06A, M1-I01, M1-I02, or a release.

## What was reviewed

Contract `contracts/pipeline/target-statistics-v1.md` and `crates/oo-pipeline/src/target_statistics.rs` from `613aa981`. The public import is `import_unpublished`. The catalog read is the existing `oo-gateway` path: `install_column_acl`, `load_policy_catalog` with `UnknownRows::Refuse`, and `DedicatedSession::execute_trusted_select`.

Fixture, independent of the a219 north/south pair: source `a230src`, other source `a230alt`, generation 4, databases `oo_a230_src` and `oo_a230_dst`. Gold rows are `(7, keel, true)`, `(8, helm, false)`, `(9, mast, true)`. Sentinel table `public.sentinel`. Source writes are counted by `app.write_audit`.

Git blob ids in `source/blobs.txt`:

| Path | Blob |
| --- | --- |
| `crates/oo-pipeline/src/target_statistics.rs` | `c3cbbce6e5f3ce6e89148f9284ffe10623b8531b` |
| `crates/oo-pipeline/src/lib.rs` | `8d92236dc828604605428f06964d5db1ba3b057c` |
| `crates/oo-pipeline/tests/target_statistics_pg_live.rs` | `8aa879a8e2245d34368fd45384fdc637fbd93cb8` |
| `contracts/pipeline/target-statistics-v1.md` | `cb1755a694967f67c9d3f174949cd917a9f9f0bc` |

Content SHA-256 from the green overlay, `linux/overlay/owned-sha256.txt`: product `target_statistics.rs` `87543126c13e6a785bc535713e2aec6c3a78184b721b2282062ddecaad950202`, `lib.rs` `97e8e6225851c9eb8daaa0028a7592af93a2b82b46b38b6e3c8fbfa6bd8e4b43`, author live test `8d0e462067ca8c236b096a296eddfa83b564c5f5386e56ee9be10990f721fb2c`, contract `5e38fe73ba77b8ced8d37be0d116b83e74d283c6c40015670ab4f84f23633960`, lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `blob-cmp.exit` is 0.

## Host and images

Linux arm64 Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a230-pg` id `f857a3ae0b04545fbcfb1062fcfe1f29a5dacaf033c11e121d33ceac5fa0e73b`, 1 GiB, 1 CPU, capabilities CHOWN, DAC_OVERRIDE, FOWNER, SETUID, and SETGID. Alias `oo-a230-pg`, SAN `DNS:oo-a230-pg`, no host port. `pg/up.exit` is 0. Autovacuum off. `server_tls=true` and `version_17_10=true`.

Each rust container had 6 GiB, 1 CPU, all capabilities dropped, and bridge network `oo-a230-eef831aa-net`. One rust builder for this task at a time. One postgres. Pre-overlay capacity: `own=0 global=1 host_cargo=0`. Green overlay capacity: `own=0 global=1 host_cargo=0`. Private cargo, target, CA, and passwords lived under `/tmp/oo-a230-eef831aa-dba4-4af2-a78b-541bc9e0a230`. The builder mount listed `admin.pass`, `ca.crt`, `login.pass`, `plain.pass`, `reader.pass`, `readonly.pass`, and `runtime.pass`. It had no private key.

Recipe origins are `recipes/ORIGIN.md`.

## Commands and exits

Pre-overlay container `oo-a230-preoverlay` id `d5a6e8da2ca7e5a39ced277adf2f1ef5101277e015019e722500cd725217a797`, docker exit 0, `OOMKilled=false`, memory 6442450944, NanoCpus 1000000000. The fixed archive contained `target_statistics.rs`. The review harness was absent. `product-present.exit` 0. `harness-absent.exit` 0.

| Phase | Exit |
| --- | --- |
| `cargo fetch --locked --offline` | 101 |
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-pipeline --lib target_statistics` | 0 |
| clippy component | 0 |
| `cargo clippy -p oo-pipeline --lib -- -D warnings` | 0 |

The two library tests passed: `closed_inputs_fail_before_a_connection` and `reltuples_rounding_is_not_an_exact_claim`.

Green overlay container `oo-a230-overlay` id `e1ff0eb8b2ea526ff217d47dafbac05cf4955e96e1614b0c9c5b98d67ec7904c`, docker exit 0, `OOMKilled=false`, same memory and CPU. The throwaway archive received only `target_statistics_a230_review.rs` and `target_statistics_a230_catalog.rs`. `archive-overlay.txt` records `product_unchanged`.

| Phase | Exit |
| --- | --- |
| overlay present, blob compare, owned hash, lib | 0 |
| `target_statistics_a230_review` ignored live test | 0 |
| clippy on `oo-pipeline` lib and that test | 0 |
| libclang extract | 0 |
| `target_statistics_a230_catalog` ignored live test | 0 |
| clippy on `oo-gateway` and that test | 0 |
| secret scan | 0 |
| live SHA-256 manifest | 0 |

`mapped_shadow_review_on_one_postgres` passed in 2.05s. `policy_catalog_reads_typed_gold` passed in 0.16s. Live fact digests are in `linux/overlay/live-sha256.txt`.

Attempt 1 is `linux/overlay/attempt-1/`. Container id `fc7a35f9c635e33595c654ce4692c1b590a4e65def4f4f2b548d8920ea168262`, docker exit 101, `OOMKilled=false`. The pipeline live test passed. The catalog test queried the three gold cells and then failed its own `pipeline_chain` check. See FINDINGS F1.

## Measured behavior

Import through `import_unpublished` returned `AwaitingPublish`, source `a230src`, generation 4, one table, 3 rows. Mapping generation 4. Config digest length 64 lowercase hex. `_oo.full_import_run.state` stayed `awaiting_publish` after statistics. `in_recovery=false`.

Before `analyze_mapped_shadows`, mapped `reltuples` bits were the -1 encoding and `public.sentinel` was the same. Source `app.party` stayed at -1. Audit count started at 3 and the delta after the run was 0. Reader writes stayed 0.

Refusals, each `Reusable`:

| Case | Result |
| --- | --- |
| project `a230alt` with source `a230src` | `Identity` |
| generation 0 | `Identity` |
| source `a230alt` with matching project | `StaleGeneration` |
| other config digest | `StaleGeneration` |
| generation 5 | `StaleGeneration` |
| logical table `app.hold` | `MissingTable` |
| `oo_a230_runtime` | `RuntimeCaller` |
| `oo_a230_readonly` | `ReadOnlyCaller` |
| `oo_a230_plain` without `MAINTAIN` | `RoleDenied` |
| owner changed to `oo_a230_plain` | `ForgedRelation` |
| mapped name replaced by a view | `ForgedRelation` |

The plain role could see generation 4 and the stored physical name, and `has_table_privilege(..., 'MAINTAIN')` was false. Owner and relkind were restored (`oo_target_owner`, relkind `r`). Mapped `reltuples` was still -1 after those refusals. Display text hid the gold labels and the passwords. The connection used for the identity refusals was idle afterward. A raw simple query on an idle connection recorded `statement_timestamp() > transaction_timestamp()` as false (`raw_simple_query_idle=false`).

One immediate `BEGIN`, with no sleep, then `analyze_mapped_shadows`: kind `OpenTransaction`, disposition `Reusable`, elapsed 0 ms, the following raw simple query stayed true, `reltuples` stayed -1, and `ROLLBACK` left the probe false. `statement_timeout_provoked=false`.

The explicit call returned protocol `target-statistics/v1`, `published=false`, `estimate_is_exact=false`, disposition `Reusable`, and the stored physical name. `reltuples_after_bits=1077936128`. `estimated_rows=3`. The harness records whether that field equals 3 and does not fail the run when it differs. `explicit_was_first_known=true`. The stored bits matched the receipt. The sentinel stayed -1. The caller probe was idle.

A second session held `ACCESS EXCLUSIVE` on the shadow. `lock_timeout_ms=400` and `statement_timeout_ms=5000` returned `LockTimeout` / `Reusable`. The probe was idle. The estimate bits were unchanged and were not -1.

The catalog loader saw the same bits `1077936128`, rounded field 3, and `estimate_treated_as_exact=false`. `install_column_acl` created `oo_r_clerk` on the stored physical table. `load_policy_catalog` returned one relation with `UnknownRows::Refuse`. `catalog_estimated_rows=3` was recorded. The pass condition for that field is that a row estimate was present. `SELECT id, label, active FROM app.party` through `execute_trusted_select` returned the three typed gold cells. `gold_match=true`. `statistics_published=false`. `ClassStatus::Confirmed` and semantic type `text` were inputs to the existing grant API. This run did not emit a classification result.

`SHA256SUMS` is `tools/sha256.rs` over this directory after these reports and `cleanup/run.log`, skipping `SHA256SUMS` and `SHA256SUMS.sha256`.
