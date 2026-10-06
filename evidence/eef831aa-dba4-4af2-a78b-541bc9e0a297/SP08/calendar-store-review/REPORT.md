# Calendar store independent review

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a297`. Fixed archive `cc98bab7eba2c58c127435db4d2ae777797259de`, tree `a9369aab2c53afdf94616abee66b5099adc46d3e`. Helper tree `tools/business-calendar` at that commit is `ea5778dfb87a7cfdb04af2f236db78fe97f7bc4b`. The smoke launch recorded repo HEAD `5ce8b1c4186eb80f9b876a306ae94309f10933ae`. The product tree was `git archive` of the fixed commit. Later repo HEAD movement is outside this proof.

This review does not accept the author session `a289` as its own proof. Author tests ran here, on this archive, against a new private server. `a283` remains the helper review at `ec8b3953cdcb63035d64fbeca3310214aab002ef` and is not a review of this store.

The parent ledger was not edited. This file does not accept SEM-D03 for the parent, does not close SP08, does not accept authentication, finance approval, ontology publication, or a shared multi-project calendar, and does not certify a release.

## What was built

`tools/calendar-store` at the fixed archive is its own workspace. It path-depends on `tools/business-calendar`, which path-depends on `crates/oo-core`. `oo-core` inherits `edition`, `license`, `publish`, `rust-version`, `version`, and lints from the repo root workspace. The root manifest also lists members and `[patch.crates-io]` paths that this slice does not contain.

`linux/smoke-2`, `linux/overlay-1`, and `linux/restart` each probed that archived root manifest with `cargo metadata --locked --offline --no-deps` on `crates/oo-core`. The probe exited 101:

```text
error: failed to load manifest for workspace member `/work/repo/crates/oo-jobs`
```

The work-tree root manifest was then replaced by `harness-Cargo.toml` (SHA-256 `8c5072a915d6f149fc7ab4572be0e46106e345190dcbf51749fd4e2f38474667`). That harness lists only `crates/oo-core` and copies the inherited package and lint lines that are present in the archived root file (SHA-256 `a277789df0b8c74febf113a2acc80a0a9f4b51bdeaa662e5d4f761be2489046b`, git blob `c78ee5d0575a11cd6078cf0055cdd2c673edf583`). It has no patch table. The archived file stayed on the read-only mount. The probe did not change the root lock (`root-lock-probe.txt` is `unchanged`). `calendar-store` kept its own workspace and `Cargo.lock`. `cargo update` was not invoked (`cargo-update-invoked.txt` is `no`).

Child lock SHA-256 `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44`. Root lock SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Both hashes match before and after smoke, overlay, and the retention run. `product-unchanged.exit` is 0 in each of those three runs.

`tools/lock_subset.rs` checked every registry triple. Smoke and overlay both recorded `registry_triples=208 missing=0 mismatch=0 direct_pins_present=true helper_pins_present=true path_packages=3`, and `lock-triple-count.txt` is 208. Path packages are `business-calendar` 0.1.0, `calendar-store` 0.1.0, and `oo-core` 0.1.0.

## Private server

PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a297-pg`, id `1e9bd7605c0848eeaf898103b934af2a388c6206c16311e60b1990fed2131575`. `pg/up.exit` is 0. Privileged false. Port bindings `{}`. Database name `oo_a289` because the frozen author test hardcodes it. Roles and the TLS certificate were created for this UUID. The fixture is mounted at `/run/oo-a289/fixture` for the author test and at `/run/oo-a297/fixture` for the overlay. Passwords stayed under `/tmp/oo-a297-eef831aa-dba4-4af2-a78b-541bc9e0a297/secrets`.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `release: 1.94.0`, `host: aarch64-unknown-linux-gnu`. Each builder had CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, Privileged false, OOMKilled false, network `oo-a297-eef831aa-net`.

## Pre-overlay author smoke

`linux/smoke-2`, container `fa3052dc6e9b4d5be849d134ad9a0ffc5c9d1e45005ed384a91269917d1c2616`, `docker-run.exit` 0.

