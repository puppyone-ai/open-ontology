# a104 — versioned target segment preflight

Session `eef831aa-dba4-4af2-a78b-541bc9e0a104`. One new migration, `0003_target_segment_preflight_expand.sql`, installs `target-fence-receipts/3`. It supersedes `0002` when the database is still on v1, and it upgrades a database that already stored the frozen v2 checksum. `0000`, `0001`, `0002`, and the v1 and v2 JSON files stay byte-equal.

The reviewed v2 file `9a71efaf9cb63a2d63061a241f6b8ee8b00426df` closes NULL segment holes. On a missing `_oo.apply_receipts` its eager `'_oo.apply_receipts'::pg_catalog.regclass` raises `42P01` before the `OO002` branch can run. That file stays. The reference applicator never schedules it.

## Identities

Assigned baseline, still an ancestor of the commit parent:

- commit `e84efd436dbc105b6a80df62464e1af7534e5af7`
- tree `26612f1cf30dc1c08046915e961fd868f8cfc572`
- parent `7da4e47e0f52890ded51e20df968cd3a198a1d2b`

Parent at the moment this report was committed:

- commit `3ca5357930a2c44c42d3061db5fabe577881269a`
- tree `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6`
- subject `connectors: decode PostgreSQL values into Arrow 60 batches`

The delivery commit is the commit that contains this file. Its parent is the commit above if HEAD had not moved. `git show --name-only` on that commit is the ownership check. This file does not contain the delivery SHA.

Workspace HEAD when the passing fixed archive was taken: `3ca5357930a2c44c42d3061db5fabe577881269a` (`linux/fixed/workspace-head-at-archive.txt`). Workspace HEAD when the baseline archive was taken: `4477f23205e989ac3c2207098efc369e6286cb98`.

## Why the old files stay

`crates/oo-pipeline/migrations/README.md` says a later edit of these migration files is a new version, not a silent rewrite. Installed v2 rows store the checksum of `0002`. Rewriting `0002` would change that checksum and would still leave every database that already applied the published bytes on the eager cast.

A migration that runs only after `0002` cannot repair a missing relation. `0002` aborts the transaction with `42P01`, so the later file is not reached. Rolling that transaction back leaves the database without the contracted `OO002` message. The live dead-path case records that result and does not treat it as a pass.

Rechecked in this workspace after the passing smoke, and inside that smoke:

| File | SHA-256 |
| --- | --- |
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0002_target_segment_null_expand.sql` | `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `target-fence-receipts-v1.json` | `429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521` |
| `target-fence-receipts-v2.json` | `3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac` |

`target-fence-receipts-v3.json` repeats the three SQL hashes above and records `0003` as `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`. The JSON file's own SHA-256, recomputed in this workspace, is `344b45bcdc57b736cfe394f92038922c9944f9695f3346b54ad04ac5c184ba3a`.

## Migration chain

`tests/segment_preflight_correction.rs` is the executable reference applicator. `route()` chooses the files. `reference_files()` never contains `0002`. `dead_path_files()` is `0002` then `0003` and is not a route.

| Observed database | Files the reference applicator executes | Marker |
| --- | --- | --- |
| Missing `_oo`, missing `_oo.apply_receipts`, or missing `apply_receipts_segment_check` | `0003` only | none |
| v1 present, v2 absent, relation and constraint present | `0003` only. Fresh install is `0000`, `0001`, `0003` | insert `target-fence-receipts/3` after `0003` returns |
| v2 row stores the frozen `0002` checksum | `0003` only | insert `target-fence-receipts/3`; v1 and v2 rows stay |
| v3 row stores the SHA-256 of this `0003` | none | `applied_at` stays |
| v3 row, or a v2 row, stores a different checksum | none | row stays; `OO001` |

A historical v2 database is built by applying the frozen `0002` only when the relation already exists, so that file can succeed. The reference upgrade that follows runs `0003` only.

`0003` assigns `pg_catalog.to_regclass('_oo.apply_receipts')` to an oid, looks up `apply_receipts_segment_check` by that oid, then `PERFORM 1 FROM _oo.apply_receipts WHERE false` inside a handler that re-raises static `OO002` on `undefined_table`. The message is `target-fence-receipts/1 segment relation is absent`. `LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE` is a top-level statement after that handler. The file contains no `::regclass` token. Comments name the historical cast. The closed segment function body, the history block, and the CHECK expression are the same bytes as `0002`. The unit test compares those slices and walks the 180-cell index/count/bitmap grid: 17 legal, 163 illegal.

