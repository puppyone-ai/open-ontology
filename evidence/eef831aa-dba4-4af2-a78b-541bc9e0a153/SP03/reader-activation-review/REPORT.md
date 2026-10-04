# Report — a153 reader activation review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a153`. Slot B. Sole independent review. No subagents and no worktree. Slot A `a152` owns the resumed target-installer review.

Verdict: **APPROVE_LIMITED**. Fixed source `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`, parent `b72d2f8b115eb9882141eac7f3cfaa05c906f3ed`. The public advertisement of `scan`, `bounded_read`, and `bounded_reader` matches the PostgreSQL full-mode reader API that a149 already approved. The reason on all three is `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`. `finish` returns `SnapshotFinished` as lifecycle close only. `finish_complete` returns `RunComplete` as full-selection admission.

This approval does not check SP03-W03. It does not edit the ledger. It does not certify full types, per-column `converted=true`, CDC, incremental mode, the registry API, source DDL, source writes, other databases, a production loader, release, or supply approval. Author evidence `057e30dbb16857d6af889a2a5c23dde49dcde8a6` (tree `8b036996c284c5ce41ddc28ae021ca7ee5e3292c`) is the change record. It is not this acceptance.

## Fixed source

Both Linux launches archived `bedd6a2c`. `linux/smoke/archive-tree.txt` and `linux/probe/archive-tree.txt` both record tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`. Workspace HEAD during both launches was `ac4b7272d9ffd0dc22a1fd6978f1780b51245efa`, the a152 evidence commit. The archive command was `git archive bedd6a2c`. It did not follow HEAD. No product overlay was applied. Smoke `archive_hashes_match=1`. Probe `archive_hashes_match=1`.

Reader production API `16ca2969cd449653a64d98d9831d3c8b8976942a` is an ancestor. Independent R4 approval is `0791e971f49cd8afc646d80179241ddbcfaa1a1c` (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review/`). Snapshot, stream, session, and SQL bytes are that approval's bytes. This review reused that evidence and did not rerun the security, fault, or dual-architecture matrix.

