# Commands — a143 owner Drop acceptance

Scripts under this directory are the runnable record. They were invoked with `bash` so `revision:path` survived the host shell. HEAD was `c83184f2f930435e17dd7c87d06b9d8ce7cc1554` before and after each archive. `--pull never`. No prune, no push.

```bash
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a143/SP03/owner-drop-review/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a143/SP03/owner-drop-review/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a143/SP03/owner-drop-review/linux/old-1/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a143/SP03/owner-drop-review/linux/probe-1/launch.sh
```

Smoke `inside.sh`: `cargo fetch --locked`, then `cargo test --locked --offline -p oo-connectors --lib`, then `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings`. Toolchain gate: `rustc` must print `rustc 1.94.0` and host `aarch64-unknown-linux-gnu`.

Old-1 `inside.sh`: one `cargo test --offline --locked -p oo-connectors --test pg_reader_drop_live -- --nocapture --test-threads=1` against archive `58afa97ded85a7dd89c72532bfc8973c45255d7c` plus `pg_reader_drop_live.rs` from `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`. Requires smoke `lib.exit` and `clippy.exit` to be 0.

Probe-1 `inside.sh`: the same cargo test invocation for `pg_reader_drop_live`, then `pg_reader_cancel_live`, then `pg_reader_keys_live`, on archive `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`. Each phase writes its own `.exit` file. The container exit is the first non-zero phase. Fixture env: `OO_PG_DROP_FIXTURE`, `OO_PG_CANCEL_FIXTURE`, and `OO_PG_KEYS_FIXTURE` point at `/secrets/pointer.json`. Outcomes land in `/evidence/{drop,cancel,keys}-outcome.txt`.

Postgres window copy is the perl block in each `inside.sh`: seek to the byte mark in `log-mark-before.txt`, redact `reader.pass` / `writer.pass` / `super.pass`, write `postgres-window.log`.

Rust `docker run` flags, from `linux/probe-1/launch.sh` (smoke and old-1 use the same isolation): `--platform linux/arm64 --pull never --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 --network oo-a143-eef831aa-net`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `CARGO_BUILD_JOBS=4`, `HOME=/tmp`.

Cleanup after the evidence files were written, only names owned by this task:

```bash
docker rm oo-a143-smoke oo-a143-old-1 oo-a143-probe-1
docker stop oo-a143-pg
docker rm oo-a143-pg
docker volume rm oo-a143-eef831aa-cargo oo-a143-eef831aa-target oo-a143-eef831aa-pgdata oo-a143-eef831aa-pglog
docker network rm oo-a143-eef831aa-net
rm -rf /tmp/oo-a143-eef831aa-dba4-4af2-a78b-541bc9e0a143
```

Results of that cleanup are in `cleanup.txt`. No `docker system prune`. a141 and a142 volumes were left in place.
