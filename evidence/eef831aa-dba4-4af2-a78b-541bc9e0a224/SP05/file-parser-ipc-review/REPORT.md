# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a224`. Reviewed source `6bd2455849da2527973c136968d4549ecd98d770`, tree `99efda0344ffff4b0b52851c31e495daf2e1cdc5`, parent `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a207/SP05/file-parser-worker`. Date of this review: 2026-10-06.

The approval covers the child `oo-file-parser-worker` at that commit for F-D01: one CSV, TSV, or JSONL file, framed Arrow IPC 60, decoded in this review with `arrow_ipc::reader::StreamReader`. It covers the bounded request, the IPC-sink ceiling, completion after a clean hash, a late hash mismatch with no completion frame, unknown-profile and path-class refusal, and parent kill plus reap of a blocked read. Import, publication, the parent exporter, the loader, and a public endpoint stay open. SP05-W02, AT-FILE-02, and the phase ledger stay unchecked. The delimited reader approval is a199. The JSONL reader approval is a185. This review cites those approvals. It does not re-close them.

No product file, manifest, lock, ledger, or supply-chain policy was edited. a207 remains the connector-manifest and root-lock writer. This directory is the review archive.

## What was read

- a207 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`
- `source/blobs/file-parser-worker-v1.md`, `oo-file-parser-worker.rs`, `file_parser_worker.rs`, `oo-connectors-Cargo.toml`, `Cargo.lock`, `delimited.rs`, `jsonl.rs`, and `lib.rs`
- `locks/parent.lock` from parent `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`
- a199 `tools/sha256_manifest.rs` and the a204 launch and report shape
- a200 smoke and proof launch scripts, recorded and not copied

`recipes/ORIGIN.txt` lists the origin commit, path, and SHA-256. The preoverlay container rechecked `recipes/origin.list` with the Safe Rust hasher (`origin.exit` 0).

The executed tree is a `git archive` of `6bd24558`. Repo HEAD at the preoverlay launch was `865fb571c8bd662e48244f66233ee56adf08c07f`. Repo HEAD at the overlay-4 launch was `6b704d64b9a0d716a3195186889d70473f4a80d6`. At the time of this report HEAD is `18cbdf3e15351ff0c584eb79663052dca69a504d`. `git diff 6bd24558` is empty for the worker, the author test, the contract, the connector manifest, `Cargo.lock`, `delimited.rs`, and `jsonl.rs`. The worktree `lib.rs` differs by the parent export, and `file_worker_parent.rs` is present in the worktree. Both overlay archives recorded `parent-absent.exit` 0. The process that was tested is the sealed archive.

## Pre-overlay smoke

`linux/preoverlay` ran before the review harness was copied into an archive. `blob-cmp.exit` 0. `overlay-absent.exit` 0. `parent-absent.exit` 0. `hash-compare.exit` 0. `origin.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, Debian 13, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `7343c8e6a86d3c79afbb304cffd5c1e8c8884502caefcd91a86fe68c637d27a2`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a224-eef831aa-net`. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`.

