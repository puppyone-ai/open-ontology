# Findings

Source under repair is the calendar-store and enum-store installers. Pre-commit HEAD is `6283bc16f6d408ffa38dbbf8837cc4f80268bbcb`. The live proof was collected at `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`. The a361 review at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/` recorded the gap and did not patch it. Live numbers below are from the a365 cluster.

## Same-database `CREATE SCHEMA` is still in the SQL files

`tools/calendar-store/migrations/oo_calendar_control_v1.sql` creates schema `oo_dict` at line 222. `tools/enum-store/migrations/oo_enum_control_v1.sql` creates it at line 252. Separate database fixtures never ran those two statements on one database. a361 did, on `oo_a361_schema_probe`, and enum stopped with `schema "oo_dict" already exists`.

The repair leaves both files byte-identical. `migration_sha256()` compares the computed digest with `ACCEPTED_MIGRATION_SHA256` and returns `InstallDrift { check: "migration_sha256" }` on a mismatch. Blind `IF NOT EXISTS` is not in either migration. `adopt.rs` refuses an adoption script that contains `CREATE SCHEMA oo_dict` or `if not exists`.

## A non-owner `GRANT USAGE` grants nothing

On PostgreSQL 17.10, `GRANT USAGE ON SCHEMA oo_dict` by an admin who does not own `oo_dict` warns `no privileges were granted for "oo_dict"` and returns success. `tokio-postgres` treats that batch as success. `proof/cal-first-attempt-2/` is that failure: enum install, put, and activate committed, `oo_r_enum` held `SELECT` on `oo_dict.enums`, and `read` returned `42501`.

The owner therefore defines `dict_grant_runtime(runtime_role text)` as `SECURITY DEFINER`, `search_path = pg_catalog`, language `plpgsql`. The body checks `^[a-z_][a-z0-9_]{0,62}$` and requires the name in the sibling `runtime_allow`, then `EXECUTE format('GRANT USAGE ON SCHEMA oo_dict TO %I', runtime_role)`. The parameter name is `runtime_role`. Naming it `role_name` made `WHERE role_name = role_name` ambiguous with `runtime_allow.role_name` (`42702`, `proof/cal-first-attempt-4/`). Identity arguments stored by this server are `runtime_role text`. A lookup for `text` or `role_name text` misses the function (`proof/cal-first-attempt-3/`).

`PUBLIC` has no execute. Execute is granted to the sibling admin. Activate calls the function only when `oo_dict`'s owner is the other admin, then checks `has_schema_privilege`. The schema owner still grants directly.

## Adoption checks the sibling before any DDL

The second installer reads the sibling catalog first. Adoptable means the sibling admin owns `oo_dict`, the sibling manifest digest is the pinned digest, the only extra relation is the sibling view with the pinned columns and options, `PUBLIC` has no privilege, the sibling open and close bodies match, and `dict_grant_runtime` is exact and executable by the adopter. Anything else is `Collision { check: "oo_dict" }` and the caller's private schema is not created.

A `42501` on the sibling manifest select is that collision. It is not `Query`. Catalog probes that can fail do not continue in an aborted transaction: the allowlist read checks that the schema exists and that the caller has `USAGE` and `SELECT` first (`25P02` in `proof/solo-replay-attempt-2/` came from skipping that order).

View text is `pg_get_viewdef(c.oid, true)`. A `::regclass` cast returns `42501` when the caller lacks `USAGE` on `oo_dict`.

The owner's `nspacl` entry is how PostgreSQL 17 records ownership. Grant checks exclude `r.oid <> n.nspowner`. Allowlisted runtime `USAGE` is closed-dict usage, not a foreign grant. An unreadable allowlist counts as empty.

## Replay does not repair a drifted catalog

Replay requires this product's view. Zero or one extra `oo_dict` object is allowed, and that object must be the sibling view. Dict owner is this admin, or the sibling admin when the sibling view and the sibling catalog verify. Exact open, close, and runtime-grant functions are required once the sibling view is present. Historical solo catalogs may omit the functions. A wrong body is `install_drift`. Grants are not rewritten to hide that.

## What the green phases exercised

`oo_a365_cal_first`: calendar owns `oo_dict`, enum adopts, both replay. `oo_a365_enum_first`: enum owns, calendar adopts, both replay. Each side puts and activates a runtime role and reads its view. `oo_a365_solo`: calendar alone, then enum install returns collision, then calendar replay still returns replay. Hostile phases cover a foreign owner, an empty sibling-owned schema, and a table named like the sibling view. Drift phases set `security_invoker=true` on the sibling view and expect install drift. Role pairs are not altered into superusers.

## Contract surface that stays

`CONTRACT` remains `calendar-store-v1` and `enum-store-v1`. Public install, put, activate, and read stay. No path dependency was added between the crates, so neither lock changed. Coexistence is sequenced `cargo test` against one database.

The shared-schema section in both `CONTRACT.md` files was written before the product edits and updated when `dict_grant_runtime` was added. It pins both digests, lock `881275280`, the adoption gate, historical function absence, and the rule that a later sibling role does not receive a back-filled `EXECUTE` grant.