`cargo test --locked --offline -- --test-threads=1 --skip restart_snapshot_persists` exited 0. `smoke-count.txt` is 6: three library tests, two contract tests, and `fresh_pg_contract`. `restart_snapshot_persists` was filtered out. `smoke-names.exit` is 0. The output does not name `a297_calendar`. `blob-cmp.exit` is 0. Offline fetch exited 101 on the empty cargo volume; online `cargo fetch --locked` exited 0.

## Restart, author read, overlay, Clippy

`linux/proof-2` restarted `oo-a297-pg` and then failed before a builder started, because `psql` defaulted to a `postgres` role that this server does not have. Both timestamp files are empty. That directory stays.

`linux/overlay-1` queried `pg_postmaster_start_time()` as `oo_fixture_admin`, with the password read inside the container from the mounted secret file.

| | postmaster start |
| --- | --- |
| before | `2026-10-06 11:01:15.444815+00` |
| after | `2026-10-06 11:02:21.551234+00` |

Container `9ab0b75484bceb20cee12ea323b3badd114ef34a950853cc7fcff29917f1b617`, `docker-run.exit` 0.

Before the overlay file was copied, `restart_snapshot_persists` exited 0 (1 passed, 1 filtered). `overlay-absent.exit` was 0 at that point. The independent test was then copied into the work tree only.

`independent_store` exited 0 in 1.60s. `overlay-count.txt` is 1. The test source is `overlay/a297_calendar.rs`. It resets the author schemas, then checks collision, replay, project binding, allowlist, the four deployment flags, install drift on the migration hash and on an extra published column, resolved-hash mismatch, a refused `as_of`, snapshot drift that leaves 64 zero bytes in place, and the handwritten April, leap-day, Los Angeles, São Paulo, and Paris rows. São Paulo `today` is civil `2017-10-15` with status `nonexistent` and a NULL instant. Paris `today` is civil `1976-09-26` with status `ambiguous`, a NULL instant, and neither fold stored as `1976-09-26T00:00:00Z`, `1976-09-25T22:00:00Z`, or `1976-09-25T23:00:00Z`. A stale compare-and-swap left the pointer NULL. Revoking `oo_calendar_view` from `oo_calendar_admin` made activation return `StoreError::Query` with SQLSTATE `42501` and rolled the pointer back. Two concurrent activations from a NULL pointer produced one success with `grants == 1` and one `CasConflict`. A second revoked-membership activation left the winner in place. The final active row is `a297sp`. The runtime role and `SET ROLE oo_calendar_view` received `42501` on control and business tables. `oo_r_other` received `42501` on the view. `read_view` refused `oo_calendar_admin` and `oo_calendar_view` with `LoginRole`. The view has the 34 typed columns, owner `oo_calendar_view`, `rolcanlogin` false, and reloptions containing `security_barrier=true` and `security_invoker=false`. The view definition text has no `now(`, `current_date`, `current_timestamp`, `clock_timestamp`, `date_trunc`, `current_setting`, `closure_label`, `calendar_yaml`, `report_json`, or `closure_record_sha256`. Stored `as_of` differed from `statement_timestamp()`. `finance_approval`, `authenticated`, `ontology_published`, and `clock_used` stayed false.

`rustup component add clippy` exited 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0 (`Finished` in 1m 05s, empty stdout).

## Second restart

`linux/restart` recorded another postmaster change:

| | postmaster start |
| --- | --- |
| before | `2026-10-06 11:02:21.551234+00` |
| after | `2026-10-06 11:03:54.104408+00` |

Container `9f692a1253f2b090b0568f4fdef1478d935070c47522c14767e985a09775e12f`, `docker-run.exit` 0. Only `restart_keeps_the_explicit_row` ran. It exited 0 in 0.15s. `overlay-count.txt` is 1. The read is the `a297sp` row, six snapshots, the damaged drift hash of 64 zeros, the private closure label `a297-close-2026-03`, the same view definition bans, deployment flags false, and install disposition `Replay`.

## Author evidence left as it was

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/` was not modified. Its `linux/impl-1` failure and the overwritten first `pg/up.sh` remain there. This review used a new server and new directories. See `FINDINGS.md`.
