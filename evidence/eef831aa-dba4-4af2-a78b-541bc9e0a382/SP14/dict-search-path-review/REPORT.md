# dict search_path review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a382`. Fixed source `9a7940ba5bae734b16928cd68913b0037c262c0f`. Product trees were mounted from `git archive` of that commit. This session did not edit `tools/calendar-store` or `tools/enum-store`.

## Verdict

`APPROVE_LIMITED` for the exact `search_path` pin in both producers.

`function_state` and `runtime_grant_state` in both `src/adopt.rs` files read `pg_proc.proconfig` as `Option<Vec<String>>` and accept only one element, `search_path=pg_catalog`. A widened `public` path, `pg_temp` on either side, `pg_catalog_extra`, and a second GUC are refused. On the replay path the refusal is `InstallDrift { check: "adopt_function" }` and the catalog fingerprint is unchanged. On enum adoption, while the calendar functions are widened and `oo_enum_control` is absent, the refusal is `Collision { check: "oo_dict" }` and `oo_enum_control` stays absent. Restoring `search_path = pg_catalog` makes the next `install()` a replay.

The limit is the raw SQL files and the open parents. Migration bytes are unchanged, so raw `psql` of both files on one database stays the unsupported path a376 recorded. This directory does not accept SP14, E51, SP14-W04, SEM-D03, SEM-D04, or `enum_calendar_same_database`. It adds no ledger count. The a376 two-order matrix and 137 catalog facts were not run again.

## What was read

a376 `FINDINGS.md` and `proof/facts-pathdrift.txt`: `pathdrift|config|{"search_path=pg_catalog, public"}` and `pathdrift|result|accepted`. a378 `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `CORRECTION-CONTRACT.md`. Both `CONTRACT.md` files require `search_path = pg_catalog` and `install_drift` or `collision` for a wrong shape. `specs/open-ontology-v0.1-plan.md` has no `search_path` rule for these functions. There is no `AGENTS.md` in the repository root.

## Proof

Pre-edit smoke, before the proof script: Linux arm64 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `linux/pre-edit-smoke/inside.exit` 0, `product-drift.exit` 0 against the fixed commit. Both locks were `package_names=211 source_lines=208`. `cargo update` was not invoked.

One private PostgreSQL 17.10, `pg/version.txt` = `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. 1 GiB, 1 CPU, network `oo-a382-eef831aa-net`, alias `oo-a382-pg`, database `oo_a382_path`, `pg/ports.json` is `{}`. Builder image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, 6 GiB, 1 CPU. Both `privileged=false`. `proof/capacity.txt` recorded own 0, global 0, host cargo 0, pg own 0 before launch. Nothing was pulled. The proof command finished in 211.89 seconds, `proof/proof.exit` 0.

Both role pairs existed before the calendar install. Admins and runtimes are `LOGIN`. View roles are not. `rolsuper`, `rolbypassrls`, `rolcreatedb`, and `rolcreaterole` are false (`proof/roles.txt`).

The unchanged product test reads fixture files from `/run/oo-a378/fixture` and selects its phase with `OO_A378_PHASE`. Those names are inside this session's container. The database, network, volumes, and containers are a382.

| Phase | Exit | Result |
|---|---|---|
| calendar unit `adopt::tests::pinned_search_path_is_exact_pg_catalog_only` | 0 | 1 passed, 0.00s |
| enum unit, same test | 0 | 1 passed, 0.00s |
| calendar baseline install + replay | 0 | `proconfig=search_path=pg_catalog`, 0.27s |
| calendar open/close/grant widened, plus extra `work_mem` | 0 | each `install_drift adopt_function mutated=false`, then replay, 0.44s |
| enum adopt while calendar functions are widened | 0 | `collision oo_dict mutated=false enum_schema=absent`, 0.11s |
| calendar restore exact path | 0 | replay, 0.18s |
| enum shared install + replay | 0 | `proconfig=search_path=pg_catalog`, 0.25s |
| enum open/close/grant widened, plus extra `work_mem` | 0 | each `install_drift adopt_function mutated=false`, then replay, 0.60s |
| calendar replay while enum functions are widened | 0 | `install_drift adopt_function mutated=false`, 0.12s |
| enum restore, then both replays | 0 | replay, 0.12s and 0.12s |
| calendar clippy `--all-targets -D warnings` | 0 | no `warning:` line, 1m 05s |
| enum clippy `--all-targets -D warnings` | 0 | no `warning:` line, 1.73s |

Server arrays from this database:

- pinned install: `Some(["search_path=pg_catalog"])`
- `SET search_path = pg_catalog, public`: `Some(["search_path=pg_catalog, public"])`
- `SET search_path = pg_catalog_extra`: `Some(["search_path=pg_catalog_extra"])`
- `SET search_path = pg_catalog, pg_temp`: `Some(["search_path=pg_catalog, pg_temp"])`
- `SET search_path = pg_temp, pg_catalog`: `Some(["search_path=pg_temp, pg_catalog"])`
- `SET work_mem = '4MB'` beside the pin: `["search_path=pg_catalog", "work_mem=4MB"]`

`proof/fetch.log` ends with `cargo-update-invoked=no`. No root manifest edit, no `cargo update`, no `cargo generate-lockfile`.

## Unchanged bytes

| file | SHA-256 |
|---|---|
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |

`proof/locks.txt` shows calendar `881275289`, enum `881275293`, and shared dict lock `881275280` at the fixed commit and at base `be4b596481d83435df2ede8abd9ebb2a8647c706`. `git diff` of the two migration files and both `Cargo.lock` files between those commits is empty.