| Path | Git blob at `bedd6a2c` | Content sha256 | Same as a149 `16ca2969` |
| --- | --- | --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `10176c16a64f45409063a1236c0651065da8a05d` | `43c76114412533600ef554450b50d1f598f04ec674bbffd0611295272dd23227` | yes |
| `crates/oo-connectors/src/stream.rs` | `5deb30e5da09e58904d04cce22760c5ffc284f96` | `7f211507126dea33fd1c20a9b38b29e2e70796437312b89caf23c504d390410f` | yes |
| `crates/oo-connectors/src/session.rs` | `7e191eacbf61439680cbd92a7109ded6c8a05596` | `6933bf4e855682e6998b89c8c756f8db6239e28e4bc9a249fd34af98c088f16f` | yes |
| `crates/oo-connectors/src/sql.rs` | `6badaf187e33346c1657c632e6d5c43fe90f1ea2` | `8c23e0fa557e59399e9f8f43f3f3ca5ae0230f027ad24b8ce67dccd958d274ba` | yes |
| `crates/oo-connectors/Cargo.toml` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` | `347f21f56657900c99939d505b72d1e60e8c5a2b7c60177e0d6ff8e009f3f2cf` | yes |
| `Cargo.lock` | `ef30377cade0ad07ba4192125f434b01452a5622` | `8e34d58dae260aeef2969b2094312b1336bb2883f70fb9d7789b1cdfe31d3a9b` | same as parent `b72d2f8b`; a149's lock was `15ae1071c7236a956547e3f1d37d627b2cbebfce` |

`Cargo.lock` is unchanged by this commit. Ancestors after `16ca2969` changed it. The smoke and the live test compiled `--locked` against the blob at `bedd6a2c`.

Changed at `bedd6a2c` relative to parent `b72d2f8b`, ten files, 188 insertions and 38 deletions. The full diff is `diff/parent.diff`.

| Path | Parent blob | Fixed blob |
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

## What the advertisement says

`connector_capabilities` in `model.rs` lines 733–752 keeps the names and order `scan`, `bounded_read`, `cdc`, `type_conversion`, `registry_api`, `source_ddl`, `source_write`. `scan` and `bounded_read` are `supported=true` with the admission reason above. The other five stay false: `cdc`, `type_conversion`, and `registry_api` use `not_in_this_slice`; `source_ddl` and `source_write` use `forbidden`.

`type_capabilities` in `types/mod.rs` lines 94–124 keeps the same names and order. `bounded_reader` at lines 110–114 is `supported=true` with the same reason. `mysql_unsigned`, `position_codec`, `cdc`, `loader_reject_persistence`, and `user_defined_output` stay false. The already-supported decode entries from `pg_builtin_lossless` through `pg_arrow_crate` stay as they were. That list is not a whole-type certificate.

`TypeSupport.converted` is not assigned in this diff. The live test's `catalog_unconverted` (`pg_reader_complete_live.rs` lines 824–830) requires every selected column's `type_support.converted` to be false. The test passed, so the columns read in this run were unconverted.

## Documentation that this commit aligned

`bounded-stream-v1.md` line 9, copied into `linux/probe/bounded-stream-line9.txt`: `finish` returns `SnapshotFinished` as lifecycle close only, and `finish_complete` returns `RunComplete` as full-selection admission. `TableEof` and `Exhausted` keep their previous meanings. The opening paragraph qualifies acceptance to the full-mode snapshot, the bounded pull, and `RunComplete` admission, and leaves resume cursors, CDC, the registry, and `type_support.converted` unsigned.

`reader-lifecycle-v1.md` line 3 removes the sentence that `scan`, `bounded_read`, and `bounded_reader` stay false. It states the same qualified acceptance. `SnapshotFinished` is lifecycle close only and is not admission. `finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table. Line 15 and line 44 say the same thing about admission. Line 13 still calls a drained portal EOF's `SnapshotFinished` "lifecycle release for compatibility" and says that value does not require `Exhausted`. That sentence agrees with close-only.

`key_plan.rs` lines 67–69: `finish` / `SnapshotFinished` is lifecycle close only, and `finish_complete` / `RunComplete` is full-selection admission.

## Path boundary

The a151 write card named `model.rs`, `types/mod.rs`, the `key_plan.rs` comment, the three live-test assertion sites, and the two contract files. `lib.rs` and `types/tests.rs` are outside that card. The coordinator accepts those two for this fixed review. The exact diff is `diff/assertion-boundary.diff`.

`lib.rs` `mod tests` starts at line 80. The only hunk is new lines 111–132, inside `driver_verify_full_options_link` (line 86). Those lines replace `all(|item| !item.supported)` with the supported names `scan`, `bounded_read` and the unsupported remainder. No production item in `lib.rs` changed.

`types/tests.rs` lines 121–148, inside `converted_flag_does_not_authorize_or_block_a_builtin` (line 117), are the same name split plus a `bounded_reader` reason check. Lines 149–154, the existing `pg_builtin_lossless` and `pg_arrow_crate` assertions, stay. No other test in that file was rewritten.

`live_pg.rs`, `lossless_live.rs`, and `pg_reader_complete_live.rs` `note_capabilities` (lines 118–168) are assertion updates on the write card. `lossless_live.rs` binds `let caps = connector_capabilities()` so the collected `&str` values do not borrow a temporary. That binding is the assertion compile fix. This review's clippy of the committed file exited 0.

## Requirement map

