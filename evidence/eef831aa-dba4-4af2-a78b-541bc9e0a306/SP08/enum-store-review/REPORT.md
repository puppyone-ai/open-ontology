# Enum store independent review

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a306`. Fixed archive `72dc9e2db841de4de368216bbbe13d738f850ea9`, tree `bf6c8d28dcb47a8dcb1c6cc1e7b6bfc21b963e64`. Helper tree `tools/enum-history` at that commit is `701743f204495101b5efd20104bccb8d104df467`. The smoke launch recorded repo HEAD `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. The product tree was `git archive` of the fixed commit. Later repo HEAD movement is outside this proof.

This review does not accept the author session `a293` as its own proof. Author tests ran here, on this archive, against a new private server. `a278` reviewed enum-history at archive `7f500ff0a61a42ea606a281dd931099091a52ef6` and is not a review of this store or of helper tree `701743f2`.

The parent ledger was not edited. This file does not accept SEM-D04 for the parent, does not close SP08, does not accept authentication, finance certification, semantic certification, or a shared multi-project enum catalog, and does not certify a release.

## What was built

`tools/enum-store` at the fixed archive is its own workspace. It path-depends on `tools/enum-history`, which path-depends on `crates/oo-core`. `oo-core` inherits `edition`, `license`, `publish`, `rust-version`, `version`, and lints from the repo root workspace. The root manifest also lists members and patch paths that this slice does not contain.

`linux/smoke`, `linux/overlay`, and `linux/restart` each probed that archived root manifest with `cargo metadata --locked --offline --no-deps` on `crates/oo-core`. The probe exited 101:

```text
error: failed to load manifest for workspace member `/work/repo/crates/oo-jobs`
```

The work-tree root manifest was then replaced by `harness-Cargo.toml` (SHA-256 `8c5072a915d6f149fc7ab4572be0e46106e345190dcbf51749fd4e2f38474667`). That harness lists only `crates/oo-core` and copies the inherited package and lint lines that are present in the archived root file (SHA-256 `a277789df0b8c74febf113a2acc80a0a9f4b51bdeaa662e5d4f761be2489046b`, git blob `c78ee5d0575a11cd6078cf0055cdd2c673edf583`). It has no patch table. The archived file stayed on the read-only mount. The probe did not change the root lock (`root-lock-probe.txt` is `unchanged`). `enum-store` kept its own workspace and `Cargo.lock`. `cargo update` was not invoked (`cargo-update-invoked.txt` is `no`).

Child lock SHA-256 `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923`. Root lock SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Both hashes match before and after smoke, overlay, and the retention run. `product-unchanged.exit` is 0 in each of those three runs.

`tools/lock_subset.rs` checked every registry triple. Smoke and overlay both recorded `registry_triples=208 missing=0 mismatch=0 direct_pins_present=true helper_pins_present=true path_packages=3`, and `lock-triple-count.txt` is 208. Path packages are `enum-history` 0.1.0, `enum-store` 0.1.0, and `oo-core` 0.1.0.

## Private server

PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a306-pg`, id `4686624ce3dedf7a93585abc55d5b0f986476250f0e7125c5a9107cd132cd7e3`. `pg/up.exit` is 0. Privileged false. Port bindings `{}`. Database name `oo_a293` because the frozen author test hardcodes it. The certificate SAN is `DNS:oo-a306-pg`, and `endpoint.txt` names that host. Roles and the TLS material were created for this UUID. The fixture is mounted at `/run/oo-a293/fixture` for the author test and at `/run/oo-a306/fixture` for the overlay. Passwords stayed under `/tmp/oo-a306-eef831aa-dba4-4af2-a78b-541bc9e0a306/secrets`.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `release: 1.94.0`, `host: aarch64-unknown-linux-gnu`. Each builder had CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, Privileged false, OOMKilled false, network `oo-a306-eef831aa-net`. The image contains a Python binary (`python-present.flag` is 1). This review did not call it. The live oracle is `overlay/a306_enum.rs`. The inventory oracle is `tools/sha256_manifest.rs`.

## Pre-overlay author smoke

`linux/smoke`, container `6ad89543b99a5983ba7fe04c29a4f45b6da84b2dc69adb2339678a3215b31901`, `docker-run.exit` 0.

`cargo test --locked --offline -- --test-threads=1 --skip restart_snapshot_persists` exited 0. `smoke-count.txt` is 8: five library tests, two contract tests, and `fresh_pg_contract`. The doctest result added 0. `restart_snapshot_persists` was filtered out. `smoke-names.exit` is 0. The output does not name `a306_enum`. `blob-cmp.exit` is 0. Offline fetch exited 101 on the empty cargo volume; online `cargo fetch --locked` exited 0.

`rustup component add clippy` exited 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0 (`Finished` in 1m 05s).

## Restart, author read, independent live, Clippy

`linux/overlay` queried `pg_postmaster_start_time()` as `oo_fixture_admin`, with the password read inside the container from the mounted secret file.

| | postmaster start |
| --- | --- |
| before | `2026-10-06 12:02:25.354892+00` |
| after | `2026-10-06 12:05:57.404939+00` |

Container `8d0b8ef703a8675b585339b9f22ef476ee528dcd4c20fd7eaea3f9d660874b9c`, `docker-run.exit` 0.

Before the overlay file was copied, `restart_snapshot_persists` exited 0 (1 passed, 1 filtered). `overlay-absent.exit` was 0. The independent test was then copied into the work tree only.

`independent_store` exited 0 in 1.50s. `overlay-count.txt` is 1. The test source is `overlay/a306_enum.rs`. It drops the author schemas, then checks the create-privilege `42501`, a foreign `oo_dict` collision, install replay, project binding, allowlist, install drift on the migration hash and on an extra snapshot column, a wrong report digest, `as_of` `not-a-date`, and `inferred_unapproved`. A wrong CA is `Connect` or `Tls`. The handwritten historical row is enum `a306.receipt.state`, version `hist-2019`, civil `as_of` `2019-07-04`, code `7`, status `deprecated`, class `open`, description `prior open receipt`. The test compiled that YAML with `enum_history::compile` and checked `report_sha256` against `sha256_hex` of `serde_json::to_string`. The printed digest is `7b715c151e8b63392c4df73a7c507d09ea5862b834eca53b5174831cd2b3094d`. Ratio `4/4`, comparison `above`, threshold defaulted, decimal `0.95`. Putting the same identity with threshold spelling `0.95` is `SnapshotDrift`, and `threshold_defaulted` stays `true`. A second version can be inserted. Changing its published status to `approved` and putting the original payload again is `SnapshotDrift`; the damaged status stays `approved` and the historical row stays `deprecated`.

A stale compare-and-swap left the pointer NULL. A wrong config digest is `Activation` and left the pointer NULL. Revoking `oo_enum_view` from `oo_enum_admin` made activation return `StoreError::Query` with SQLSTATE `42501`, left the pointer NULL, and left `served` empty.

The retroactive YAML has code `7` superseded then approved. Snapshots `retro-2021` at `2021-12-15` and `retro-2022` at `2022-01-01` keep statuses `superseded` then `approved`, and the later classes are `open` then `closed`. Coverage observations `("7", 95), ("8", 5)` print digest `559091929746e4012afc17176f71c27d5e5f03c8c915beac15ba4b2b07dd6d25`, ratio `95/100`, comparison `equal`, `threshold_met` true, `review_needed` true, notice code `8` reason `unknown_code` count `5`. Observations `("7", 94), ("8", 6)` print digest `51400204dd207f3cd866480a3f3a11d8cfc286ae62fc13d694e2539af6043bf1`, ratio `94/100`, comparison `below`, `threshold_met` false, notice count `6`. Snapshot count is 6.

Two concurrent activations from a NULL pointer produced one success with `grants == 1` and one `CasConflict`. The served view was moved to the coverage snapshot when the winner was the other snapshot. The reader saw code `7` status `approved` class `open` description NULL, and code `8` status `unexplained` with a null class, a null description, and null validity dates. The final compare-and-swap selected `hist-2019`. The view `as_of` was `2019-07-04`, which differed from `current_date::text` in the test. Product SQL does not call `current_date`.

Runtime `UPDATE`, `DELETE`, and `INSERT` on `oo_dict.enums` returned SQLSTATE `42501`. Runtime `SELECT` of `enum_yaml`, `report_json`, notice, published, served, and `biz.receipts` returned `42501`. `SET ROLE oo_enum_view` can select `oo_enum_control.served` (count 1) and received `42501` on snapshot YAML, report JSON, notices, published rows, and `biz.receipts`. `oo_r_other` received `42501` on the view. `read_view` as `oo_enum_admin` is `LoginRole`. A second revoked-membership activation, expecting the historical pointer, returned `42501` and left that pointer and the deprecated served row in place.

The view columns are `enum_id`, `code`, `description`, `class`, `valid_from`, `valid_to`, `version`, `as_of`, `project_id`, `status`. The owner is `oo_enum_view`. `rolcanlogin` is false. Reloptions contain `security_barrier=true` and `security_invoker=false`. The view definition text has no `now(`, `current_date`, `current_timestamp`, `clock_timestamp`, `date_trunc`, `current_setting`, `enum_yaml`, `report_json`, or `row_kind`. Deployment flags `finance_certified`, `semantic_certified`, `owner_authenticated`, `metric_certified`, and `clock_used` are false. Store receipts set `database_used` true and those five flags false. The nested report JSON contains the six helper flags false, `"historical_business_class":true`, and `"status":"deprecated"`.

`cargo clippy --locked --offline --all-targets -- -D warnings` on that same work tree exited 0 (`Finished` in 1.41s). `product-unchanged.exit` is 0. The overlay file is not part of the product hash list.

## Second restart

`linux/restart` recorded another postmaster change:

| | postmaster start |
| --- | --- |
| before | `2026-10-06 12:05:57.404939+00` |
| after | `2026-10-06 12:06:22.319356+00` |

Container `25f8e0fd4829c6a7700a2b4993e3ec601ecccf014803bdc0db51a7e607480b80`, `docker-run.exit` 0. Only `restart_keeps_the_explicit_row` ran. It exited 0 in 0.22s. `overlay-count.txt` is 1. The read is the `hist-2019` row, six snapshots, the damaged drift status `approved`, the coverage notice `unknown_code` for code `8`, ratios `95/100` and `94/100`, a fresh compile digest equal to the stored `report_sha256`, snapshot disposition `Replay`, install disposition `Replay`, retro statuses `superseded` then `approved`, and the same view definition bans. Deployment flags stay false.

## Author evidence left as it was

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/` was not modified. Its `linux/precode` read-only `mv` record, the `toolchain-aside.txt` text `moved-aside`, and clippy exits 101 in `linux/impl-1` and `linux/impl-2` remain there. This review did not recreate those runs. See `FINDINGS.md`.