Version 3 readiness is the conjunction of an externally computed SHA-256 of `0003` and `_oo.segment_preflight_ready(that digest)`. The function compares the argument to the stored `target-fence-receipts/3` row and does not open the file. `_oo.schema_ready` and `_oo.segment_contract_ready` do not authorize version 3. A stored lie makes the function true only for the lie. The applicator does not update or delete a stored checksum. `book_caller_supplied_checksum` is a test-only lie insert; `apply_reference` inserts the digest of the file it executed.

Owner checks run before any `DROP`. `commit_apply_receipt`, `segment_contract_ready`, `segment_preflight_ready`, `segment_service_readiness`, and `segment_preflight_readiness` must be absent or owned by `oo_target_owner`. A foreign owner raises `42501` and the replacement does not run. There is no `GRANT oo_target_owner TO oo_pipeline_runtime`.

Failures return before the marker insert, or raise inside the same transaction as the file. The absent path does not insert a marker. A forced `OO003` after `0003` returns rolls the transaction back.

## Checks

Linux arm64 Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (17.10, linux/arm64). No pull. Both smokes set `RUSTUP_TOOLCHAIN=1.94.0` inside that image. Capacity checks saw `host_cargo=0`. This continuation did not invoke a host `rustc`. The deciding compiler is the container `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Commands inside both smokes. Fetch has no `-p`:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
```

