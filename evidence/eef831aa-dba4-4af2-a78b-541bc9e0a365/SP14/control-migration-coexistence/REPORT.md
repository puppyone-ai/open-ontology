# Shared `oo_dict` install repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a365`. Ledger a365. This packet repairs the calendar-store and enum-store installers so both products can share one control database. It is not a new acceptance unit. Parent SP14 and E51 stay open. Nothing here auto-accepts SP14-W04, SEM-D03, or SEM-D04.

Pre-commit HEAD is `6283bc16f6d408ffa38dbbf8837cc4f80268bbcb`. The live proof was collected at `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`. The two commits between them do not touch calendar-store or enum-store. The commit that contains this directory is the packet commit. Its hash is not stored in the tree.

## Verdict on the install path

`install()` composes. Calendar then enum, and enum then calendar, each install, replay, activate, and read on one private PostgreSQL 17.10. A foreign owner, an empty sibling-owned schema, a table standing in for the sibling view, and a view whose `security_invoker` was altered all refuse. The refusal leaves the caller's private schema absent, or, on replay drift, rolls the transaction back.

The accepted SQL files are unchanged:

| file | SHA-256 |
|---|---|
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

`git diff` on both files and on both child `Cargo.lock` files is empty. Raw `psql` of those two files on one database still stops at `CREATE SCHEMA oo_dict` (calendar line 222, enum line 252). That failure is what keeps the digests true. The supported shared path is `install()`, which splits the same bytes and calls the sibling's `SECURITY DEFINER` open and close functions.

## What ran

Linux arm64, Rust `1.94.0 (4a4ef493e 2026-03-02)`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/version.txt` is `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Both images were already local. Nothing was pulled. No `cargo update`, no `cargo generate-lockfile`, no root manifest edit. Child locks stayed `package_names=211 source_lines=208`.

Pre-edit smoke `linux/pre-edit-smoke/inside.exit` is 0. `linux/smoke-attempt-1/docker-run.exit` is 1 and is kept. Its `WHY.txt` records the lock-shape pin that the retry corrected.

One builder, 6 GiB, 1 CPU, and one database, 1 GiB, 1 CPU, on network `oo-a365-eef831aa-net` (`internal=false`, not host networking, no published port). Labels `oo.owner=a365` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a365`. Identifiers at packet time are in `proof/resources-at-packet.txt`. The builder process list was `sleep infinity`. PostgreSQL was accepting connections. `oo-a370-builder` was also running and was left alone.

The proof archive base recorded in `proof/archive-commit.txt` is `9f60eddf9f6e701aee55d0eeb04ee00912bfe8a8`. Before the green shared-database phases, `linux/resume-proof-6.sh` copied the working-tree `sql.rs`, `adopt.rs`, `store.rs`, and `CONTRACT.md` for both crates into that archive. After the proof, those files plus `lib.rs`, both `tests/coexist.rs`, both `tests/contract.rs`, both migrations, and both `Cargo.lock` files compared equal to the archive.

## Live exits

Current phase logs, all exit 0. Attempt directories are listed in `FAILED-ATTEMPTS.md` and were not rerun.

| Phase | Log | Result line |
|---|---|---|
| fetch | `proof/fetch.log` | exit 0 |
| calendar lib + contract | `proof/calendar-lib-contract.log` | 4 passed, then 2 passed |
| enum lib + contract | `proof/enum-lib-contract.log` | 6 passed, then 2 passed |
| calendar solo `oo_a365_solo` | `proof/calendar-store-solo.log` | 1 passed, 0.38s |
| enum roles | `proof/enum-roles.log` | exit 0 |
| enum solo reject | `proof/enum-store-solo_reject.log` | 1 passed, 0.09s |
| calendar solo replay | `proof/calendar-store-solo_replay.log` | 1 passed, 0.10s |
| calendar then enum | `proof/calendar-store-cal_first.log`, `proof/enum-store-cal_first.log` | 0.42s, 0.36s |
| both replays on that database | `*-cal_first_replay.log` | 0.14s, 0.14s |
| enum then calendar | `proof/enum-store-enum_first.log`, `proof/calendar-store-enum_first.log` | 0.36s, 0.37s |
| both replays on that database | `*-enum_first_replay.log` | 0.19s, 0.14s |
| hostile, each crate | `*-hostile.log` | 0.23s, 0.22s |
| drift, each crate | `*-drift.log` | 0.16s, 0.10s |
| calendar clippy | `proof/calendar-clippy.log` | Finished dev profile in 1m 03s |
| enum clippy | `proof/enum-clippy.log` | Finished dev profile in 1.28s |
| driver | `proof/proof.exit`, `proof/drivers/resume6-tail.txt` | 0, `proof_ok` |

Coexist command: `cargo test --locked --offline --test coexist -- --exact phase`. Clippy command: `cargo clippy --locked --offline --all-targets -- -D warnings`. Lib command, from `linux/continue-proof.sh`: `cargo test --locked --offline --lib --test contract`.

The lib logs still print `unused import: VIEW_ROLE` and `unused constant ACCEPTED_MIGRATION_SHA256`. Those two commands were not run again. The later source uses the constant in `migration_sha256` and uses `VIEW_ROLE` in the store. `resume-proof-6.sh` compiled that source. Both clippy logs contain no `warning:` line.

`oo_a365_solo` was not dropped. `oo_a365_cal_first` was recreated only by `reset-cal-first` and `reset-cal-first-2`, both exit 0, before the green calendar-first pair. `linux/resume-proof-4.sh` and `linux/resume-proof-5.sh` now exit 2 at the first line so they cannot drop that catalog again. `linux/resume-proof-6.sh` is the script that produced `proof_ok`. Running it again would drop `oo_a365_cal_first`.

## Behavior the exits cover

Both crates take advisory lock `881275280` before the product lock (`881275289` calendar, `881275293` enum). The first installer owns `oo_dict` and runs the accepted SQL unchanged, then creates `dict_create_open`, `dict_create_close`, and, when the sibling admin and view roles are safe, `dict_grant_runtime(runtime_role text)`. The second installer does not execute `CREATE SCHEMA` and does not `ALTER SCHEMA`. It calls the owner's open function, creates its view, calls the close function, and creates its own functions.

Activate grants `USAGE` on `oo_dict` directly when the caller owns the schema. Otherwise it calls the owner's `dict_grant_runtime`. PostgreSQL 17.10 makes a non-owner `GRANT USAGE` return success while granting nothing (`no privileges were granted`). Activate then reads `has_schema_privilege` and returns `Activation` when the runtime role still lacks `USAGE`. The runtime role must already be named in the sibling `runtime_allow`.

Hostile schemas and a drifted `security_invoker=true` return `Collision { check: "oo_dict" }` or `InstallDrift` with no partial private schema from the failing call. Historical solo catalogs, which have no adoption functions, still replay. Allowlisted runtime `USAGE` after activate is not drift. The owner's `nspacl` row is not an extra grant.

Project id in the coexist fixture is `project-one`. Runtime roles are `oo_r_calendar` and `oo_r_enum`. Product admins are `NOCREATEDB NOCREATEROLE NOSUPERUSER NOBYPASSRLS`. View roles are `NOLOGIN`. Connections use `SslMode::Require`.

## Still open

Parent SP14 and E51 stay open. `w04_gate` stays open. This packet does not close `enum_calendar_same_database` in a parent ledger. Raw application of the two SQL files on one database is still unsupported. A first install that runs before the sibling roles exist does not grow the `EXECUTE` grant when those roles appear later; the second install returns collision and creates nothing. No production call, no customer call, no push, no release.
