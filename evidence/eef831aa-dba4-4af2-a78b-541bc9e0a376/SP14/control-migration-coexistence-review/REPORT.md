# Control-migration coexistence review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a376`. Repair under review: `be4b596481d83435df2ede8abd9ebb2a8647c706`. Verdict: **APPROVE_LIMITED**.

This packet does not accept SP14, E51, SP14-W04, SEM-D03, SEM-D04, or `enum_calendar_same_database`. It adds no ledger count. Product trees `tools/calendar-store` and `tools/enum-store` were not modified. `git diff be4b596481d83435df2ede8abd9ebb2a8647c706 HEAD` on those two trees is empty at report HEAD `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8`. The oracle ran while HEAD was `55be6e88e191e89748c8e9e9905f074bda87e810`. The commit after that is a web test and does not touch these two trees. The proof archive is `daff88a3253bf2c5171bece48ade8309b17e7417`, a descendant of the repair and an ancestor of the report HEAD, with the same empty diff. See `proof/reviewed-commits.txt`.

## Verdict

`install()` shares one `oo_dict` in both orders on PostgreSQL 17.10 when both `NOSUPERUSER` role pairs already exist. Replay, put, activate, and read succeed on that path. A foreign owner, an empty sibling-owned schema, a table standing in for the sibling view, and a `security_invoker=true` sibling view are refused. The hostile refusal leaves both private schemas absent. The drift refusal rolls the install transaction back on a catalog that already had both private schemas.

Two limits stay on the approval:

1. Replay treats `search_path` as a substring. `tools/calendar-store/src/adopt.rs` and `tools/enum-store/src/adopt.rs` accept a function when `config.contains("search_path=pg_catalog")` (`function_state` and `runtime_grant_state`). After `ALTER FUNCTION oo_calendar_control.dict_grant_runtime(text) SET search_path = pg_catalog, public`, calendar `cal_first_replay` returned replay. `proof/facts-pathdrift.txt` records `{"search_path=pg_catalog, public"}` and `pathdrift|result|accepted`. `proof/calendar-store-pathdrift.log.exit` is 0. Functions collected before that `ALTER` are exactly `{search_path=pg_catalog}` in `proof/facts-green.txt`. The contract text says `search_path = pg_catalog`. The matcher does not require that exact config.

2. Raw `psql` of the two migration files on one database is still unsupported. That result is not a general fix. The supported shared path is Rust `install()`, which splits the same bytes. The SQL files still contain `CREATE SCHEMA oo_dict` (calendar line 222, enum line 252) and their digests are unchanged.

## Pinned bytes

| file | SHA-256 |
|---|---|
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

`proof/oracle.log` prints `oracle_ok` with those digests. Both `CONTRACT.md` files contain `Both role pairs may exist before either install.` and both digests. Both child locks stayed `package_names=211 source_lines=208`. `proof/fetch.log` ends with `cargo-update-invoked=no`. No root manifest edit, no `cargo update`, no `cargo generate-lockfile`.

## What ran

Pre-edit smoke, before the oracle and before any proof edit that changed a check: Linux arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `linux/pre-edit-smoke/inside.exit` 0, `product-drift.exit` 0.

