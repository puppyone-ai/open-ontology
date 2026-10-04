# Report — a155 reader documentation alignment

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a155`. Slot B. Sole documentation and test executor. No subagents and no worktree. Slot A `a154` owns the real PostgreSQL full import, one pipeline writer, one builder, and one Postgres. This slot used one Rust builder and no Postgres.

Docs commit `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`. Parent `fd83c00c4a3aa42b22e9425bb2919ffb24fd813b`. Fixed archive `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`, tree `10a29baf168403bc849e9fd2078a16f1d1ee0682`. a153 review `b476c7561f7507b8f17128407a257907172b6f9a`. a149 reader API `16ca2969cd449653a64d98d9831d3c8b8976942a`.

The smoke ran on `git archive` of `bedd6a2c` before the documentation edit. Workspace HEAD during that launch was `fd83c00c4a3aa42b22e9425bb2919ffb24fd813b`. The archive command did not follow HEAD. No product overlay was applied. `linux/smoke/console.log` prints `archive_hashes_match=1`.

## Fixed production identity

These blobs are equal at `bedd6a2c`, at HEAD after the docs commit, and in the worktree. `checks/production-blobs.txt` is that comparison.

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `10176c16a64f45409063a1236c0651065da8a05d` |
| `crates/oo-connectors/src/stream.rs` | `5deb30e5da09e58904d04cce22760c5ffc284f96` |
| `crates/oo-connectors/src/session.rs` | `7e191eacbf61439680cbd92a7109ded6c8a05596` |
| `crates/oo-connectors/src/sql.rs` | `6badaf187e33346c1657c632e6d5c43fe90f1ea2` |
| `crates/oo-connectors/src/model.rs` | `5964e40036b76efe561c7441b6acc0f05333265a` |
| `crates/oo-connectors/src/types/mod.rs` | `dbb95bcc8980b68d66502300fc13e4bfcc800b72` |
| `crates/oo-connectors/src/types/tests.rs` | `3e98361d79bd6bb1deb8b1700e5820a0f2410d88` |
| `crates/oo-connectors/src/key_plan.rs` | `6052fe9ff1bc4bdc0a3e0e3b6fd9c717649aea86` |
| `crates/oo-connectors/src/lib.rs` | `64a7efae0e442e7babf67a56ba07d398749ef5f0` |
| `crates/oo-connectors/Cargo.toml` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` |
| `Cargo.lock` | `ef30377cade0ad07ba4192125f434b01452a5622` |
| `contracts/source/pgsql/reader-lifecycle-v1.md` | `6f4f242f3cc00c2149bde91c2b782a72c0365813` |
| `contracts/source/pgsql/bounded-stream-v1.md` | `1226ea542bec16c64fd02b99e92318eb49f1bc58` |

`crates/oo-pipeline/src/lib.rs` is `9c940731d79ff3555dfff717ede6b5e6451040cb` at `bedd6a2c` and at HEAD. `specs/open-ontology-v0.1-spec.md` and `specs/open-ontology-v0.1-plan.md` match `bedd6a2c`. `specs/open-ontology-development-ledger.md` is `1af1f0b155c29c88bb9df18e1641c7eb1f76e0dd` at HEAD and in the worktree. At `bedd6a2c` it was `3793e99b066f6151a7432c003ec90b5d6e29d532`. The difference is the coordinator commits after the fixed archive, including `fd83c00c`. `git show --stat e6bd4c7a` is five documentation files. This task did not edit the ledger, the spec, the plan, the pipeline, a manifest, or a lock.

## Documentation blobs