`fetch-offline.exit` 101 is the cold private cargo volume. The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `test.exit` 0 is the author `file_parser_worker` suite: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s`. `clippy-component.exit` 0. `clippy.exit` 0 on `--bin oo-file-parser-worker --test file_parser_worker -- -D warnings`, ending `Finished dev profile [unoptimized + debuginfo] target(s) in 28.37s`. `supply-curl.exit` 0. `supply-rustc.exit` 1. `supply.exit` is the text `not_run`. `docker-run.exit` 1. The two `sqlx-postgres` `unfulfilled_lint_expectations` warnings are the locked vendor. Clippy still exited 0.

## Supply audit rerun

`linux/supply` compiled the lifetime-fixed `tools/supply_audit.rs` on the warm cargo volume and re-fetched the four index and API documents inside the container. `toolchain.exit` 0. `blob-cmp.exit` 0. `supply-rustc.exit` 0. `supply-curl.exit` 0. `supply.exit` 0. `docker-run.exit` 0. Container `3ea1d1ae0987238c0a615e5009a78c709d45c54a62641d52c582e1a8b59777ce`. Same memory, CPU, cap drop, privilege, and network settings. `OOMKilled=false`. Clock file `linux/supply/supply/clock.txt` is `2026-10-05T21:49:34Z`.

The auditor compared `Cargo.lock` at `6bd24558` with `locks/parent.lock`. Every parent registry row is still present with the same checksum. No checksum moved. The addition set is exactly these four rows. Each `.crate` SHA-256 matches the lock, the sparse index `cksum`, and the crates.io API checksum. Each extracted `Cargo.toml` says `Apache-2.0`. Each `rust-version` is at most `1.94.0`. Index and API `yanked` are false. Age is at least 7 days at that clock.

| Package | Version | SHA-256 | MSRV | Published | Age |
|---|---|---|---|---|---|
| arrow-cmp | 60.0.0 | `73a0e9adb28d95739fabcdae637261a2c14e618caf58ba1b88857120f9ee2687` | 1.88 | 2026-09-15T15:30:07.395040Z | 20 days |
| arrow-ipc | 60.0.0 | `86c523472d22e31f984df7a44fc36e6323d3c9b05ffe53d267ee4c3aa7c128d2` | 1.88 | 2026-09-15T15:30:28.788489Z | 20 days |
| arrow-select | 60.0.0 | `ba3482b7d03bf4a3bf0b195f08a7f2b071595ccd54897e35b975ac63d3104977` | 1.88 | 2026-09-15T15:30:11.676477Z | 20 days |
| flatbuffers | 25.12.19 | `35f6839d7b3b98adde531effaf34f0c2badc6f4735d26fe74709d8e513a96ef3` | 1.51 | 2025-12-19T23:16:16.576273Z | 289 days |

`linux/supply/supply.stdout` ends `supply_audit=ok`. The dependency baseline and the supply-chain policy were not edited. These rows are not admitted by this review.

## Decisive integration run

`linux/overlay-4` copied `overlay/file_parser_worker_a224_review.rs` onto a fresh archive of `6bd24558` as `crates/oo-connectors/tests/file_parser_worker_a224_review.rs`. The product blobs match `source/blobs`. `blob-cmp.exit` 0. `parent-absent.exit` 0. `fetch.exit` 0 with `cargo fetch --locked --offline`. `clippy-component.exit` 0. `test.exit` 0. `hash-compare.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s`.

Container `ed26c6696c84d82da500f19808c29f8323c08ff8c33fadf6ac113bd8ae0c6edf`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `Privileged=false`. `OOMKilled=false`. Network `oo-a224-eef831aa-net`. `ReadonlyRootfs=false`. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. `/src` was read-only. Capacity at launch was `own=0 global=0 host_cargo=0`.

The harness spawns `CARGO_BIN_EXE_oo-file-parser-worker` with an empty environment. It reads length-prefixed frames and decodes each IPC payload with `StreamReader`. Profile and schema digests are SHA-256 of preimage text in the harness. The object hash is the harness SHA-256, which self-checks the empty digest and `abc`. `sha256sum` of the eight saved fixtures equals those Rust digests (`linux/overlay-4/hash-compare.txt`). The harness does not link `sha2` and does not call the library `profile_preimage`. `OO_A224_ORACLE_DIR` receives the fixtures. The child environment stays empty.

Clippy is `cargo clippy --locked --offline -p oo-connectors --test file_parser_worker_a224_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.26s` after the same two vendor warnings. `clippy.stdout` is empty. The author `file_parser_worker` suite was not run again in this container.

## Fresh fixtures

| Fixture | Id | Rows | IPC frames | Observed values | Object SHA-256 |
|---|---|---:|---:|---|---|
| CSV | a224-shelf | 3 | 1 | `i64::MIN`, `i64::MAX`, `0`; decimal coefficients `3140`, `3140`, `-10`; `甲`, empty sku, `ab\nc`; quoted empty, null `~`, `x,y` | `cd9c70c60ce4cf93b1f5715862e89e21cb5ff55d424971df80a99953dd808c2c` |
| TSV | a224-tab | 2 | 1 | `雀`, quoted `q\tt`, `a,b`; unquoted empty is null; `-0` stored as `0` | `db3814f9a7869aae9a5b8406e2204543ae8321c1cf75b423a63e6dd9c9f399cd` |
| JSONL | a224-notes | 3 | 3 | `乙` / null label / missing label; `n` is `-1`, `0`, `7`; flags `false`, `true`, `false` | `132ce226d1f4f90db4967e63831fca2c2eb561864c65c6074a25c8ba90cf25af` |
| Empty | a224-blank | 0 | 0 | header absent, `arrow_ipc=false`, one completion frame | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

CSV profile digest `fd2fdfd22fd957605d9e546f516394b449326f8432f2202f2e686e4d4b896c66`. Schema digest `58a1500ad8a77bafbfc5c845cb33c1a3cbcdd37fc06aca1e19c5f4787cdc75b2`. Price type is `Decimal128(8, 3)` and metadata `oo.delimited.scale` is `3`. Data rows are lines 2, 3, and 4. The note on the quoted-newline record is line 5. Lineage field offsets match the raw bytes. Lineage text does not contain `甲`, `3.140`, `3.14`, either int64 extreme, or `x,y`.

JSONL profile digest `3229211b6411fc6d7391b095804d3c48d3971e403112fd4f4d49f6b75c47dd1f`. Schema digest `571bd8459b44f3ad5e4b38e61cd06afc04b1694b6fb0224429f4f3090cd2bdf2`. Null `label` offset is the opening quote of that key. Missing `label` offset is the `{`. Logical metadata `oo.jsonl.logical` is `utf8`.

The completion receipt on the success paths starts `oo-file-parser-complete/v1`. The harness required `admits_import=false`, `published=false`, `executes_native=false`, `kernel_network=unsupported`, `kernel_filesystem=unsupported`, `kernel_memory=unsupported`, `parent_kill=caller`, `parent_reap=caller`, `launcher=not_this_process`, `pg_run_complete=false`, and `resumable=false`. It rejected receipt text containing `PgRunComplete`, `oo.source.pgsql`, `sandbox`, `landlock`, or `seccomp`. Header-only CSV `sku,qty,price,note\n` completes with `rows=0` and the shelf profile, config, and schema digests. Empty JSONL id `a224-blank-jsonl` completes with `rows=0` and `reader_protocol=oo.source.jsonl/v1`.

## Refusals and the blocked read

| Case | Exit | Stderr token | Frames |
|---|---:|---|---|
| Empty file, header exact | 4 | `header_required` | 0 |
| Late hash, last hex digit flipped | 6 | `hash_mismatch` | IPC batch plus lineage, no completion |
| `format` `parquet` on an absent absolute path | 2 | `config` | 0 |
| 1,048,577 request bytes | 2 | `request_too_large` | 0 |
| `relative.csv` | 2 | `path_form` | 0 |
| Directory, symlink, fifo | 2 | `not_regular` | 0 |
| 40 decimal columns, 204 rows, `batch_bytes` 16384 | 8 | `ipc_ceiling` | 0 |
| Stdin held, parent kill at 400 ms | none | signal | no completion |

The late-hash batch contains `丁`. The lineage frame does not. Its `config_identity` is the digest of the preimage that carries the wrong expected hash. Stderr does not contain `丁` or the path.

The parquet refusal stderr does not contain `parquet` or the path. The shelf input was a regular file before the successful decode. A fifo is refused as `not_regular` and is not opened. That is the path class check. It is a trusted local path. This run does not prove a jail or immutability.

The IPC ceiling fixture is the shape above. The child wrote zero frames and exited 8. The cap binds the IPC sink. This review did not measure resident set size. The public reader can allocate the `RecordBatch` before the sink refuses it.

The blocked-read test holds stdin, calls `Child::kill`, and observes `status.code() == None`, no completion frame, and `/proc/<pid>` gone. The drop guard kills and waits, then the test joins the reader threads and drops stdin. `/proc/<pid>` is gone after that too. `deadline_ms` is not what ended the process.

## Preserved attempts

- Host `bash linux/preoverlay/inside.sh` from the repo root exited 127 because the path was relative. No container and no `console.log` were created.
- `linux/preoverlay`: author tests and clippy passed. `supply_audit.rs` failed to compile, `E0106` at `version_window`. `supply-rustc.exit` 1. `supply.exit` is `not_run`. `docker-run.exit` 1. Container `7343c8e6a86d3c79afbb304cffd5c1e8c8884502caefcd91a86fe68c637d27a2`.
- `linux/overlay`: harness `find_field` was missing a lifetime. `test.exit` 101. `clippy.exit` 101. `hash-compare.exit` is `not_run`. `docker-run.exit` 101. Container `4bc7a6b02333405e2f35256d8a7b8a04eaad01e5d84522fb129efd95cf52d526`.
- `linux/overlay-2`: harness stored a field borrowed from a temporary schema. Two `E0716` errors. `test.exit` 101. `clippy.exit` 101. `docker-run.exit` 101. Container `b749080d8fb08e7603c1340826afb602f055ed53947eb0234a15af10b7d9f5c1`.
- `linux/overlay-3`: the same 9 tests passed (`test.stdout` says `finished in 0.42s`). `hash-compare.exit` 0. Clippy rejected `useless_format` on the parquet profile string. `clippy.exit` 101. `docker-run.exit` 101. Container `9543062601a2a16d90b2151951f274f666ed86aaf14353d4c4cde88e58005155`.

Those directories were not overwritten. The product sources in them match the fixed commit. The harness that overlay-4 compiled is `overlay/file_parser_worker_a224_review.rs`, digest `949adda191f49a6d647623bbd79027b6de01d885796a6eb1933910ae68efbfc0` in `linux/overlay-4/source-sha256.txt`.

a207 `build` exit 101 remains the recorded `cargo update -p arrow-ipc` failure. Container `1053c811ff8c`. The lock did not change. This review did not run `cargo update`. a207 `MANIFEST.sha256` was Python `hashlib`. This review's deciding hashes are the Safe Rust tools in `tools/`.

## Limits

Kernel network, filesystem, and memory stay `unsupported` on the receipt. `parent_kill` and `parent_reap` stay `caller`. This review did not certify a sandbox and did not run a hostile-format campaign. PostgreSQL was not started. `pg_run_complete` stays false.

The new builder for every container in this review was 6 GiB and 1 CPU. Existing foreign containers were left at their own limits. No privileged mode, host network, published socket, host home, shared cargo cache, or auth environment was used.

## Sealed blob digests

`sha256sum` inside the overlay-4 container, from `linux/overlay-4/source-sha256.txt`:

```text
de5ad3f3c7bcf9368011aff422a4aa24043794231a49bde5d8679b0cf7acf0e0  crates/oo-connectors/src/bin/oo-file-parser-worker.rs
6a231e716c0baac0cf03360b15433fa209a93f756a110b35ff06432ba91f60bd  crates/oo-connectors/tests/file_parser_worker.rs
949adda191f49a6d647623bbd79027b6de01d885796a6eb1933910ae68efbfc0  crates/oo-connectors/tests/file_parser_worker_a224_review.rs
f544a6bb37c0012613d02c7a335477e8da470fcf0c24780b8d1b8b2dcba54550  contracts/connectors/file-parser-worker-v1.md
e0c0466eb75c3de8acf4210bdc5a2ff5ccb9358a93ce19bb6abeca4964afbfd6  crates/oo-connectors/Cargo.toml
16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a  Cargo.lock
12583452df440852b8979d0dfa966966eebbbc53a04433d1c7bc41aa60cd4587  crates/oo-connectors/src/delimited.rs
55056174f5ac1216c23590471b6ed40e84f2e84de940626bc27f564aeae99764  crates/oo-connectors/src/jsonl.rs
5ca17a403e2e8ee01bf074bc81af8a261ffd639a2aa660610cf0c240aaf0007d  crates/oo-connectors/src/lib.rs
```

The eight product digests match the blobs saved from `6bd24558`. `MANIFEST.sha256` is the std-only Rust manifest of this directory. `linux/manifest/launch.sh` wrote the first copy. The rewrite container then hashed the closed tree, and its extra `cmp` exited 1 because a second walk included `MANIFEST.sha256`. `linux/manifest/REWRITE.txt` records that container. The manifest after this sentence is one later write of the same tool, and that write excludes itself.
