# Source inventory

Product and the repository root were read only. Nothing under `tools/`, `crates/`, `examples/`, or the root manifests was written by this session.

## Repair identity

| name | value |
|---|---|
| repair | `be4b596481d83435df2ede8abd9ebb2a8647c706` |
| proof archive | `daff88a3253bf2c5171bece48ade8309b17e7417` |
| report HEAD | `0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8` |
| oracle HEAD | `55be6e88e191e89748c8e9e9905f074bda87e810` |
| store diff `be4b596..HEAD` | empty (`proof/reviewed-commits.txt`) |
| calendar migration | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| enum migration | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |
| child locks | `package_names=211 source_lines=208` each |

`CREATE SCHEMA oo_dict` is calendar migration line 222 and enum migration line 252. `SET LOCAL ROLE` is calendar line 226 and enum line 256.

## Product files read for the verdict

- `tools/calendar-store/CONTRACT.md` and `tools/enum-store/CONTRACT.md`, shared `oo_dict` sections, including the sentence `Both role pairs may exist before either install.` and advisory lock `881275280`.
- `tools/calendar-store/src/adopt.rs` and `tools/enum-store/src/adopt.rs`, `function_state` and `runtime_grant_state`, substring `search_path=pg_catalog`.
- `tools/calendar-store/src/sql.rs` and `tools/enum-store/src/sql.rs`, `SECURITY DEFINER` and `SET search_path = pg_catalog` on the adoption functions.
- `tools/calendar-store/tests/coexist.rs` and `tools/enum-store/tests/coexist.rs`, phase names, put, activate, read, hostile setups, and `InstallDrift { check: "dict_objects" }`.
- Both `migrations/oo_*_control_v1.sql` files, around the schema and view statements.
- `specs/open-ontology-v0.1-plan.md` was in scope for the open SP14 and E51 gates. This packet does not close them.
- a365 reports under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a365/SP14/control-migration-coexistence/` were read as the author packet. Their proof was not rerun.

## Owned drivers

Recipes and their origin commit, path, and hash are in `origin/recipes.txt`.

- `linux/run-smoke.sh`, `linux/smoke-inside.sh` — pre-edit Rust 1.94 smoke. Exits 0.
- `linux/run-proof.sh` — green phases. Aborted at raw `psql` because stdin was dropped. Not a product failure.
- `linux/continue-proof.sh` — deny, raw `psql`, clippy, oracle, on the containers `run-proof.sh` had already started.
- `linux/oracle/main.rs` — std-only catalog oracle. First compile failed. The fixed file produced `oracle_ok`.
- `linux/hash-oracle/main.rs` — tree walker, bytes copied from the a365 walker `e85c22eaa948586e8739d0976eea536d442408d950076a143bac15447ce404df`.
- `linux/hash-tree.sh`, `linux/cleanup.sh`, `pg/pg-wrap.sh`.
- `proof/facts.txt` is the concatenation of `facts-before`, `facts-green`, `facts-deny`, `facts-pathdrift`, `facts-refuse`, and `facts-raw`.

## Images

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

No pull. No host network. No published port. No privilege. Passwords stayed in the mode-700 tmp directory until `linux/cleanup.sh`. They are not copied into this tree.