| Requirement | Fixed source | Evidence | Result |
| --- | --- | --- | --- |
| Linux 1.94 connector lib and scoped clippy on the fixed archive | `git archive` of `bedd6a2c` | `linux/smoke/console.log`: lib `63 passed; 0 failed`, `lib.exit` 0, `clippy.exit` 0, `docker-run.exit` 0, elapsed 41s | Held |
| `scan` and `bounded_read` true, remainder false, names and order kept | `model.rs` lines 736–743 | Unit assertions in the lib run. Live outcome prints both true with the admission reason | Held |
| `bounded_reader` true with the same reason. No whole-type certification | `types/mod.rs` lines 110–114 | Outcome `type_capability name=bounded_reader supported=true` | Held |
| Per-column `converted` stays false | Unchanged snapshot and stream blobs. Test lines 226–228 and 278 | `catalog_unconverted` passed inside the one live test | Held |
| Public capability, then the reviewed entry, actual Arrow, `Exhausted`, `RunComplete` | Existing `pg_reader_complete_live` on `SnapshotOwner` | Generation 1: 5 Arrow batches, 2 item rows, 3 line rows, `exhausted=1`, `same_cut_match=1`, `RunComplete` schema hash `b76a05b20baf5847f2acd640290d33b6765c8d4ce2099e1d0cec0f328b3616db` | Held. See the digest note |
| `finish` is close only | `SnapshotOwner::finish` | Generation 3 `SnapshotFinished`, `admission=0`, `close_only=1` | Held |
| Snapshot, stream, session unchanged | Blobs in the table above | Smoke and probe `fixed-blobs.txt`, `archive_hashes_match=1` | Held. a149 evidence reused |
| Docs for close versus admission | bounded-stream line 9, lifecycle lines 3, 15, and 44, key plan lines 67–69 | `bounded-stream-line9.txt` matches the source | Held for the files this commit edited |

`linux/probe/pg_reader_complete_live.exit` is 0. The test is `ok. 1 passed; 0 failed; finished in 2.76s`. Container elapsed 7s. `docker-run.exit` is 0. Outcome file ends `complete_live_ok`.

The one test calls public `connector_capabilities()` and `type_capabilities()` before it opens `SnapshotOwner`. `drain` (`pg_reader_complete_live.rs` lines 761–809) reads `batch.record_batch` through `Int32Array` and `StringArray`, compares the rows to the pre-writer cut by key, and requires `StreamItem::Exhausted`. `finish_complete` then returns `RunComplete`. That is the advertisement and the existing entry API in one run. `live_pg` and `lossless_live` were not executed. Clippy `--all-targets` typechecked them.

Digest note. Generation 1 digest on this fixture is `b2b301ead04b1cc970f7bdc4b45ffc8f4fd6285ca1440472f83e8908afb2006c`. The author's a151 run recorded `82e3f995a7ff539ec41d6ad1f14e6210e17268ac1c887c13cf38b43389bf1fe0`. Schema hash matches that run. `snapshot_input_digest` folds `config_identity`, and `config_identity` includes host, database, and username (`snapshot.rs` lines 371–377). This fixture is `oo-a153-pg`, database `oo_a153`, role `oo_a153_reader`. The author fixture used the a151 names. The row cut matched by key: items `willow` then a fresh generation `pine`, 4 lines, line 13, scan `2:oak,1:pine`.

The same existing test also printed the arms it already contains. Generations 4–7 returned `not_exhausted` with `backends_gone=2`. Generation 8 returned `cancelled`. Generation 9 returned `Budget` `max_value`. None minted `RunComplete`. Reader SQL in the window: 397 statements, `dml_ddl_commit_terminate_offset=0`, verbs `RELEASE=6,ROLLBACK=23,SAVEPOINT=7,SELECT=343,START=18`. Those arms are the author test's body. This review did not add a fault campaign.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `HOME=/tmp`. The image has no Clippy component. The smoke container ran `rustup component add clippy --toolchain 1.94.0` into its writable layer. `clippy-component.exit` 0. Smoke ran `cargo fetch --locked` (`fetch.exit` 0). The probe reused the private cargo and target volumes with `--offline`.