One private PostgreSQL 17.10, `pg/version.txt` = `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, network `oo-a376-eef831aa-net`, alias `oo-a365-pg`, no published port. Builder image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, 6 GiB, 1 CPU, same network. Both images were local. Nothing was pulled. `pg/hostconfig.txt` and `proof/rust-hostconfig.txt` record `privileged=false`. At the oracle rerun, `proof/continue-capacity.txt` recorded one a376 builder and three running Rust builders. The other two were `oo-a375-proof` and `oo-fe01-rust-m6`. They were left alone.

Focused commands, all exit 0: `cargo test --locked --offline --lib --test contract` for each crate; `cargo test --locked --offline --test coexist -- --exact phase` for the phases below; `cargo clippy --locked --offline --all-targets -- -D warnings` for each crate. Clippy logs contain no `warning:` line. Calendar clippy finished in 1m 03s. Enum clippy finished in 1.28s.

| Phase | Exit | Result line |
|---|---|---|
| calendar lib + contract | 0 | 4 passed, then 2 passed |
| enum lib + contract | 0 | 6 passed, then 2 passed |
| calendar solo | 0 | 1 passed, 0.60s |
| enum solo reject | 0 | 1 passed, 0.10s |
| calendar solo replay | 0 | 1 passed, 0.10s |
| calendar then enum | 0 | 0.40s, then enum 0.43s |
| replays on that database | 0 | 0.14s, 0.14s |
| enum then calendar | 0 | 0.36s, then calendar 0.36s |
| replays on that database | 0 | 0.14s, 0.14s |
| hostile, each crate | 0 | 0.22s, 0.28s |
| drift, each crate | 0 | 0.10s, 0.10s |
| widened search_path replay | 0 | accepted, 0.14s |

Calendar `operate` puts `fy2026`, activates, and reads `oo_dict.calendar` as `oo_r_calendar`. A read before activate returns `42501`. Enum `cal_first` and `enum_first` call the same operate shape. Solo replay asserts that a late `oo_enum_admin` still lacks `EXECUTE` on `oo_calendar_control.dict_create_open()`.

## Catalog facts

`proof/facts-before.txt`, taken before any shared install: six roles, admins and runtimes `LOGIN`, view roles not `LOGIN`, and `rolsuper`, `rolbypassrls`, `rolcreatedb`, `rolcreaterole` all false. `runtime_membership|0`. `oo_a365_cal_first`, `oo_a365_enum_first`, and `oo_a365_hostile` had zero of `oo_calendar_control`, `oo_enum_control`, and `oo_dict`.

After both install orders, before the search_path `ALTER`: `oo_a365_cal_first` is owned by `oo_calendar_admin` and `oo_a365_enum_first` by `oo_enum_admin`. Both runtimes have `USAGE` and not `CREATE` on `oo_dict`. Both view roles lack `CREATE`. `oo_r_calendar`, `oo_r_enum`, and `oo_r_other` lack `EXECUTE` on all six signatures. The sibling admin has `EXECUTE` on the owner's `dict_create_open()` and `dict_grant_runtime(text)`. Each allowlist holds exactly its own runtime role. All six functions match the pinned body, owner, `SECURITY DEFINER`, language, `{search_path=pg_catalog}`, `PUBLIC` execute count 0, and identity arguments `runtime_role text`.

A non-owner `GRANT USAGE ON SCHEMA oo_dict TO oo_r_other`, rolled back, left `has_schema_privilege` false (`noop|...|f` on both databases). The driver stderr recorded `WARNING: no privileges were granted for "oo_dict"` twice (`proof/nonowner-grant-warning.txt`). Allowlisted runtime `USAGE` is the `usage|...|t|f` rows, which is a different grant from that quiet success.

The minimal runtime cannot open the sibling functions, call `dict_grant_runtime('oo_r_other')`, or `CREATE TABLE oo_dict.a376_no`. Each call returned `42501`. Open and grant failed with `permission denied for schema` on the control schema, so the body did not run. Create failed with `permission denied for schema oo_dict`. Both `a376_no` counts are 0. `proof/facts-deny.txt`.

Hostile private-schema count is 0. Drift left `security_invoker=true` on `oo_dict.enums` in `oo_a365_cal_first` and on `oo_dict.calendar` in `oo_a365_enum_first`. `drift_schemas` is 2 on both of those databases because the successful installs were already there. The drift tests expect `InstallDrift { check: "dict_objects" }` and an unchanged snapshot count.

## Raw psql, still unsupported

`linux/continue-proof.sh` applied the pinned files with `docker exec -i` on databases `oo_a376_raw_tx` and `oo_a376_raw_auto`. Those names are outside the frozen `oo_a365_*` fixtures.

Single transaction, calendar file as `oo_calendar_admin`: exit 0. The view `oo_dict.calendar` is owned by `oo_calendar_view`. Enum file as `oo_enum_admin`: exit 3, `ERROR: 42P06: schema "oo_dict" already exists` (`proof/raw-tx-enum-retry.log`). After that error the enum schema count is 0, the calendar schema count is 1, and `oo_dict` count is 1. The second transaction rolled back.

Autocommit, same order. `SET LOCAL ROLE` is a warning, not an abort: `WARNING: 25P01: SET LOCAL can only be used in transaction blocks` (`proof/raw-auto-calendar-retry.log`). The calendar file still exits 0. `oo_dict.calendar` exists and is owned by `oo_calendar_admin`. The enum file then commits `oo_enum_control` tables through the statement before `CREATE SCHEMA oo_dict`, and stops with the same `42P06` (exit 3). `oo_enum_control` count is 1. The `enums` view was not created. The dual view count is 1. That is a partial private catalog beside one view, and it is the unsupported outcome. It is not evidence that both migrations compose under raw `psql`.

## Oracle

`linux/oracle/main.rs` is std-only. It checks the empty and `abc` SHA-256 vectors, the two migration digests, the contract sentences, and `proof/facts.txt`. `proof/proof.exit` is 0. `proof/oracle.log.exit` is 0. The first compile failed (`proof/oracle-compile-attempt.log`) and is recorded in `FAILED-ATTEMPTS.md`. The product crates were not the compiler input.