The fixed smoke adds, only after the package test exit is 0:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test segment_preflight_correction live_segment_preflight -- --ignored --exact
```

Baseline smoke, archive of `e84efd43` only, run `e44357a9-a8ec-4ce3-a394-22e15537b631`:

```
evidence/eef831aa-dba4-4af2-a78b-541bc9e0a104/SP04/segment-preflight/linux/baseline/launch.sh
```

`fetch.exit=0` `test.exit=0` `clippy.exit=0` `clippy-component.exit=0` `docker-run.exit=0` `worst.exit=0`. Host config in `linux/baseline/hostconfig.txt`: `privileged=false`, `capdrop=["ALL"]`, `ports={}`. That run is the pre-change package smoke. It does not execute `0003`.

First fixed launch, archive of `e84efd43` plus the five-file overlay. `launch.sh` exited 101 after 40.49s. The detached PostgreSQL container id printed by that launch was `ff0697ec5a564314b454eaa880a900af856ef1c749e38e86c93ed4828210420e`. The rerun deletes `console.log`, `summary.log`, `live-console.log`, and `cases/` before it starts, so those files on disk are the second run. The first console, read before that replacement, recorded:

- `phase_exit=0 phase=cargo_fetch_locked`
- package `cargo test` exit 0, with `unused_mut` on `TcpStream` in `segment_preflight_correction.rs`
- `phase_exit=101 phase=cargo_clippy_oo_pipeline` from four `-D warnings` errors: `clippy::byte_char_slices` on `write_all(&[b'Q'])`, `unused_mut` on `TcpStream`, `clippy::collapsible_if` on the booked rollback, `clippy::useless_format` on `contains(&format!("target-fence-receipts/1|"))`
- the live test still ran because the package test exit was 0, and failed in 0.49s: `acl 42883: function "_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)" does not exist`
- `phase_exit=101 phase=cargo_test_live_segment_preflight`, `elapsed_seconds=36`

`has_function_privilege(text, text, text)` does not resolve that packed signature. The rerun looks up `pg_proc.oid` and `pg_class.oid`. Receipt denial is `INSERT` on the table oid. The same four Clippy sites were edited before the rerun. The product path in that first summary had already reached the search-path year `2026` before `phase_acl` panicked. That summary file is gone.

Passing fixed smoke, run `bfb4014f-6724-4b46-9359-0d98a0586b64`, observed `2026-10-02T13:36:34Z`. Fresh `e84efd43` archive plus the same five-file overlay. Archive tar SHA-256 `bb8d6d3b0101558af05b01e3cacef6fd9bfb1a7cf725147cd6ab996bdad3cd99`. Capacity line: `builder_containers=0 host_cargo=0 own_postgres=0`.

```
evidence/eef831aa-dba4-4af2-a78b-541bc9e0a104/SP04/segment-preflight/linux/fixed/launch.sh
```

Inside the container the six hashes in the table above matched. `fetch.exit=0` `test.exit=0` `clippy.exit=0` `clippy-component.exit=0` `live.exit=0` `docker-run.exit=0` `worst.exit=0`. Package `segment_preflight_correction` was `ok. 2 passed; 0 failed; 1 ignored`. The deciding invocation printed:

```
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 4.70s
```

`phase_exit=0 phase=cargo_test_live_segment_preflight`, `elapsed_seconds=40`. `linux/fixed/inspect.txt`: `ExitCode=0 Status=exited OOMKilled=false`. Server line: `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu`. `linux/fixed/summary.log` ends with `live=pass`.

Deciding lines from that summary:

- empty database, `0003` only: `OO002`, `no_schema=true`. `cases/empty.stderr` is `OO002: target-fence-receipts/1 segment relation is absent`
- dead path `0002` then `0003` on bootstrap only: `42P01`. `cases/dead-path.stderr` is `42P01: relation "_oo.apply_receipts" does not exist`. The summary marks `reference_does_not_use_this_order`
- schema present, relation missing, and constraint missing: `OO002`, no marker
- fresh and populated lawful v1: `SupersedeFromV1`, receipts byte-equal, v2 marker absent
- existing v2: `UpgradeFromV2`, historical markers unchanged
- three NULL shapes, direct unknown row, and 8193: `23514`; absent-valid, present-valid, and 8192 inserted
- stale illegal `23514`, stale legal `OO013`
- replay immutable, conflict `OO010` with side rollback
- search-path poison rejected, stored year `2026`
- runtime insert `42501`, owner role not granted
- top-level share lock still conflicts after the preflight `DO`: `55P03` at 2011 ms
- illegal history `OO021`, receipts unchanged. `cases/bad.stderr` is the fixed `OO021` message
- redo `Skip`, stamp and constraint oid stable
- checksum mismatch `OO001`, row unchanged, file not executed
- forced `OO003`, rollback, no marker
- concurrent advisory lock `55P03` at 2012 ms
- stored lie: conjunction with the real file hash is false, v1 checksum unchanged
- foreign owner `42501`, function not adopted

`open_migration` labels a `55P03` from the advisory lock as route `AbsentPreflight` on the error value. `phase_lock` calls `apply_reference` and asserts SQLSTATE `55P03`, an empty executed-file list, elapsed time under 8 seconds, and zero v3 rows. That case passed. The label is not a second product route.

Posture for the fixed run is the `docker run` in `linux/fixed/launch.sh`: `linux/arm64`, `--pull never`, `--init`, `--cap-drop ALL`, `no-new-privileges`, private bridge, no published ports, no docker socket, no `HOME` mount. PostgreSQL adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, trust auth, `--data-checksums`, memory 1g, cpus 1, pids 256. The Rust builder is user `0:0`, memory 8g, cpus 4, pids 4096, `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target`, `HOME=/tmp`.

## Cleanup

The fixed `EXIT` trap removes the Rust container, the PostgreSQL container, the run network, and the PostgreSQL volume, then exits with the docker run status. Retained names from the passing run:

- removed container `oo-a104-msrv-bfb4014f-6724-4b46-9359-0d98a0586b64`
- removed container `oo-a104-pg-bfb4014f-6724-4b46-9359-0d98a0586b64`
- removed network `oo-a104-bfb4014f-6724-4b46-9359-0d98a0586b64-net`
- removed volume `oo-a104-pg-bfb4014f-6724-4b46-9359-0d98a0586b64`
- retained volumes `oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-cargo` and `oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-target`

A check in this continuation, `docker ps --filter label=oo.owner=a104`, printed no containers. `docker volume ls` still lists those two cargo and target volumes. No prune. The foreign PostgreSQL container `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3` was not addressed.

## Scope

Owned paths are the new migration, the two READMEs, `target-fence-receipts-v3.json`, `tests/segment_preflight_correction.rs`, and this evidence directory. Root manifests, the lockfile, jobs, the CLI, connectors, older evidence, the spec, and the plan were not edited. No root dependency was added. The new test has no `unsafe` block. Workspace `unsafe_code = "forbid"` was already in force; the 1.94 Clippy run exited 0.

A085-F2, F3, and F5 stay open. An owner that can set the writer GUC can still insert. A receipt does not fence arbitrary DML. `owner_token` is not an external worker identity.

## Not claimed

SP04-W05 completion, AT-PL-05, a production Rust Loader, CDC, a control-database mirror, stream acknowledgement, physical mapping, 100 workers, a second architecture, and release. Plan checkboxes were not edited. Stop for a separate independent review of this fixed commit.