| Path | Blob at `bedd6a2c` | Blob at `e6bd4c7a` |
| --- | --- | --- |
| `crates/oo-connectors/README.md` | `55d104709504b18d0e3f82b24a591e983c16a1fe` | `1699beed9f814ae5e9b5c8d0dbec25f67c141c2c` |
| `contracts/source/pgsql/README.md` | `88c1947e1685caa036c07c7dc609f9b2b38443f2` | `05337709b73801eaa34f8533a0596f5000274e48` |
| `contracts/source/pgsql/reader-keys-v1.md` | `346833019418700f39e3cc3061d74dda4dbcabd8` | `b7a69643f6374ba5f049c4154729ed73047f45e1` |
| `contracts/source/pgsql/type-conversion-v1.md` | `a86eb4595a3d71743eb2a7fd81e9e48dcfcf20ae` | `f17f7eb041e07e64c6ca55b67cad09a362a3db7e` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `724c692bc138732500dc01d44aa0fe3e0a97296d` | `faaf24c2897eea168ec7f2fa505e23607b3c851d` |

`diff/stat.txt`: 5 files changed, 11 insertions, 11 deletions. The full diff is `diff/docs.diff`.

## Requirement map

| Requirement | Evidence | Result |
| --- | --- | --- |
| Linux 1.94 connector lib and scoped clippy on the fixed archive, before the edit | `linux/smoke/console.log`. Lib `63 passed; 0 failed`. `lib.exit` 0. `clippy.exit` 0. `docker-run.exit` 0. Elapsed 41s. `archive_hashes_match=1` | Held |
| Align the nine stale lines to limited full-mode acceptance | `diff/docs.diff`. `checks/stale-phrases.txt` is `old_phrases_present=0` | Held |
| `finish` / `SnapshotFinished` is lifecycle close. `finish_complete` / `RunComplete` is full-selection admission | Crate README, reader-keys close paragraph, snapshot-owner opening and finish paragraph | Held |
| CDC, incremental mode, registry, source DDL, and source write stay unsupported | Same sentences. Catalog README keeps `not_in_this_slice` and `forbidden` | Held |
| Per-column `converted` stays false. Aggregate `type_conversion` stays false | type-conversion opening and limits. Catalog README line on `type_support.converted` was left in place | Held |
| Key chunks are not full-run admission. A lost snapshot restarts the whole unpublished run with no mixed cut | reader-keys opening and close paragraph. Crate README states the same restart | Held |
| Historical protocol ids and the R1/R2/R3/R4 split stay | Opening headings unchanged. R1 stays the owner. R2, R3, and R4 keep their parenthetical meanings | Held |
| No production loader claim while a154 is the import writer | The five files say the reader token is the prerequisite only | Held |
| Production code unchanged against the fixed input | `checks/production-blobs.txt` | Held |
| Exact diff and links | `diff/docs.diff`. `checks/links.txt` | Held, with one pre-existing miss |

No security, edge, or full matrix ran. No local package rebuild ran after the markdown edit. The smoke is the package check, and it ran on the immutable archive.

## Toolchain and containment

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0 (85eff7c80 2026-01-15)`. `HOME=/tmp`.

Smoke `oo-a155-smoke` id `d613639095c75e655dc2d3f1a059857c21a8d6a47f2f462d6f5ea33d9149c86a`. `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--cpus 4`, `--pids-limit 4096`, network `oo-a155-eef831aa-net` (`afaa884645c7897b9e4bd813b307519c728fb252835b8974a111a30c5da0b6a5`), privileged false, `OOMKilled=false`. Private volumes `oo-a155-eef831aa-cargo` and `oo-a155-eef831aa-target`. The source mount was the archive, read-only. No host network, no docker socket, no host home mount, no shared cargo cache, no auth env, no argv secrets. No Postgres. A scan of `linux/smoke` for private-key markers and `password=` / `PGPASSWORD` found zero hits.

Capacity before launch: `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. One rust builder. Exited `oo-a064-*` containers were left in place.

## What ran

```
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a155/SP03/reader-docs/linux/smoke/launch.sh
```

Smoke, archive only:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

`SHA256SUMS` is the hash of every other file in this directory. It does not hash itself.
