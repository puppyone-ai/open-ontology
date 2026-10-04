# Commands — a147 reader completion

Fixed archive `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`. Smoke ran on that archive before the product edit was compiled. Probe-3 compiled the same archive plus the overlay whose content hashes are the production commit `16ca2969cd449653a64d98d9831d3c8b8976942a`.

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a147/SP03/reader-completion/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a147/SP03/reader-completion/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a147/SP03/reader-completion/linux/probe/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a147/SP03/reader-completion/linux/probe-2/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a147/SP03/reader-completion/linux/probe-3/launch.sh
```

Smoke and probes use `docker run --platform linux/arm64 --pull never --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 --network oo-a147-eef831aa-net`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `CARGO_BUILD_JOBS=4`, `RUSTUP_TOOLCHAIN=1.94.0`, `HOME=/tmp`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Inside the smoke container, after `cargo fetch --locked` and `rustup component add clippy --toolchain 1.94.0`:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Probe-3, after the same Clippy component install:

```
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_finish_live -- --test-threads=1
```

Results: smoke `docker-run.exit` 0. Probe `docker-run.exit` 1. Probe-2 `docker-run.exit` 101. Probe-3 `docker-run.exit` 0. Postgres `pg/init.exit` and `pg/up.exit` are 0. `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`.

Cleanup after the evidence commit, this task only:

```
docker rm oo-a147-smoke oo-a147-probe oo-a147-probe-2 oo-a147-probe-3
docker stop oo-a147-pg
docker rm oo-a147-pg
docker volume rm oo-a147-eef831aa-cargo oo-a147-eef831aa-target oo-a147-eef831aa-pgdata oo-a147-eef831aa-pglog
docker network rm oo-a147-eef831aa-net
rm -rf /tmp/oo-a147-eef831aa-dba4-4af2-a78b-541bc9e0a147
```

Do not `docker system prune`. Do not remove containers whose names do not start with `oo-a147-`.
