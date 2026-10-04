# a175 report — SQL admission

Task `a175`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a175`. Repository `/Users/redux/puppy-one/open-ontology`. Source commit `03512f91324ac33e772e9702fe806e8e04037ec4` (`gateway: admit one SELECT through the PostgreSQL parser`), parent `93e6b6a24fab71c10dd9966ee28d5258516bed63`.

`oo_gateway::sql_guard::guard_select` parses one statement with `pg_query` 6.2.1, rewrites the AST from a caller-supplied snapshot, and deparses. Contract `contracts/gateway/sql-guard-v1.md`. SP06-W02 stays open. No release is claimed.

## Pre-edit smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `linux/docker-run.exit` is 0. Copies of that run's phase exits are `linux/phases/smoke-clippy.exit`, `linux/phases/smoke-fetch.exit`, and `linux/phases/smoke-test.exit`, each 0. The smoke ran before `sql_guard.rs` existed.

## Focused build

The same Rust image, one container, name `oo-a175-rust194`, id `057629845c905838df1e607d613a8db5f5d6601769688f0b71e3953f52e91204`. Private bridge `oo-a175-net`, cap drop `ALL`, no new privileges, no host network, no published port, no host home, `HOME=/tmp`, cargo volume `oo-a175-cargo`, target volume `oo-a175-target`. `RUSTUP_TOOLCHAIN=1.94.0`. Host cargo processes were 0 and this was the only running rust-builder while it compiled.

The green run is `linux/compile.exit` = 0 and `linux/compile-run.log` ending in `compile_ok`.

| Phase | Exit |
| --- | --- |
| libclang extract (already present) | 0 |
| libclang load (`ld-linux --list`) | 0 |
| clippy component | 0 |
| `cargo fetch` (adds the manifest delta) | 0 |
| `cargo update -p pg_query --precise 6.2.1` | 0 |
| lock delta against the pre-fetch snapshot of that run | 0 |
| `cargo test --locked --offline -p oo-gateway --lib` | 0 |
| `cargo test --locked --offline -p oo-gateway --test sql_guard` | 0 |
| `cargo clippy --locked --offline -p oo-gateway --all-targets -- -D warnings` | 0 |
| `cargo run -p oo-arch-check --locked --offline` | 0 |

Lib tests: 5 passed (`linux/lib-test.log`). SQL guard tests: 24 passed in 0.04s (`linux/sql-guard-test.log`). That run left `OO_A175_LIVE` unset, so `live_column_role_executes_guarded_select` returned before any database call. Clippy finished with no warnings (`linux/clippy.log`, `linux/phases/clippy.exit` = 0). Architecture check printed `architecture check: pass` (`linux/arch-check.log`).

`pg_query` 6.2.1 checksum in `Cargo.lock` is `da43168013d137b3ca49b0044c1108ec09ff596a0626d4db89209c0a1d450cd2`. Parser protobuf version on the live plan is 170007.

## Failed attempts kept

| Record | Stop |
| --- | --- |
| `linux/apt-update.log` | `apt-get` exit 100 under cap-drop `ALL` (cannot drop to `_apt`) |
| `linux/lock/update-absent.101.log` | `cargo update -p pg_query --precise 6.2.1` before the package was in the lock |
| `linux/lock/fetch-pin-usage.log` | `cargo fetch -p` is not a cargo flag |
| `linux/lib-test-stdbool.log` | bindgen: `stdbool.h` file not found |
| `linux/lib-test-string-cmp.log` | `String == &String` in qualifier matching |
| `linux/sql-guard-test-depth.log` | 140 `NOT`s returned `Parse` (prost recursion), expected `Budget` |
| `linux/clippy-collapsible.log` | eight clippy `-D warnings` on the library |
| `linux/lib-test-fold-derive.log` | duplicate `#[derive]` on `Fold` |

Later compile runs re-snapshotted `linux/lock/before.txt` after `pg_query` was already locked, so those runs wrote empty added/removed lists. Those empty files are `linux/lock/added-last-rerun.txt` and `linux/lock/removed-last-rerun.txt`. The delta against `HEAD` before the source commit is `linux/lock/added.txt` and `linux/lock/removed.txt`.

## Database

One PostgreSQL container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (`pg/image.txt`, linux arm64), name `oo-a175-pg`, id `313158fcea9c38d165e9f3e955b0f5d19bc5e69f2dde0c33e995b2244d79b941`. `pg/up.exit` is 0. `pg/server-san.txt` has DNS `oo-a175-pg`. `pg/ports.json` is `{}`. `pg/caps.json` drops `ALL` and adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. The container was not privileged. Passwords and the CA private key lived under `/tmp/oo-a175-eef831aa-dba4-4af2-a78b-541bc9e0a175/secrets` and were removed in cleanup.

`linux/sql-guard-live.log`: `live_column_role_executes_guarded_select` passed, 0.25s, with `OO_A175_LIVE=1`. Oracle `live/oracle.txt`. The oracle does not contain either password.

- Physical table `pt_443991dc03bf039e80f17e8752527fbd667c9aa81f384dfb76db3d8dc785`. Visible columns `pc_10ca3cbe…` (`order_id`) and `pc_df3ed6fe…` (`region`).
- Integer OID 23, text OID 25, `count` OID 2803. Parser version 170007.
- Executed SQL is the deparsed physical projection with logical output names `order_id` and `region`. SHA-256 `97c86c99a18f02230327a8fbdf031208358c58ee330cc2c52cbd3deb148abeff`.
- Under `search_path` empty and `SET ROLE oo_r_dispatcher`, that SQL returned 42 and west. `SELECT order_id, region … WHERE order_id = 42` returned 42.
- Unguarded `SELECT * FROM logistics.shipment` returned SQLSTATE `42P01`. That name is not a physical relation. This is not a measured `42501`.
- Guarded `count(*)` executed and returned one row (`count_sqlstate=ok:1`). Admission of `count(*)` is not a column-GRANT proof.

## Dependency delta

`linux/lock/removed.txt` is empty. `linux/lock/added.txt` lists 35 new name/version pairs, including `pg_query` 6.2.1. Existing package checksums in `Cargo.lock` are unchanged (0 missing, 0 checksum changes against the parent commit's lock). `sha2` 0.10.9 remains. Root `Cargo.toml` is unchanged. `cargo deny` and `cargo audit` were not run.

The mandated precise update could not select `pg_query` until an unlocked `cargo fetch` had written it. That fetch is not a broad `cargo update`. The precise update then exited 0.

## Cleanup

`cleanup.txt`. Removed `oo-a175-rust194`, `oo-a175-pg`, volumes `oo-a175-cargo`, `oo-a175-target`, and `oo-a175-pgdata`, network `oo-a175-net`, and `/tmp/oo-a175-*`. Each recorded command exited 0. Afterwards own containers, volumes, the network, and `/tmp/oo-a175-*` counts were 0. No other container was removed. `docker prune` was not used.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list. Source bytes are under `source/blobs/`.
