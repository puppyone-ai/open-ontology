# Handoff

Owner of this directory: session `eef831aa-dba4-4af2-a78b-541bc9e0a382`. Product files are read-only here. Parent SP14 and E51 stay open. This directory does not accept them and does not change a ledger count.

## Result to carry

`APPROVE_LIMITED`. At `9a7940ba5bae734b16928cd68913b0037c262c0f`, both producers require `proconfig` to be the single setting `search_path=pg_catalog`. A widened `public`, `pg_temp`, `pg_catalog_extra`, or extra GUC returns `InstallDrift { check: "adopt_function" }` when that product's schema exists, and enum adoption returns `Collision { check: "oo_dict" }` when `oo_enum_control` does not. Neither committed path grants, creates the missing private schema, or writes a snapshot. Restoring the exact setting makes replay succeed. Migration bytes stay `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` and `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd`. Locks stay `881275280`, calendar `881275289`, enum `881275293`.

`proof/proof.exit` is 0. The unit pass is `proof/calendar-unit-exact.log` and `proof/enum-unit-exact.log`. Raw `psql` of both migration files on one database is still unsupported.

## Do not rerun

`linux/run-proof.sh` creates `oo-a382-pg` and `oo-a382-proof` and repeats this database. `linux/run-smoke.sh` refuses once `inside.exit` exists. The a376 two-order matrix and the a378 repair driver are not this session's drivers.

## Cleanup

`linux/cleanup.sh` removes a container, volume, or network only when `oo.owner=a382` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a382`. Named objects are `oo-a382-pg`, `oo-a382-proof`, `oo-a382-hash`, volumes `oo-a382-eef831aa-cargo`, `oo-a382-eef831aa-target`, `oo-a382-eef831aa-pgdata`, and network `oo-a382-eef831aa-net`. It also removes `/tmp/oo-a382-eef831aa-dba4-4af2-a78b-541bc9e0a382`. It does not prune and it does not remove other owners.

## Hash

After `cleanup-done.txt` is `done`, `linux/hash-tree.sh` runs twice inside the pinned Rust image, network none, one builder, 6 GiB, 1 CPU, `--pull never`. The walker is `linux/hash-oracle/main.rs`, content SHA-256 `e85c22eaa948586e8739d0976eea536d442408d950076a143bac15447ce404df`. It checks empty and `abc`, skips `SHA256SUMS` and `SHA256SUMS.sha256`, and writes the sidecar. Pass 2 includes the finished pass 1 logs. Pass 2's own console is copied in after that walk.

## Left open

SP14, E51, SP14-W04, SEM-D03, SEM-D04, and `enum_calendar_same_database`. No production call, no customer call, no foreign database, no push, no release.
