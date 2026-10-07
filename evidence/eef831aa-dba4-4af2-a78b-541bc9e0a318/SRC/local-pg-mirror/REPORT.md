# SRC-D01 local PostgreSQL mirror

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a318`. Public-fixture adapter proof. `customer_complete` is false. The enterprise endpoint was not supplied, so no enterprise connection was opened and no enterprise row was copied. This report does not accept the parent goal and does not mark a release.

Fixed archive `54e8f8ea1def6ef351e8f6a0957c242315283c63`. Proof recorded repo HEAD `3f4a358ab1502215e956cdfe1f02ba899b06a8ca`. Root `Cargo.lock` matches that tree before and after the proof (`root-lock-stable.exit=0` and a host `cmp` against the working tree).

## What was built

`tools/local-pg-mirror` is a standalone Rust 2024 package. It is not a root workspace member. It calls the existing connector and pipeline APIs:

- Discover uses `PgConnector::prepare_snapshot` on one session, then drops that session.
- Copy opens `ImportSession` on the verified snapshot owner. That owner uses the data socket and the catalog-guard socket. The manifest records `source_socket_peak` 2 and `source_socket_fact` `verified_reader_guard` only after that open.
- Rows are stored by the canonical COPY loader. Physical names are the loader's `pt_` / `pc_` / `pn_` names. A later local read uses the secret-free target config and those physical names.
- An engine other than `postgresql` returns `unsupported_engine` before a socket and before a credential read.
- The plan file and the manifest are secret-free. Passwords stay in separate files and are read at runtime. The plan stores CA SHA-256 values.

The passing public proof is `linux/proof-10`.

| Check | Exit |
| --- | --- |
| version (rustc 1.94.0, aarch64-unknown-linux-gnu) | 0 |
| source archive gate | 0 |
| generate-lockfile | 0 |
| lock subset (`lock_subset_ok tool_checksums=251`) | 0 |
| fetch --locked | 0 |
| cargo test --locked --offline | 0 |
| lock stable and root lock stable | 0 |
| clippy --all-targets -D warnings | 0 |
| docker run | 0 |
| tool lock installed into `tools/local-pg-mirror/Cargo.lock` | 0 |

Tests: 7 contract tests and 1 live test, `public_fixture_copy_is_local`, 5.23s. The live test used one PostgreSQL 17.10 container, databases `srcdb` and `mirrordb`, and no published host port (`ports.json` is `{}`). It checked readonly source SQL (`42501`), three typed tables against the hand gold, cancel, stop-after-one-batch, jsonb quarantine, a 1ms deadline, catalog drift, resume refusal, local read after the source role lost `CONNECT`, and replay of a complete manifest with a missing password path.

Tool lock SHA-256: `78117125b0ebcdfbf1363d949428655fb1d7dd7be126567fdc2003a605ad874b`.

## Builder

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` (`--pull never`, linux/arm64). Proof container `oo-a318-proof-10`: 6GiB, 1 CPU, `CapDrop=ALL`, not privileged, not OOM-killed, private network `oo-a318-eef831aa-net`. Capacity at launch: own builders 0, global builders 1, host cargo 0, own postgres 1. The container was removed after the run.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Server certificate SAN `DNS:oo-a318-pg`.

Pre-overlay smoke on the fixed archive exited 0 (`linux/smoke/docker-run.exit`).

## Attempts kept

| Attempt | Result |
| --- | --- |
| 1 | `generate-lockfile` succeeded. Subset exit 1. Fourteen checksums were newer than the fixed locks. No compile. Lock not installed. |
| 2 | Subset exit 0. `read_local` needed `sqlx::Connection` for `begin`. |
| 3 | CLI `path` took `Args` while `skip(1)` yields `Skip<Args>`. |
| 4 | `expect_err` required `Debug` on `CopyOutcome`. |
| 5 | Contract CLI lookup used the wrong `CARGO_BIN_EXE` name. Tamper, generation hash, and closed budgets passed. |
| 6 | Tests passed, including the live fixture. Clippy `collapsible_if` in `copy`. |
| 7 | Tests passed. Clippy `ptr_arg` on the contract helper. |
| 8 | Tests passed. Clippy `too_many_arguments` on `run_copy`. |
| 9 | Live copy state was `refused` after `DROP DATABASE` left the installer roles in the same cluster. |
| 10 | Tests and Clippy passed. |

Attempt 1's missing crates were pinned in `tools/local-pg-mirror/Cargo.toml` to versions already checksummed in the fixed root lock: `cc` 1.4.7, `crypto-common` 0.1.6, `ctutils` 0.4.2, `either` 1.18.0, `find-msvc-tools` 0.1.13, `generic-array` 0.14.9, `js-sys` 0.3.105, `libc` 0.2.189, `mio` 1.2.3, `wasm-bindgen` 0.2.128 and its macro crates, `web-sys` 0.3.105, `yoke-derive` 0.8.2. No root dependency edit.

## Limits

A complete local manifest has `customer_complete` false, `enterprise` `not_configured`, and `consistency` `single_snapshot`. Replay of that manifest does not open a source socket. An incomplete manifest is `not_a_snapshot` and `read_local` refuses it before dialing. This proof does not show that an enterprise database fits one 300s snapshot.

## Inventory

Two direct walks of the evidence bind mount failed in `read_dir` with `ENOENT` (`os error 2`) on `linux/proof-attempt-*`. Those processes wrote no `SHA256SUMS` and the containers were removed. `linux/inventory/launch.sh` is the staging builder: 6GiB, 1 CPU, network none, cap-drop all. It copies the evidence tree and `tools/local-pg-mirror` into the container writable layer, then compiles the unchanged a199 tool. Its stdout stays outside the trees until `console.log` is stored beside that launch. `linux/inventory/final-launch.sh` then rewrites only `SHA256SUMS` at the evidence root and `tools/local-pg-mirror/SHA256SUMS`. Each file lists every regular file under its root except itself. The final builder's stdout and hostconfig stay outside the trees. `customer_complete` remains false.
