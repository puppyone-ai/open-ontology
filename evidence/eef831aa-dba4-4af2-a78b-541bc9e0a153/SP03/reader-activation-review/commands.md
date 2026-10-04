# Commands — a153 reader activation review

Workspace `/Users/redux/puppy-one/open-ontology`. Fixed archive `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`.

## Exits

| Command | Exit | Notes |
| --- | --- | --- |
| `pg/up.sh` | 0 | `init.exit` 0. `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. |
| `linux/smoke/launch.sh` | 0 | `toolchain.exit` 0, `snapshot.exit` 0, `fetch.exit` 0, `lib.exit` 0, `clippy-component.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Lib `63 passed; 0 failed`. Elapsed 41s. |
| `linux/probe/launch.sh` | 0 | `toolchain.exit` 0, `archive.exit` 0, `pg_reader_complete_live.exit` 0, `docker-run.exit` 0. Live test 1 passed in 2.76s. Elapsed 7s. |

Smoke container `97ecb89857129ffc225f010388d4b90b9d93b3caa58578a59721ad285f10ff3c`. Probe container `37d10d0c33043d1c289302741c7dd9ed328c81c72d5d3b12c415fc301ceef0b4`. Postgres container `7f21d568891e46565b0ae6530e4216d2e51eef8072eaaa7d7665438342f35b71`.

## Inside the containers

Smoke:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Probe:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1 --nocapture
```

`OO_PG_COMPLETE_FIXTURE=/secrets/pointer.json`. `OO_PG_COMPLETE_OUTCOME=/evidence/complete-outcome.txt`. Passwords were file mounts, not process arguments.

A blanket `cargo test -p oo-connectors --all-targets` was not run. That command starts every live binary. Those binaries stay covered by the unchanged R4 blobs and by Clippy `--all-targets` typechecking. The executed tests are the library suite and `pg_reader_complete_live` once.

## Cleanup

Run after this evidence commit. Own names only. Do not prune.

```
docker rm -f oo-a153-probe oo-a153-pg
docker volume rm oo-a153-eef831aa-cargo oo-a153-eef831aa-target oo-a153-eef831aa-pgdata oo-a153-eef831aa-pglog
docker network rm oo-a153-eef831aa-net
rm -rf /tmp/oo-a153-eef831aa-dba4-4af2-a78b-541bc9e0a153
```

`oo-a153-smoke` was removed before the probe. Its logs remain under `linux/smoke/`.
