# Report — a151 reader activation

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a151`. Slot B. Sole coder. No subagents and no worktree. Slot A `a152` owns the fixed target-installer review. This change advertises the accepted PostgreSQL full-mode reader. It does not redesign the reader, and it does not certify full types or CDC.

Source commit `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`, parent `b72d2f8b115eb9882141eac7f3cfaa05c906f3ed`. The Linux runs archived fixed base `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`, and the post-edit probe overlaid only the activation files. That fixed base includes reader `16ca2969cd449653a64d98d9831d3c8b8976942a`. `Cargo.lock` at the fixed base and at the parent is blob `ef30377cade0ad07ba4192125f434b01452a5622`.

Reader R4 is independent limited approval `0791e971f49cd8afc646d80179241ddbcfaa1a1c` (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/`). This activation uses that approval. It does not replace it.

## What is advertised

`connector_capabilities` keeps the same names and order. `scan` and `bounded_read` are `supported=true`. Their reason is `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. `cdc`, `type_conversion`, and `registry_api` stay `supported=false`, reason `not_in_this_slice`. `source_ddl` and `source_write` stay `supported=false`, reason `forbidden`.

`type_capabilities` `bounded_reader` is `supported=true` with the same reason. The other type-capability entries are unchanged. Catalog `type_conversion` stays `unsupported`. `TypeSupport.converted` stays false. The live test still requires every selected column to be unconverted.

`finish` returns `SnapshotFinished` as lifecycle close only. `finish_complete` returns `RunComplete` as full-selection admission. `contracts/source/pgsql/bounded-stream-v1.md` line 9 and the `PlannedChunk` comment say that. The lifecycle opening no longer says the flags stay false. Qualified scope remains: resume cursors, CDC, and the registry stay unsigned, and column conversion stays false.

## Requirement map

| Requirement | Result | Evidence |
| --- | --- | --- |
| Pre-edit Linux 1.94 lib and scoped Clippy on the fixed-base archive | lib `63 passed; 0 failed`, clippy exit 0, `docker-run.exit` 0, elapsed 40s | `linux/smoke/` |
| `scan` and `bounded_read` true, remainder false, names and order kept | Unit tests in the lib run. Live outcome prints both true with the admission reason | `linux/probe/lib.exit` 0, `complete-outcome.txt` |
| `bounded_reader` true with the same reason. No whole-type certification | Outcome line `type_capability name=bounded_reader supported=true`. Selected columns stayed unconverted | `complete-outcome.txt` |
| Public reader: Arrow, `Exhausted`, `RunComplete` | Generation 1: 5 batches, 2 item rows, 3 line rows, `exhausted=1`, `same_cut_match=1`, `RunComplete` digest `82e3f995a7ff539ec41d6ad1f14e6210e17268ac1c887c13cf38b43389bf1fe0` | `complete-outcome.txt` |
| Early refusal | Generations 4–7 return `not_exhausted` with `backends_gone=2` | `complete-outcome.txt` |
| `finish` stays non-admission | Generation 3 `SnapshotFinished`, `admission=0`, `close_only=1` | `complete-outcome.txt` |
| Snapshot, stream, session, SQL, manifest, and lock unchanged | Blobs match the fixed base. Probe hash check `overlay_hashes_match=1` | Blob table below, `linux/probe/fixed-blobs.txt` |
| Ledger and parent SP03 stay open | No spec, ledger, or pipeline edit | This commit's path list |

`linux/probe/pg_reader_complete_live.exit` is 0. The test is `ok. 1 passed; 0 failed; finished in 2.71s`. Container elapsed 10s. `docker-run.exit` is 0.

Generation 2 read the committed cut (`pine`, 4 lines, line 13, scan `2:oak,1:pine`) and minted its own `RunComplete`. Generation 8 cancel returned `cancelled` and did not mint. Generation 9 returned `Budget` `max_value` and did not mint. Reader SQL in the window: 397 statements, `dml_ddl_commit_terminate_offset=0`, verbs `RELEASE=6,ROLLBACK=23,SAVEPOINT=7,SELECT=343,START=18`.

## Blobs

Changed at `bedd6a2c` relative to fixed base `4ed22b8c`:

| Path | Fixed blob | New blob |
| --- | --- | --- |
| `crates/oo-connectors/src/model.rs` | `0d7af7c1b9b7ffc30bbecdc75b934c965c8dbefa` | `5964e40036b76efe561c7441b6acc0f05333265a` |
| `crates/oo-connectors/src/types/mod.rs` | `9920752ceee306796ff5ea4113f5d6109b659555` | `dbb95bcc8980b68d66502300fc13e4bfcc800b72` |
| `crates/oo-connectors/src/types/tests.rs` | `0b80698990fc027457d53b58dac7b7129a568cd8` | `3e98361d79bd6bb1deb8b1700e5820a0f2410d88` |
| `crates/oo-connectors/src/key_plan.rs` | `8f152763844330532df58efdc77568e1f36e3d40` | `6052fe9ff1bc4bdc0a3e0e3b6fd9c717649aea86` |
| `crates/oo-connectors/src/lib.rs` | `9f233d68a474f26c81201e1e6e025c8a950e3a06` | `64a7efae0e442e7babf67a56ba07d398749ef5f0` |
| `crates/oo-connectors/tests/live_pg.rs` | `d475cd734704280e745125fffa8537f9fa584ab7` | `c21d5ee841ec1a98c2810b34df29bc6ccc6b087c` |
| `crates/oo-connectors/tests/lossless_live.rs` | `ccd367158840650c9c5818372cc2e002861e9db9` | `dbbbc79ed8611c983248b28daecbb2ca75f1eb2e` |
| `crates/oo-connectors/tests/pg_reader_complete_live.rs` | `26ace07d5c2cadcd2584b43d981372834fc29050` | `5a50f65c502f414115931218b807bfe88ec87814` |
| `contracts/source/pgsql/bounded-stream-v1.md` | `c0f01b52c4a408a7838b607e29322f318f16372e` | `1226ea542bec16c64fd02b99e92318eb49f1bc58` |
| `contracts/source/pgsql/reader-lifecycle-v1.md` | `bdae1239de6feaf06f73818f54b784dd0c4e1dbb` | `6f4f242f3cc00c2149bde91c2b782a72c0365813` |

Unchanged, and the reason the a149 matrix was reused:

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `10176c16a64f45409063a1236c0651065da8a05d` |
| `crates/oo-connectors/src/stream.rs` | `5deb30e5da09e58904d04cce22760c5ffc284f96` |
| `crates/oo-connectors/src/session.rs` | `7e191eacbf61439680cbd92a7109ded6c8a05596` |
| `crates/oo-connectors/src/sql.rs` | `6badaf187e33346c1657c632e6d5c43fe90f1ea2` |
| `crates/oo-connectors/Cargo.toml` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` |
| `Cargo.lock` | `ef30377cade0ad07ba4192125f434b01452a5622` |

