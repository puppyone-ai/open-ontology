# dict search_path correction

Frozen before any product edit. Session `eef831aa-dba4-4af2-a78b-541bc9e0a378`. Base `be4b596481d83435df2ede8abd9ebb2a8647c706`. Pre-edit smoke is already exit 0: Linux arm64 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, both child locks `package_names=211 source_lines=208`, and `git diff be4b596481d83435df2ede8abd9ebb2a8647c706 HEAD` on `tools/calendar-store` and `tools/enum-store` is empty.

a376 `APPROVE_LIMITED` does not waive the exact `search_path` requirement. Both `CONTRACT.md` files already say the adoption functions are `search_path = pg_catalog` and that replay accepts them only when owner, body, definer bit, and `search_path` match. A wrong shape is `install_drift` or `collision`. Nothing is dropped and nothing is rewritten.

## Defect

`function_state` and `runtime_grant_state` in both `src/adopt.rs` files use `config.contains("search_path=pg_catalog")` on `proconfig::text`. a376 altered `oo_calendar_control.dict_grant_runtime(text)` to `search_path = pg_catalog, public`. The text cast was `{"search_path=pg_catalog, public"}`. Calendar replay returned replay. That substring also matches a `pg_catalog` prefix and an extra GUC whose text cast still contains `search_path=pg_catalog`.

## Rule

Read `pg_proc.proconfig` as `Option<Vec<String>>`. Do not compare the text cast.

Exact means all of the following:

- The value is `Some` with length 1. `None`, an empty array, or a second element is not exact.
- The single element splits on the first `=` into a name and a value. The name is `search_path`, with no space around `=`.
- The value splits on `,` into schema items. Each item is trimmed. One pair of double quotes around an entire item is removed. That is PostgreSQL's `GUC_LIST_QUOTE` form for one identifier. A quote that does not wrap the whole item, an empty item, or a doubled quote inside the item is not the pinned name.
- The resulting schema list is exactly `pg_catalog`.

Not exact, including the open, close, and `dict_grant_runtime(text)` variants, on this schema and on the sibling schema:

- `search_path=pg_catalog, public`
- `search_path=public, pg_catalog`
- `search_path=pg_catalog, pg_temp` and `search_path=pg_temp, pg_catalog`
- a prefix or longer token such as `search_path=pg_catalog_extra` or `search_path=pg_catalogfoo`
- any second GUC, including `work_mem`, beside an otherwise pinned `search_path`
- the text-cast wrappers `{search_path=pg_catalog}` and `{"search_path=pg_catalog, public"}` stored as one element

a376's server text casts decode to the array elements `search_path=pg_catalog` and `search_path=pg_catalog, public`. The pinned element is only the first.

## Routing, unchanged

- This product's private schema is already present and a checked function is not exact: `InstallDrift { check: "adopt_function" }`. The transaction rolls back.
- This product's private schema is absent, `oo_dict` is present, and a sibling function is not exact: `Collision { check: "oo_dict" }`. This product's schema is not created. The transaction rolls back.
- Solo replay with both adoption functions absent stays accepted. That is an install from the accepted SQL before those functions existed.
- Solo or shared replay with exact functions stays `Replay`.
- Restoring `SET search_path = pg_catalog` on the altered functions, with no extra GUC, makes the next replay succeed.
- A refusal does not grant, does not rewrite function config, does not insert a snapshot, and does not create the caller's private schema when that schema was absent.

## Original cases preserved

- Migration bytes stay `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` and `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`.
- Both migration files still contain `CREATE SCHEMA oo_dict`. Raw `psql` of both files on one database stays unsupported.
- No `IF NOT EXISTS`, no `CREATE OR REPLACE`, and no `DROP` is added.
- Child locks stay `881275280`, calendar `881275289`, enum `881275293`.
- Both `Cargo.lock` files stay registry subsets. No `cargo update`, no `cargo generate-lockfile`, no root manifest edit.
- Function bodies, owners, `SECURITY DEFINER`, languages, `PUBLIC` execute revoke, and sibling `EXECUTE` grants stay as the repair wrote them.
- Wrong owner, wrong definer bit, wrong body, wrong language, or `PUBLIC` execute other than zero stays not exact.
- Hostile sibling role stays `Collision { check: "sibling_role" }`.
- Hash, project, and allowlist disagreement stay on their existing checks.
- Put, activate, and read signatures stay `calendar-store-v1` and `enum-store-v1`.
- Both role pairs may still exist before either install.
- Adoption still does not run `ALTER SCHEMA` and does not take ownership of `oo_dict`.
- The two-order install matrix, the 137 catalog facts, and the runtime attack matrix are not repeated. a376 already proved them on this repair. This correction proves the search_path predicate those runs did not enforce.

## What may change

Only the proconfig comparison in both `function_state` and `runtime_grant_state` functions, the two catalog reads that return `p.proconfig` instead of `proconfig::text`, and the tests that pin this rule. Executed migration text and adoption function bodies do not change.

## Still open

SP14, E51, SP14-W04, SEM-D03, SEM-D04, and `enum_calendar_same_database`. No new count unit. No public authentication. No production call.