Postgres image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `pg/init.stdout` records `init_ok version=17.10 (Debian 17.10-1.pgdg12+1)`. `pg/init.exit` 0. `pg/up.exit` 0. Database `oo_a153`. Roles `oo_a153_reader` and `oo_a153_writer`. TLS verify-full. `pg/server-san.txt`: `CN=oo-a153-pg`, SAN `DNS:oo-a153-pg`. `pg/pg-ports.json` is `{}`. `pg/init.stderr` is empty. `pg/init-stderr-redacted.txt` is `redacted=0`.

Container `oo-a153-pg` id `7f21d568891e46565b0ae6530e4216d2e51eef8072eaaa7d7665438342f35b71`, privileged false, network `oo-a153-eef831aa-net`, memory 536870912, `CapDrop=ALL`, `CapAdd` `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Private volumes `oo-a153-eef831aa-cargo`, `oo-a153-eef831aa-target`, `oo-a153-eef831aa-pgdata`, `oo-a153-eef831aa-pglog`. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. Passwords and TLS private keys stayed under `/tmp/oo-a153-eef831aa-dba4-4af2-a78b-541bc9e0a153/secrets`. A byte scan of this evidence tree against those three passwords and against private-key markers found zero hits. `linux/probe/postgresql.log` is 251888 bytes.

Smoke `oo-a153-smoke` id `97ecb89857129ffc225f010388d4b90b9d93b3caa58578a59721ad285f10ff3c`. Probe `oo-a153-probe` id `37d10d0c33043d1c289302741c7dd9ed328c81c72d5d3b12c415fc301ceef0b4`. Each rust container: `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, private network, privileged false, `OOMKilled=false`. Capacity before smoke and before the probe: `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. One rust builder at a time. Postgres stayed up beside it. The probe launch removed the exited smoke container after its logs were on disk.

Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations during clippy. They did not fail the package lint. `clippy.exit` is 0.

## What ran

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a153/SP03/reader-activation-review/pg/up.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a153/SP03/reader-activation-review/linux/smoke/launch.sh
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a153/SP03/reader-activation-review/linux/probe/launch.sh
```

Smoke, archive only:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

Probe, the same archive, one existing test:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1 --nocapture
```

`OO_PG_COMPLETE_FIXTURE=/secrets/pointer.json`. `OO_PG_COMPLETE_OUTCOME=/evidence/complete-outcome.txt`. The fixture env was set inside the container script. Passwords were not process arguments.

No production, manifest, lock, spec, or ledger file was edited.

## Left unsigned

SP03-W03 stays open. The ledger was not edited. Full types, per-column `converted=true`, CDC, incremental mode, other databases, the registry API, source DDL, source writes, release, and supply approval stay unsigned. No production loader admits `RunComplete`.

These readonly files still describe the flags as unsupported. They were outside the write list. The stale sentences are a documentation follow-up. They are not a functional failure of the API this review executed:

- `crates/oo-connectors/README.md` line 11 says `bounded_read` stays unsupported and R4 is unsigned.
- `crates/oo-connectors/README.md` line 17 says `scan`, `bounded_read`, `cdc`, `type_conversion`, and `registry_api` are unsupported.
- `contracts/source/pgsql/README.md` line 3 says this slice does not implement scan or bounded read.
- `contracts/source/pgsql/README.md` line 47 lists `scan` and `bounded_read` as `not_in_this_slice`.
- `contracts/source/pgsql/reader-keys-v1.md` line 3 says `connector_capabilities` and `bounded_reader` stay unchanged.
- `contracts/source/pgsql/reader-keys-v1.md` line 37 says `SnapshotFinished` is still the only successful close.
- `contracts/source/pgsql/type-conversion-v1.md` line 3 says `connector_capabilities` stays unsupported, including `type_conversion`. The `type_conversion` entry is still false. The blanket sentence is older than this advertisement.
- `contracts/source/pgsql/type-conversion-v1.md` line 53 says the bounded reader stays unsupported.
- `contracts/source/pgsql/snapshot-owner-v1.md` line 3 says R4 is unsigned and `scan` and `bounded_read` stay unsupported.

`SHA256SUMS` is the hash of every other file in this directory. It does not hash itself.
