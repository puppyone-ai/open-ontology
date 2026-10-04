# a154 unpublished PostgreSQL full import

Session `eef831aa-dba4-4af2-a78b-541bc9e0a154`. Source commit `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4`, tree `da85327b402378cfdc3b826300205569c43b3642`, parent `2996562ceb332cc0ce505c703e775615fa321e5b`. Fixed base `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`. The parent is later repository history. The Linux archive was `git archive` of the fixed base plus the owned overlay. It was not an archive of `HEAD`.

`0000` through `0004`, the COPY encoder, physical mapping, the connector, `Cargo.toml`, and `Cargo.lock` are unchanged from the fixed base. The product commit adds ten paths: `full_import.rs`, `copy_sink.rs`, `0005_full_import_expand.sql`, the full-import contract, the live tests, and narrow additions to `lib.rs`, `target_installer.rs`, `migrations/README.md`, and `target_installer_live.rs`. Blob and file hashes are in `SOURCE.txt`.

`import_unpublished` reads through the existing `SnapshotOwner`, encodes with the existing public COPY encoder, and `COPY`s on the same target connection that holds the shard advisory lock, owner, generation, and backend identity. Data, masked quality, and the apply receipt commit in that transaction. The returned state is `awaiting_publish` after `finish_complete` yields `RunComplete`. `install` does not apply `0005` and cannot report that state. Control mirror, job, and stream acknowledgement are not written.

## Images and toolchain

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `verify/image.txt` and `smoke/image.txt` record `id=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. `pg/image.txt` records that id. The live server string is `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu, compiled by gcc (Debian 12.2.0-14+deb12u1) 12.2.0, 64-bit`.

Both images were local. Pull was `never`. The builder had cap-drop ALL, memory 8589934592, privileged false, no host network, no docker socket, no home mount, and no private-key mount. Passwords stayed in `/tmp/oo-a154-eef831aa/secrets`.

## Smoke, frozen base, before the product edit

Container `oo-a154-full-import-smoke`. Archive of `bedd6a2c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`. Phases rustup-clippy, fetch, and `cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings` all exited 0. `docker-run.exit=0`. `inside.exit=0`. Raw log `smoke/console.log`.

## Green verify

One container `oo-a154-full-import-verify` on the already running `oo-a154-pg`. Launch printed `reusing running postgres oo-a154-pg`. `0005` SHA-256 in the mounted archive was `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6`, matching `contracts/pipeline/full-import-v1.json`.

Phase exits, all 0: toolchain, lib, installer, full-import, rustup-clippy, clippy, inside, docker-run. Host config: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a154-eef831aa-net`, Memory 8589934592, CapDrop ALL, no port bindings.

Lib: `ok. 11 passed; 0 failed`, 0.01s. Installer live: `ok. 1 passed`, 0.62s, including the existing bootstrap, replay, v1, v2, `OO002`, `OO001`, `OO021`, and runtime refusal, plus `full_import_installed=1`, `full_import_replay=1`, `full_import_oo001=OO001`, `full_import_runtime_install=not_administrator`. Full import live: `ok. 1 passed`, 37.51s. Clippy was the same `-D warnings` command and exited 0. `sqlx-postgres` printed two unfulfilled `dead_code` expectations. Those are dependency warnings. The `oo-pipeline` crate produced none.

The two live tests were separate `cargo test` invocations.

## Live full-import oracle

`verify/live/outcome.txt` and `verify/full-import.stdout`:

```
server=PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu, compiled by gcc (Debian 12.2.0-14+deb12u1) 12.2.0, 64-bit
schema_setup_runs=0
main_state=awaiting_publish
main_rows=4
main_generation=1
agent_denied=1
backpressure_chunks=2
oversized=retired
unsupported_jsonb=refused
coherent_cut=2_then_3
cancel=retired
deadline=retired
snapshot_lost=retired
same_shard=blocked
other_shard=imported
takeover=incremented
rollback=absent
crash_before=absent
crash_after=replay
conflict=aborted
source_reader_writes=0
full_import_live_ok
```

`schema_setup_runs=0` is printed after the test counted zero rows in `_oo.full_import_run` before `import_unpublished`. The main import then returned `AwaitingPublish` with two tables and four rows at target generation 1. The agent login was denied `SELECT` on the shadow, `SELECT` on `_oo.full_import_quality`, and `INSERT` into the shadow. Source reader writes stayed 0. A writer committed during the first cut; the next generation read 3 rows. Same-shard fence blocked the second writer. A different shard imported. Takeover incremented the generation. Rollback left the batch absent. Crash before the commit response left it absent. Crash after was recovered as replay of the stored receipt. A conflicting payload aborted. `jsonb` was refused. The oversized table retired the run. Backpressure held two chunks. This is the representative type set in the fixture, not a certification of every PostgreSQL type.

## Backend visibility grant

`oo_target_owner` is `NOINHERIT` in unchanged `0000`. A security-definer function owned by that role could not see the caller's `pg_stat_activity.backend_start`, so `full_import_bind_session` raised `OO018`. `install_full_import` grants `pg_read_all_stats` to `oo_target_owner` `WITH INHERIT TRUE, SET FALSE` before `SET LOCAL ROLE`. `0005` checks `pg_has_role('oo_target_owner', 'pg_read_all_stats', 'usage')` and, when the executor is still superuser and usage is absent, runs that same grant. Otherwise it raises `42501`. `SET FALSE` blocks assuming the stats role. `oo_pipeline_runtime` and the Agent login are not members. The membership lets the fence read `backend_start`. It does not replace the pid, owner, generation, or held shard-lock checks. It is not a security proof.

## Failed attempts kept

| directory | docker-run | what failed |
| --- | --- | --- |
| `verify/attempt-1-lib-101` | 101 | lib: `tx` in `checksum_mismatch` was not `mut`; `as_slice` deprecation warnings |
| `verify/attempt-2-full-import-101` | 101 | full-import compile: moved host, admin, CA; `ImportSession` has no `Debug` |
| `verify/attempt-3-oo018` | 101 | live `OO018` target backend identity is not bound |
| `verify/attempt-4-oo018-noinherit` | 101 | same `OO018` after a grant stored with inherit false |
| `verify/attempt-5-writer-audit` | 101 | fixture writer lacked `INSERT` on `app.write_audit` |
| `verify/attempt-6-deadline-open` | 101 | deadline error left run state `open` |
| `verify/attempt-7-replay-oo013` | 101 | replay adopted a new owner and the fence raised `OO013` |
| `verify/attempt-8-tests-green-clippy-missing` | 1 | tests passed; clippy component was absent in that container |
| `verify/attempt-9-clippy-lib` | 101 | tests passed; clippy denied 9 lib lints |

Lib, installer, and full-import exits for attempts 3 through 7 are 0, 0, and 101. Attempts 8 and 9 have full-import exit 0. Their stdout contains `full_import_live_ok`. Those runs are preserved and are not this delivery's acceptance run. Acceptance is the green verify root above, compiled from the committed sources.

## Cleanup

`CLEANUP.txt`. a154 containers, volumes, network, and `/tmp/oo-a154-eef831aa` are gone. `docker prune` was not run.
