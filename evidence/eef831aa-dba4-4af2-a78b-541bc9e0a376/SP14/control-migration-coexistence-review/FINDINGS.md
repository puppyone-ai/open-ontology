# Findings

Repair `be4b596481d83435df2ede8abd9ebb2a8647c706`. Live proof archive `daff88a3253bf2c5171bece48ade8309b17e7417`. Report HEAD `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8`. The oracle ran at `55be6e88e191e89748c8e9e9905f074bda87e810`. The calendar-store and enum-store trees do not differ across that range.

## APPROVE_LIMITED

The Rust `install()` path composes. Calendar then enum, and enum then calendar, each install, replay, put, activate, and read on one private PostgreSQL 17.10. Role pairs were `NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE` before either shared install. View roles are `NOLOGIN`. Contracts state that both role pairs may exist before either install.

The approval is limited by the search_path matcher and by the raw SQL files. Those limits are the rest of this file.

## search_path is a substring

`function_state` and `runtime_grant_state` in both `src/adopt.rs` files require `config.contains("search_path=pg_catalog")`. A config of `{search_path=pg_catalog, public}` contains that substring. Calendar replay on `oo_a365_cal_first` accepted it (`proof/facts-pathdrift.txt`, exit 0, 0.14s).

The functions `install()` created, read back before that `ALTER`, have `proconfig` exactly `{search_path=pg_catalog}`, language `sql` or `plpgsql` as pinned, `PUBLIC` execute count 0, and the pinned `prosrc`. `dict_grant_runtime` identity arguments are `runtime_role text`. Owners are the creating admins. That part of the repair holds. Replay will also accept a widened `search_path` that still mentions `pg_catalog`.

## Raw dual psql is unsupported

Both migration files still execute `CREATE SCHEMA oo_dict`. Digests:

- calendar `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520`
- enum `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`

`--single-transaction` of the calendar file, then the enum file, on `oo_a376_raw_tx`: the second file exits 3 with `42P06`. `oo_enum_control` is absent. `oo_calendar_control` and `oo_dict` remain. The calendar view owner is `oo_calendar_view`, because `SET LOCAL ROLE` ran inside the transaction.

Autocommit on `oo_a376_raw_auto`: `SET LOCAL` outside a transaction block is `WARNING 25P01` and the calendar file exits 0. The calendar view owner is `oo_calendar_admin`. The enum file commits the `oo_enum_control` tables (`raw_auto|oo_enum_control|1`; names in `proof/raw-catalog.txt`) and stops at `CREATE SCHEMA oo_dict` with `42P06`. There is no `oo_dict.enums` view. Dual view count is 1. That partial private catalog is the unsupported raw path. It is not a repair failure of `install()`, and it is not a general fix.

## Actual USAGE

On PostgreSQL 17.10 a non-owner `GRANT USAGE ON SCHEMA oo_dict` warns `no privileges were granted for "oo_dict"` and leaves `has_schema_privilege` false. Both orders recorded `noop|...|f` after a rolled-back grant to `oo_r_other`. The activated runtimes recorded `usage|<db>|<role>|t|f`: `USAGE` true and `CREATE` false. Those are the grants `install()` / activate made, checked with `has_schema_privilege`, not the quiet non-owner `GRANT`.

## Minimal runtime

`oo_r_enum` on `oo_a365_cal_first` and `oo_r_calendar` on `oo_a365_enum_first` each received `42501` for `dict_create_open()`, `dict_grant_runtime('oo_r_other')`, and `CREATE TABLE oo_dict.a376_no`. The messages are `permission denied for schema oo_calendar_control`, `permission denied for schema oo_enum_control`, and `permission denied for schema oo_dict`. The function body did not run. Catalog `has_function_privilege` for both runtimes and for `oo_r_other` is false on all six signatures. The sibling admin's `EXECUTE` on the owner's open and `dict_grant_runtime` is true. No `a376_no` relation remained.

## Refusals

Hostile phases (`foreign owner`, empty sibling-owned schema, table named `enums`) exit 0. `hostile_private|0`: neither `oo_calendar_control` nor `oo_enum_control` exists on `oo_a365_hostile` after the refusals. The calendar test expects `Collision { check: "oo_dict" }` and asserts the caller schema is absent.

Drift phases set `security_invoker=true` on the sibling view and exit 0 with `InstallDrift { check: "dict_objects" }`. `drift_schemas|2` counts the private schemas that the earlier successful installs created. The drift test compares snapshot counts before and after the refused install.

## What stayed closed

Parent SP14 and E51 stay open. This directory does not close SP14-W04, SEM-D03, SEM-D04, or the parent name `enum_calendar_same_database`. No production call, no customer call, no push, no release.
