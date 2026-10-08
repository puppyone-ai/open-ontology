# Findings

Fixed `9a7940ba5bae734b16928cd68913b0037c262c0f`. Base of the repair `be4b596481d83435df2ede8abd9ebb2a8647c706`.

## APPROVE_LIMITED

The substring check is gone. Both `adopt.rs` files compare `Option<Vec<String>>`. The predicate `proconfig_is_pinned_pg_catalog` is the same 889 bytes in both files. `function_state` and `runtime_grant_state` both call it. `src/sql.rs` in each crate selects `p.proconfig`, and the text cast is gone. No `contains("search_path=pg_catalog")` remains under either `src` tree.

Exact means `Some` with one element. The element splits on the first `=` into name `search_path` and a value. The value has one comma-separated item. That item is trimmed, and one pair of wrapping quotes is removed when the inside is non-empty and contains no quote. The result must be `pg_catalog`. `None`, an empty array, a second element, a second schema, a longer token, and the text-cast wrappers fail.

a376 recorded calendar replay accepting `{"search_path=pg_catalog, public"}`. On this database the same setting came back as `Some(["search_path=pg_catalog, public"])`. Calendar `install()` returned `InstallDrift { check: "adopt_function" }`. The fingerprint of schema ACLs, relation ACLs, function ACLs, and row counts was equal before and after that call.

## Where the refusal sits

`install` opens one transaction. `ensure_roles` only reads role attributes. The dict lock and the product lock are `pg_advisory_xact_lock`. An error calls `rollback` and does not reach `finish_install`, which is the commit.

When this product's schema is absent and `oo_dict` is present, `require_adoptable` checks the sibling open function, close function, and `runtime_grant_state` before `batch_execute` of the adoption script. Enum `install()` in that state, with calendar `dict_create_open` at `pg_catalog, public`, `dict_create_close` at `pg_temp, pg_catalog`, and `dict_grant_runtime(text)` at `pg_catalog, pg_temp`, returned `Collision { check: "oo_dict" }`. `oo_enum_control` was absent. The calendar snapshot count stayed 0. The fingerprint matched.

When this product's schema is already present, replay calls `verify_shared` before commit. That checks `function_state` and `runtime_grant_state` for this schema and, when the sibling view is present, for the sibling schema. A wrong config returns `InstallDrift { check: "adopt_function" }`.

Calendar producer, own schema present, each change restored before the next:

- `dict_create_open()` set to `pg_catalog, public`
- `dict_create_close()` set to `pg_catalog_extra`
- `dict_grant_runtime(text)` set to `pg_catalog, pg_temp`
- `dict_create_open()` given `work_mem=4MB` beside the pinned path

Each was `InstallDrift { check: "adopt_function" }` with `mutated=false`, then replay after `RESET ALL` and `SET search_path = pg_catalog`.

Enum producer, after the shared install, the same three widened settings, and `work_mem=4MB` on `dict_grant_runtime(text)`, did the same.

Leaving those three enum functions widened and calling calendar `install()` returned `InstallDrift { check: "adopt_function" }`. Calendar's own `proconfig` was still the pin. Restoring the enum functions made both replays succeed.

The unit test accepts the server element `search_path=pg_catalog` and `search_path="pg_catalog"`. It rejects the text-cast wrappers, `public`, `pg_temp` on either side, `pg_catalog_extra`, `pg_catalogfoo`, a second `pg_catalog`, and a second GUC. This server returned the unquoted element.

## What this run did not repeat

One database, calendar then enum. The calendar-absent `require_adoptable` path was read: it has the same shape and the same predicate, and it returns collision before the adoption script. It was not executed. a376 already ran both install orders and the 137 catalog facts on the old matcher. Those runs stay the evidence for coexistence, and they are the runs that accepted the widened path. This review does not re-sign them as an exact-pin proof.

Migration digests and lock constants match the base. Put, activate, and read were not modified and were not re-run. Lib tests other than `pinned_search_path_is_exact_pg_catalog_only` were not run.

## Still open

Raw `psql` of both migration files on one database stays unsupported. Both files still contain `CREATE SCHEMA oo_dict`. SP14, E51, SP14-W04, SEM-D03, SEM-D04, and `enum_calendar_same_database` stay open. This directory does not grant public authentication and does not sign the parent.
