# REPORT a093 source role GUC identity correction

Session `eef831aa-dba4-4af2-a78b-541bc9e0a093`. Fix commit `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362`, tree `c35b95664d53dcef1497bbd224ecbeec65586cea`, parent `35320e9f691cc3c879eb371446101b01839dcb8e`. No push, pull request, or release.

## Baseline

| Item | Value |
|---|---|
| Commit | `fc877c482a61729ea46e779a674a46f8f72c80c5` |
| Tree | `d40755c31abd82d932b56886aa048ab4d7aa5e4b` |
| Parent | `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df` |
| Reviewed integration | `2980e85af22c3ade05272c66c7d2835b52730322` |
| Integration tree | `c027c4599cdebf701b21e97e51ded45b9067ca8e` |
| Integration parent | `1868ffbca151c64a21efd6d1a09cb4806b1e95f3` |
| Archive files | 3365, matching `git ls-tree` |
| `Cargo.lock` sha256 | `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057` |
| `crates/oo-connectors/Cargo.toml` sha256 | `b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d` |
| Baseline `session.rs` sha256 | `125a21fa8c4773d0adba0640689c79722f2584c1ad50e0b4efb99f0a16db3068` |

Validation compiled a `git archive` of that baseline plus overlays of `session.rs`, `sql.rs`, `model.rs`, and `tests/live_pg.rs`. The dirty worktree was not the compiler input. `Cargo.lock` and the connector manifest were not edited. HEAD's lock differs from the baseline by two `oo-builder` dependency lines (`sha2 0.10.9` and `unicode-normalization`). Those lines are not in this commit.

## What changed

A missing `pg_stat_ssl` row is `server_observation=unknown` and `negotiated_version=unknown`. `ssl=false` remains `tls_not_negotiated`. `ssl=true` still requires a `TLSv1.` version of at most 32 bytes and no NUL. The driver still connects with `sslmode=verify-full` and the caller CA only.

`IDENTITY` stays one `SELECT`. It does not send `SET ROLE`, `RESET ROLE`, or `SET SESSION AUTHORIZATION`. `role` is absent from `pg_settings`, so the statement adds `role` when `current_setting('role')` is distinct from `none`. Sources `user`, `database`, and `database user` join the existing client, session, and startup filter. Names on the allow-list are unchanged.

`inspect_access` returns `role_mismatch` / `session_user` and `startup_option` / `startup_options`, with registration blocked. `catalog` returns `Config` / `session_user_mismatch` before a snapshot. No `pg_read_all_stats` grant and no security definer.

## Commands and exits

Run id `c68e02ce-53fd-4924-b766-eca8b482d1ac`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, `--init`, bridge network, no published ports, private cargo and target volumes, `HOME=/tmp`. One Rust container at a time. Host `cargo` / `rustc` / `rustdoc` / `clippy-driver` count was 0 before each launch. Process checks used pid, ppid, etime, and comm only.

Focused smoke, snapshot only, no fixture:

```
cargo fetch --manifest-path /src/Cargo.toml --locked
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --doc
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

| Phase | Exit |
|---|---|
| clippy component | 0 |
| fetch | 0 |
| lib | 0, 14 passed |
| doc | 0, 0 passed |
| package test | 101 |
| clippy | 0 |
| container `oo-a093-msrv-c68e02ce-53fd-4924-b766-eca8b482d1ac` | 101, oom false |

The package-test 101 is the frozen `live_pg.rs` panicking because `OO_PG_FIXTURE` is absent. That history stays.

RED, baseline `session.rs` plus the new `live_pg.rs`, fixture attached:

```
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test live_pg -- --nocapture
```

Container `oo-a093-red-c68e02ce-53fd-4924-b766-eca8b482d1ac` exit 101, oom false. Panic at `live_pg.rs:633`: `guc connect: SourceError { kind: Tls, fact: Some("tls_not_negotiated"), sqlstate: None }`. Finished in 3.90s. Production code was changed after this panic.

Probe on the same server, verify-full, passwords not printed: `oo_src_guc_login|oo_src_side|null|null`, reader `oo_src_reader|oo_src_reader|true|TLSv1.3`, `current_setting('role')` `oo_src_side` for the GUC login and `none` for the reader, `pg_db_role_setting` `role=oo_src_side`. Server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. The proposed extra string was `role` for the GUC login and empty for the reader.

Accepting run `oo-a093-green2-c68e02ce-53fd-4924-b766-eca8b482d1ac` exit 0, oom false. Overlays were the final `session.rs`, `sql.rs`, `model.rs`, and `live_pg.rs`. Same fixture, log truncated after the RED archive. Locked offline. No fetch.

| Phase | Exit |
|---|---|
| clippy component | 0 |
| `live_pg` | 0, 1 passed, 4.55s |
| lib | 0, 19 passed |
| doc | 0, 0 passed |
| clippy `-D warnings` | 0 |
| container | 0, oom false |

The earlier green container exit 101 was `ServerTls` missing `Debug`, which `unwrap` requires. `#[derive(Debug)]` was added and this run is the one that passed. Its live phase also passed before that compile fix; the accepting binary includes the derive.

`live_source_catalog` covers the reader registration with `server_observation=observed`, the GUC login (`role_mismatch`, `startup_option`, `negotiated_version=unknown`, `server_observation=unknown`, write `unknown`, empty memberships and definers, catalog `session_user_mismatch`), inherit / set-only / definer / operator / cast / admin roles, wrong CA, wrong host, and the child `PGOPTIONS` probes. The probes use `Command::env`. The parent environment is not changed. `std::env::set_var` is not used.

## SQL audit

Application `oo-pg-source` only. Password-file hits 0. Comment-canary hits 0.

| Log | Statements | SELECT | WITH | START | ROLLBACK | Other | sha256 |
|---|---|---|---|---|---|---|---|
| RED, aborted at the GUC connect | 859 | 751 | 44 | 32 | 32 | none | `1829482f438833b70d63c22bcde0557aac1dd2db388a054d7f64667a83575f6a` |
| Accepting run | 963 | 843 | 48 | 36 | 36 | none | `2652658b3fc92d6920942269ab338f05157128e9c1a81001ee55b89ec718e708` |

The in-test audit line was `sql_audit ROLLBACK=36,SELECT=843,START=36,WITH=48`. Against the a083 counts (`SELECT=840`, `START=36`, `WITH=48`, `ROLLBACK=36`, 960 statements), the accepting run adds three `SELECT`s: identity, session guards, and version on the GUC session. That session does not `START TRANSACTION`.

Log bodies were hashed and deleted with `/tmp/oo-a093-c68e02ce-53fd-4924-b766-eca8b482d1ac`. They are not in this evidence tree.

## Cleanup

Exact names for run `c68e02ce-53fd-4924-b766-eca8b482d1ac` only. No prune. After cleanup, `docker inspect` of the five containers, the network, and the three volumes exits 1. `/tmp/oo-a093-c68e02ce-53fd-4924-b766-eca8b482d1ac` is absent. `docker ps -a` has no `oo-a093-` name. Details are in `commands/cleanup.txt`.

## Left open

PostgreSQL 13 and 14, lossless types, bounded reader, import, CLI, supply, and amd64 were not run. a087's missing log mount, oracle `set_var` compile failure, oracle runtime 101, and cargo-deny history were not rewritten.
