# Commands — a149 reader completion review

Fixed archive `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`. Smoke and both probe attempts compiled that archive. The green probe copied `overlay/pg_reader_r4_review.rs` into the archive snapshot before `cargo test`. It did not edit the git tree.

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/linux/probe/launch.sh
```

The probe script was run twice. The first run's files were moved to `linux/probe-1/` and container `oo-a149-probe` (`f7929b482799`) was removed. The second run wrote `linux/probe/`.

Postgres: `docker run -d --name oo-a149-pg --platform linux/arm64 --pull never --init --security-opt no-new-privileges --cap-drop ALL --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add SETUID --cap-add SETGID --memory 512m --memory-swap 512m --cpus 1 --pids-limit 256 --network oo-a149-eef831aa-net`. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `POSTGRES_PASSWORD_FILE=/secrets/super.pass`. No published ports. `pg/init.exit` 0. `pg/up.exit` 0. `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`.

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Smoke and probe:

```
docker run --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL \
  --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 \
  --network oo-a149-eef831aa-net
```

Environment: `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `CARGO_BUILD_JOBS=4`, `RUSTUP_TOOLCHAIN=1.94.0`, `HOME=/tmp`. Volumes `oo-a149-eef831aa-cargo` and `oo-a149-eef831aa-target`. Source mount is the archive snapshot, not the workspace checkout.

Inside smoke, after `rustup component add clippy --toolchain 1.94.0` and `cargo fetch --locked`:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Inside the green probe, after the same Clippy install, using the cargo volume smoke already filled (`--offline`):

```
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_r4_review -- --test-threads=1 --nocapture
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test run_complete_private_fail -- --test-threads=1
```

The private-construction `cargo test` is expected to fail compilation. `linux/probe/private-construct.cargo.exit` is 101. `linux/probe/private-construct.exit` is 0. The script then removes that test file and rechecks production hashes.

## Exits

| Path | Exit | Meaning |
| --- | --- | --- |
| `pg/init.exit` | 0 | PostgreSQL 17.10 initialized |
| `pg/up.exit` | 0 | container started |
| `linux/smoke/fetch.exit` | 0 | `cargo fetch --locked` |
| `linux/smoke/lib.exit` | 0 | 63 passed |
| `linux/smoke/clippy.exit` | 0 | scoped clippy |
| `linux/smoke/docker-run.exit` | 0 | container `a6b30c0b5efa` |
| `linux/probe-1/clippy.exit` | 101 | overlay `format!` compile |
| `linux/probe-1/docker-run.exit` | 101 | container `f7929b482799`, preserved |
| `linux/probe/clippy.exit` | 0 | green clippy |
| `linux/probe/pg_reader_r4_review.exit` | 0 | 1 passed in 12.83s |
| `linux/probe/private-construct.cargo.exit` | 101 | required compile failure |
| `linux/probe/private-construct.exit` | 0 | harness accepted that failure |
| `linux/probe/docker-run.exit` | 0 | container `f98559450ed7` |

No `cargo update`. Vendored `sqlx-postgres` unfulfilled `dead_code` expectations printed and did not fail `-D warnings` on `oo-connectors`.

## Cleanup after the evidence commit

This task only. No prune. No foreign containers, volumes, or networks.

```
docker rm oo-a149-smoke oo-a149-probe
docker stop oo-a149-pg
docker rm oo-a149-pg
docker volume rm oo-a149-eef831aa-cargo oo-a149-eef831aa-target oo-a149-eef831aa-pgdata oo-a149-eef831aa-pglog
docker network rm oo-a149-eef831aa-net
rm -rf /tmp/oo-a149-eef831aa-dba4-4af2-a78b-541bc9e0a149
```
