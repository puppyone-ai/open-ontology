# Findings

Base `be4b596481d83435df2ede8abd9ebb2a8647c706`. a376 `APPROVE_LIMITED` recorded that `config.contains("search_path=pg_catalog")` accepts `{search_path=pg_catalog, public}`. That approval does not waive the exact pin in both `CONTRACT.md` files.

## The substring check is gone

Both `adopt.rs` files compare `Option<Vec<String>>`. On PostgreSQL 17.10 the pinned functions come back as one element, `search_path=pg_catalog`. The widened forms below are refused.

Calendar producer, own schema already present, each change restored before the next:

- `dict_create_open()` set to `pg_catalog, public` → `InstallDrift { check: "adopt_function" }`
- `dict_create_close()` set to `pg_catalog_extra` → same
- `dict_grant_runtime(text)` set to `pg_catalog, pg_temp` → same
- `dict_create_open()` given an extra `work_mem=4MB` beside the pinned path → same

Enum producer, after the shared install, the same three widened settings and an extra `work_mem` on `dict_grant_runtime(text)` → the same drift.

Leaving all three calendar functions widened (`public`, `pg_temp, pg_catalog`, and `pg_catalog, pg_temp`) and calling enum `install()` → `Collision { check: "oo_dict" }`. `oo_enum_control` stayed absent. Calendar snapshot count stayed 0.

Leaving all three enum functions widened and calling calendar `install()` → `InstallDrift { check: "adopt_function" }`. Calendar's own `proconfig` was still `search_path=pg_catalog`.

Each refused call compared a catalog fingerprint before and after. The fingerprints matched. Restoring `SET search_path = pg_catalog` (`RESET ALL`, then that one setting) made the next call `Replay`.

The unit test `pinned_search_path_is_exact_pg_catalog_only` passed in both crates. It rejects the text-cast wrappers, `public`, `pg_temp` on either side, `pg_catalog_extra`, `pg_catalogfoo`, a second copy of `pg_catalog`, and a second GUC. It accepts the server element `search_path=pg_catalog` and the one-identifier quoted form `search_path="pg_catalog"`. The live server returned the unquoted element.

## Original cases that stayed

Migration digests are the two values in `REPORT.md`. `git diff be4b596481d83435df2ede8abd9ebb2a8647c706 --` the migration files and both `Cargo.lock` files is empty. Lock shape stayed 211 names and 208 source lines. Advisory locks were not edited. Lib tests other than the new predicate still passed, including the adoption-script test that rebuilds the migration bytes. Contract tests passed. Put, activate, and read were not modified and were not re-run. The a376 two-order, 137-fact, and runtime proofs were not repeated.

## Still unsupported or open

Raw `psql` of both files on one database is still unsupported. SP14, E51, SP14-W04, SEM-D03, SEM-D04, and `enum_calendar_same_database` stay open. This directory does not grant public authentication and does not sign the parent.
