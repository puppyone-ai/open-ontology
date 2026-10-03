# Commands — a144 finish semantics

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a144/SP03/finish-semantics/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a144/SP03/finish-semantics/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a144/SP03/finish-semantics/linux/probe-1/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a144/SP03/finish-semantics/linux/probe-2/launch.sh
```

Smoke and both probes use `docker run --platform linux/arm64 --pull never --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 --network oo-a144-eef831aa-net`, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `CARGO_BUILD_JOBS=4`, `RUSTUP_TOOLCHAIN=1.94.0`, `HOME=/tmp`. Probe-2 adds `OO_PG_FINISH_FIXTURE=/secrets/pointer.json` and `OO_PG_FINISH_OUTCOME=/evidence/finish-outcome.txt`. The source mount is a git archive of `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`. Probe-2 copies only `crates/oo-connectors/tests/pg_reader_finish_live.rs` onto that archive.

Owned source commit, after probe-2:

```
git add -- crates/oo-connectors/tests/pg_reader_finish_live.rs contracts/source/pgsql/reader-lifecycle-v1.md
git commit -F -  # connectors: record finish after a failed portal read
```

Result `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`.

Cleanup after the evidence commit, this task only:

```
docker rm oo-a144-smoke oo-a144-probe-1 oo-a144-probe-2
docker stop oo-a144-pg
docker rm oo-a144-pg
docker volume rm oo-a144-eef831aa-cargo oo-a144-eef831aa-target oo-a144-eef831aa-pgdata oo-a144-eef831aa-pglog
docker network rm oo-a144-eef831aa-net
rm -rf /tmp/oo-a144-eef831aa-dba4-4af2-a78b-541bc9e0a144
```
