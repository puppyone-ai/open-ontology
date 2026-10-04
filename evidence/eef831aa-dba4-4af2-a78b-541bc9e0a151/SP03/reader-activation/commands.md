# Commands — a151 reader activation

Workspace `/Users/redux/puppy-one/open-ontology`. Fixed archive `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`. Source commit `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`.

## Exits

| Command | Exit | Notes |
| --- | --- | --- |
| `linux/smoke/launch.sh` | 0 | Before the edit. `lib.exit` 0, `clippy.exit` 0, `fetch.exit` 0, `toolchain.exit` 0, `snapshot.exit` 0. Lib `63 passed`. Elapsed 40s. |
| `pg/up.sh` | 0 | `init.exit` 0. `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. |
| first `linux/probe/launch.sh` | 101 | Preserved as `linux/probe-1`. `lib.exit` 0. `clippy.exit` 101. E0716 in `lossless_live.rs`. Elapsed 5s. |
| second `linux/probe/launch.sh` | 0 | `lib.exit` 0, `clippy.exit` 0, `pg_reader_complete_live.exit` 0, `overlay.exit` 0, `toolchain.exit` 0. Live test 1 passed in 2.71s. Elapsed 10s. |

Smoke container `2a75acb6942092fa2411ec7037d3303924f11e22c1c29be591ed63b1282f63b3`. Failed probe container `deb98dde19cc1dd2ccb57bca6690b07d047ebdaa274f2367091ae7022eddb995`. Green probe container `1fe07557f128da806d717e04862dd697f2582f85e866760956a2ffd4627a36e5`. Postgres container `40d07c9b4d287b7f02c88d2e540b5150d659c5d8b13195ee7023d5506a3880a5`.

## Inside the containers

Smoke:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Green probe:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1 --nocapture
```

`OO_PG_COMPLETE_FIXTURE=/secrets/pointer.json`. `OO_PG_COMPLETE_OUTCOME=/evidence/complete-outcome.txt`. The fixture env was set only for the live test. Passwords were not process arguments.

A blanket `cargo test -p oo-connectors --all-targets` was not run. It would start the other live binaries. Those stay covered by the unchanged R4 blobs and by Clippy `--all-targets` typechecking.

## Cleanup

Run after this evidence commit. Own names only. Do not prune.

```
docker rm -f oo-a151-probe oo-a151-pg
docker volume rm oo-a151-eef831aa-cargo oo-a151-eef831aa-target oo-a151-eef831aa-pgdata oo-a151-eef831aa-pglog
docker network rm oo-a151-eef831aa-net
rm -rf /tmp/oo-a151-eef831aa-dba4-4af2-a78b-541bc9e0a151
```

`oo-a151-smoke` was removed before the green probe. The failed probe container was removed before the green probe reused the name `oo-a151-probe`.