`lib.rs` and `types/tests.rs` are assertion updates. The activation prompt named four all-false sites (`model.rs`, `live_pg.rs`, `lossless_live.rs`, `pg_reader_complete_live.rs`). `--lib` also executes the same all-false check in `lib.rs` `driver_verify_full_options_link` and `types/tests.rs` `converted_flag_does_not_authorize_or_block_a_builtin`. Those two checks were updated to the same supported names and unsupported remainder so the required library run can pass. No other lines in those tests were rewritten. `types/tests.rs` also asserts `bounded_reader` is supported with the admission reason.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `HOME=/tmp`. The image has no Clippy component. Each container ran `rustup component add clippy --toolchain 1.94.0` into its writable layer. Smoke and the green probe record `clippy-component.exit` 0. Smoke ran `cargo fetch --locked` (`fetch.exit` 0). The probe reused the private cargo and target volumes with `--offline`.

Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `pg/init.stdout` records `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. `pg/init.exit` 0. `pg/up.exit` 0. Database `oo_a151`. Roles `oo_a151_reader` and `oo_a151_writer`. TLS verify-full. `pg/server-san.txt`: `CN=oo-a151-pg`, SAN `DNS:oo-a151-pg`. `pg/pg-ports.json` is `{}`.

Container `oo-a151-pg` id `40d07c9b4d287b7f02c88d2e540b5150d659c5d8b13195ee7023d5506a3880a5`, privileged false, network `oo-a151-eef831aa-net`, memory 536870912, `CapDrop=ALL`, `CapAdd` `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Private volumes `oo-a151-eef831aa-cargo`, `oo-a151-eef831aa-target`, `oo-a151-eef831aa-pgdata`, `oo-a151-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. Passwords and TLS private keys stayed under `/tmp/oo-a151-eef831aa-dba4-4af2-a78b-541bc9e0a151/secrets`. A byte scan of this evidence tree against those three passwords and against private-key markers found zero hits. `linux/probe/postgresql.log` is 251888 bytes.

Smoke `oo-a151-smoke` id `2a75acb6942092fa2411ec7037d3303924f11e22c1c29be591ed63b1282f63b3`. Probe-1 `oo-a151-probe` id `deb98dde19cc1dd2ccb57bca6690b07d047ebdaa274f2367091ae7022eddb995`. Green probe `oo-a151-probe` id `1fe07557f128da806d717e04862dd697f2582f85e866760956a2ffd4627a36e5`. Each rust container: `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, private network, privileged false, `OOMKilled=false`. Capacity before smoke and before the green probe: `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. One rust builder at a time. Postgres stayed up beside it.

## What ran

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a151/SP03/reader-activation/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a151/SP03/reader-activation/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a151/SP03/reader-activation/linux/probe/launch.sh
```

The probe launch was invoked twice. The first is preserved as `linux/probe-1`. Its `docker-run.exit` is 101 and `clippy.exit` is 101. `lib.exit` is 0 (`63 passed`). Clippy rejected `lossless_live.rs` with E0716 because two `Vec<&str>` collections borrowed temporaries from `connector_capabilities()`. That file was corrected to bind the vector. Production behavior was not rewritten. The second launch is `linux/probe`.

Smoke, archive only, before any product edit:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Green probe, archive plus overlay:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1 --nocapture
```

Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail the package lint.

`cargo test -p oo-connectors --all-targets` was not executed as one blanket command. That command starts every live binary. Those binaries require their own fixtures. The prompt says to reuse the unchanged R4 evidence and not to rerun the broad matrix. Clippy `--all-targets` typechecked those binaries. The executed tests are the library suite and `pg_reader_complete_live` once.

## Left unsigned

SP03-W03 stays open. The ledger was not edited. Full types, per-column `converted=true`, CDC, incremental mode, other databases, the registry API, source DDL, source writes, release, and supply approval stay unsigned. No production loader admits `RunComplete`. `crates/oo-connectors/README.md`, `contracts/source/pgsql/README.md`, `reader-keys-v1.md`, and `type-conversion-v1.md` still contain the older unsupported wording. They were outside the write list. The named cleanup fact `cleanup_unconfirmed` was not provoked again. a149 already recorded the killed-backend result as `Protocol` `database`.
