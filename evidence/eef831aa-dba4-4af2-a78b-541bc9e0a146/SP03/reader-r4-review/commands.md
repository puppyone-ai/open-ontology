# Commands — a146 R4 review

Fixed archive `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`. Smoke ran before every probe.

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/probe/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/probe-2/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/probe-3/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/probe-4/launch.sh
```

Smoke and probes use `docker run --platform linux/arm64 --pull never --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 --network oo-a146-eef831aa-net`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `CARGO_BUILD_JOBS=4`, `RUSTUP_TOOLCHAIN=1.94.0`, `HOME=/tmp`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Inside the smoke container, after `cargo fetch --locked` with no `-p`:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Probe-1 runs, in order, `--test-threads=1`: `pg_stream_live`, `pg_stream_bounds`, `pg_stream_acl_live`, `pg_reader_keys_live`, `pg_reader_cancel_live`, `pg_reader_drop_live`, `pg_reader_finish_live`, `pg_reader_r4_probe`. Probe-2 runs ACL, then the r4 overlay. Probe-3 and probe-4 run only `pg_reader_r4_probe`. The source mount is a git archive. The overlay copied onto that archive is `overlay/pg_reader_r4_probe.rs`. Probe-4 is the run whose outcome is the early-finish evidence.

Results: smoke `docker-run.exit` 0. Probe-1, probe-2, and probe-3 `docker-run.exit` 101. Probe-4 `docker-run.exit` 0.

Cleanup after the evidence commit, this task only:

```
docker rm oo-a146-smoke oo-a146-probe oo-a146-probe-2 oo-a146-probe-3 oo-a146-probe-4
docker stop oo-a146-pg
docker rm oo-a146-pg
docker volume rm oo-a146-eef831aa-cargo oo-a146-eef831aa-target oo-a146-eef831aa-pgdata oo-a146-eef831aa-pglog
docker network rm oo-a146-eef831aa-net
rm -rf /tmp/oo-a146-eef831aa-dba4-4af2-a78b-541bc9e0a146
```

Do not `docker system prune`. Do not remove containers whose names do not start with `oo-a146-`.
