# dict search_path repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a378`. Base `be4b596481d83435df2ede8abd9ebb2a8647c706`. This packet repairs the substring `search_path` check. It does not accept SP14, E51, SP14-W04, SEM-D03, SEM-D04, or `enum_calendar_same_database`. It adds no ledger count. An independent review of the change is still required.

`CORRECTION-CONTRACT.md` was written after the pre-edit smoke and before the product edits.

## What changed

`function_state` and `runtime_grant_state` in `tools/calendar-store/src/adopt.rs` and `tools/enum-store/src/adopt.rs` no longer use `contains("search_path=pg_catalog")`. Both read `pg_proc.proconfig` as `Option<Vec<String>>`. Exact means one element whose name is `search_path` and whose schema list is only `pg_catalog`. A quoted identifier `"pg_catalog"` is the same schema. `public`, `pg_temp`, a prefix such as `pg_catalog_extra`, and any second GUC are not exact.

The catalog reads in both `src/sql.rs` files return `p.proconfig` instead of `proconfig::text`. Migration files, adoption function bodies, and child lock ids are unchanged.

## Proof

Pre-edit smoke, before the edits: Linux arm64 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `linux/pre-edit-smoke/inside.exit` 0, `product-drift.exit` 0. Both locks were `package_names=211 source_lines=208`. `cargo update` was not invoked.

One private PostgreSQL 17.10, `pg/version.txt` = `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. 1 GiB, 1 CPU, network `oo-a378-eef831aa-net`, alias `oo-a378-pg`, `pg/ports.json` is `{}`. Builder image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, 6 GiB, 1 CPU. Both `privileged=false`. `proof/capacity.txt` recorded own 0, global 0, host cargo 0 before launch. Nothing was pulled.

Both role pairs existed before the calendar install. Admins and runtimes are `LOGIN`. View roles are not. `rolsuper`, `rolbypassrls`, `rolcreatedb`, and `rolcreaterole` are false (`proof/roles.txt`).

Database `oo_a378_path`, one shared install:

| Phase | Exit | Result |
|---|---|---|
| calendar lib + contract | 0 | 5 passed, then 2 passed |
| enum lib + contract | 0 | 7 passed, then 2 passed |
| calendar baseline install + replay | 0 | `proconfig=search_path=pg_catalog`, 0.20s |
| calendar open/close/grant widened, plus extra `work_mem` | 0 | each `install_drift adopt_function mutated=false`, then replay, 0.47s |
| enum adopt while calendar functions are widened | 0 | `collision oo_dict mutated=false enum_schema=absent`, 0.12s |
| calendar restore exact path | 0 | replay, 0.13s |
| enum shared install + replay | 0 | `proconfig=search_path=pg_catalog`, 0.20s |
| enum open/close/grant widened, plus extra `work_mem` | 0 | each `install_drift adopt_function mutated=false`, then replay, 0.53s |
| calendar replay while enum functions are widened | 0 | `install_drift adopt_function mutated=false`, 0.13s |
| enum restore, then both replays | 0 | replay, 0.13s and 0.13s |
| calendar clippy `--all-targets -D warnings` | 0 | no `warning:` line, 1m 10s |
| enum clippy `--all-targets -D warnings` | 0 | no `warning:` line, 1.90s |

`proof/proof.exit` is 0. The two-order coexist matrix, the 137 catalog facts, and the runtime attack matrix were not run.

Server arrays read as `Option<Vec<String>>`, not guessed from a text cast:

- pinned install: `Some(["search_path=pg_catalog"])`
- `SET search_path = pg_catalog, public`: `Some(["search_path=pg_catalog, public"])`
- `SET search_path = pg_catalog_extra`: `Some(["search_path=pg_catalog_extra"])`
- `SET search_path = pg_catalog, pg_temp`: `Some(["search_path=pg_catalog, pg_temp"])`
- `SET search_path = pg_temp, pg_catalog`: `Some(["search_path=pg_temp, pg_catalog"])`
- `SET work_mem = '4MB'` beside the pin: `["search_path=pg_catalog", "work_mem=4MB"]`

Each of those returned `InstallDrift { check: "adopt_function" }` when the caller's schema already existed, or `Collision { check: "oo_dict" }` when enum install ran before `oo_enum_control` existed. The fingerprint of schema ACLs, relation ACLs, function ACLs, and row counts was equal before and after each refused call. Calendar snapshot count stayed 0. Enum schema was absent on the collision. Restoring `search_path = pg_catalog` made the next replay succeed.

## Unchanged bytes

| file | SHA-256 |
|---|---|
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

`proof/fetch.log` ends with `cargo-update-invoked=no`. No root manifest edit, no `cargo update`, no `cargo generate-lockfile`.

## Raw SQL

Raw `psql` of both migration files on one database is still unsupported. a376 already recorded `42P06` on the second `CREATE SCHEMA oo_dict`, and the autocommit `25P01` warning that leaves the calendar view owned by `oo_calendar_admin`. This repair does not change those files and does not claim a general fix.

## Logger note

`proof/source-sha256.txt` was passed through the 48-hex password redactor, which also cut SHA-256 digests. The command exit is 0. `proof/source-sha256-full.txt` is the same `sha256sum` list, rerun in the same builder against the same `/src` mount, without that filter. See `FAILED-ATTEMPTS.md`.
