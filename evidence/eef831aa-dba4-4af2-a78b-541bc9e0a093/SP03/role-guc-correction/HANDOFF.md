# HANDOFF a093 role GUC identity correction

Status: local fix commit ready for a later independent review. No push and no pull request.

Fix commit: `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` (tree `c35b95664d53dcef1497bbd224ecbeec65586cea`). Parent: `35320e9f691cc3c879eb371446101b01839dcb8e`.

Baseline: `fc877c482a61729ea46e779a674a46f8f72c80c5` (tree `d40755c31abd82d932b56886aa048ab4d7aa5e4b`, parent `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`). Reviewed integration `2980e85af22c3ade05272c66c7d2835b52730322` remains an ancestor of the baseline.

## Ownership

This commit touches only:

- `crates/oo-connectors/src/session.rs`
- `crates/oo-connectors/src/sql.rs`
- `crates/oo-connectors/src/model.rs`
- `crates/oo-connectors/tests/live_pg.rs`
- `crates/oo-connectors/README.md`
- `contracts/source/pgsql/README.md`
- `docs/17-rust-implementation.md`
- `testing/product/pg-source/init.sql`
- `testing/product/pg-source/load.sh`
- `testing/product/pg-source/up.sh`
- `testing/product/pg-source/README.md`

Not touched: root `Cargo.toml`, root `Cargo.lock`, `crates/oo-connectors/Cargo.toml`, `vendor`, `oo-builder`, `oo-pipeline`, `xtask`, CI, and the SDK. a088 keeps the NFC root lock. a092 keeps the pipeline.

`Cargo.lock` sha256 on the baseline archive remains `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057`. Connector manifest sha256 remains `b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d`.

## Result

`oo_src_guc_login` is `LOGIN NOINHERIT` with `SET` membership of `oo_src_side` and a server `ALTER ROLE ... SET role`. On caller-CA verify-full, `session_user` is the login and `current_user` is `oo_src_side`. `pg_stat_ssl` returns no row. The connector records that as `unknown`, blocks registration with `role_mismatch` and `startup_option`, and refuses `catalog` with `session_user_mismatch`. It does not change source roles and does not send `SET ROLE`.

Accepting Linux arm64 Rust 1.94 run: `live_pg` 1 passed, lib 19 passed, doc 0 passed, clippy `-D warnings` exit 0. SQL for `app=oo-pg-source`: `SELECT=843`, `WITH=48`, `START=36`, `ROLLBACK=36`, no other verb, password hits 0, canary hits 0. Log sha256 `2652658b3fc92d6920942269ab338f05157128e9c1a81001ee55b89ec718e708`.

## Still pending

Full source on PostgreSQL 13 and 14, lossless types, bounded reader, import, CLI, supply, and amd64. The baseline package-test exit 101 without `OO_PG_FIXTURE` is preserved. a087 evidence was not edited.

Containers, network, volumes, and `/tmp/oo-a093-c68e02ce-53fd-4924-b766-eca8b482d1ac` for this run were removed. No prune.
