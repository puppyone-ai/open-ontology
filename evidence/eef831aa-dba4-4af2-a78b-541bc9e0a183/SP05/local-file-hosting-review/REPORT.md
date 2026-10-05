# REPORT — a183 fixed local file hosting review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a183`. Slot C. One Rust builder, no PostgreSQL.

**Verdict: APPROVE_LIMITED.**

The approval covers the local upload primitive on `object_store` `LocalFileSystem` as `contracts/control/local-file-hosting-v1.md` describes it: sequential multipart bytes, checksum, idempotent resume and complete, two-context refusal of a known key, no visible object after a bad complete, and abort that deletes only the session's owned keys. It does not cover the plan's project-quota guarantee, a process memory bound, S3, the HTTP backend, grant authentication, the reaper, or the parent upload state machine after `stored`.

No product file was edited. The ledger was not changed. This is not a release claim.

## Source

| Item | Value |
|---|---|
| Fixed commit | `755fb84acc7f80e5ba2fba1c2b80412dab7fab26` |
| Tree | `9bb6da0a68e8533831f3d92fe105caf1423e2c0c` |
| Parent | `72b5136ba23d134916425132523383947046aeff` |
| Diff | 6 files, 2638 insertions, 2 deletions (`diff/parent.diff`) |
| Workspace HEAD at review time | `753423b75e630da2a95a923659ae171a158236c8` |

The smoke archived `755fb84`, not HEAD. `control/archive-match.txt` says `archive_hashes_match=1`. `control/archive-tree.txt` is the tree above. After the oracle, `oracle/product-files-match.txt` says `product_files_match=1` for the same six blobs.

## Control smoke, before the oracle overlay

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Container `oo-a183-review-control`: privileged false, CapDrop ALL, no-new-privileges, network `oo-a183-eef831aa-net` (`internal=false`), no published ports, memory and swap 8 GiB, `/tmp` tmpfs. `HOME=/tmp`. Private volumes `oo-a183-eef831aa-cargo` and `oo-a183-eef831aa-target`. No PostgreSQL.

`control/docker-run.exit` is 0. Phase exits are 0 for rustfmt component, clippy component, `cargo fmt --check`, fetch, lock unchanged, tree, test, clippy, arch, lock-after, and worst. Host lock delta exit is 0. `cargo test --locked --offline -p oo-control --test file_hosting --lib` reported 0 library tests and 6 integration tests passed (`limits_and_names`, `conflicting_part`, `checksum_and_size`, `abort_releases_quota`, `multipart_reopen`, `two_contexts`). Clippy `--all-targets -D warnings` finished clean. `oo-arch-check` printed `architecture check: pass`.

## Oracle

Same image, toolchain, network, and volumes. Container `oo-a183-review-oracle`. `/tmp` tmpfs was 2 GiB. The snap was a fresh `git archive` of `755fb84` plus `oracle/review_oracle.rs` and `oracle/review_memory.rs`. Those two files were copied only into the snap.

`oracle/docker-run.exit` is 0. `oracle/inspect.txt`: `ExitCode=0`, `OOMKilled=false`. Phase exits are 0 for clippy component, features tree, `review_memory`, `review_oracle`, clippy, lock-after, and worst. Clippy was downloaded into this container (`oracle/console.log` line 16); cargo itself stayed `--locked --offline`. `Cargo.lock` was unchanged (`oracle/phases/lock-after.exit` is 0).

`review_memory`: 1 passed in 1.28s. `review_oracle`: 6 passed in 0.02s.

| Check | Fresh result |
|---|---|
| Resume, out-of-order parts, drop/reopen, second complete | `resume_lineage_ok session=1dc4417eb7cd6073c262826930a1233d object_sha256=94113e2d754471c27cf4d8a7b2a0e768dbd88f72118bbb9b570a5f416914a218` |
| Independent idempotency and lineage | asserted inside the passing test, domains in `review_oracle.rs` |
| Same grant string on another project and another principal | `Denied`, display `admission context does not match this upload` |
| Same project and principal, different grant | reads; manifest keeps the first grant |
| Wrong digest | `NotFound` for the owner and the other context |
| Checksum miss and size miss | no object key, no part, no session; `staging/.../part/notes` kept |
| Abort | removed only the four owned keys in `oracle-notes.txt`; foreign notes and non-hex quota name kept; second abort `NotFound`; completed abort `Closed` |
| Concurrent complete, one pair | `oks=2 errs=` empty; follow-up complete, stored bytes, and lineage matched |
| Sequential quota | second 8-byte `begin` under a 10-byte ceiling returned `Quota` |
| Concurrent quota, one pair | `oks=2 quota_errors=0 reserved=16 limit=10` |
| `complete` memory | stream chunk 8192; VmHWM delta 16801792 bytes on an 8388608-byte part |

Feature edges from `oracle/features-tree.txt`: `object_store` feature `fs`; dev `tokio` features `macros` and `rt-multi-thread`. The `http` feature is absent. `control/cargo-tree.txt` still lists `http v1.5.0` as a normal dependency of `object_store`. That tree does not list `windows-sys`, `reqwest`, `aws-lc-rs`, `hyper`, or `quick-xml`. The lock still carries `windows-sys` 0.61.2 for the `fs` feature's non-Linux targets.

## Dependency pins

Against parent `72b5136b`, the lock adds five package versions, changes no existing checksum, and removes nothing. `control/lock-delta-errors.txt` is empty. `itertools` 0.15.0 sits beside 0.10.5 and 0.14.0. Lock checksum, downloaded `.crate` checksum, and the crates.io API checksum are the same value for each row. None of the five versions is yanked. Highest added MSRV is 1.85. Product `rust-version` stays 1.94.0. `cargo deny` was not run.

| Crate | Version | License | MSRV | Published | Checksum |
|---|---|---|---|---|---|
| object_store | 0.14.2 | MIT/Apache-2.0 | 1.85 | 2026-09-15T15:15:46.622836Z | `f1796bc93603f78c5760a69f2d58badc9618d22adade0a95385bb2adbae4eb94` |
| nix | 0.31.3 | MIT | 1.69 | 2026-05-11T07:01:18.650221Z | `cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d` |
| humantime | 2.4.0 | MIT OR Apache-2.0 | 1.60 | 2026-07-02T11:38:57.566374Z | `15cdd26707701c53297e2fa6afb323d55fbc1d0810c3aec078ae3ef0424c3c15` |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | 1.71 | 2026-08-11T12:13:14.842510Z | `9fb9654ba8355388abeb8dcb4fc62f511300867002afc858860463bdd9fe0c44` |
| itertools | 0.15.0 | MIT OR Apache-2.0 | 1.63.0 | 2026-06-16T15:09:28.880416Z | `8b4baf93f58d4425749ca49a51c50ebab072c5df6994d08fed93541c331481dc` |

Direct pins already named by the crate stay `sha2` 0.10.9, `getrandom` 0.2.17, `futures-util` 0.3.34, and dev `tokio` 1.53.1. `oo-core` and `oo-jobs` stay path dependencies. The architecture edges `oo-control` to `oo-core` and `oo-control` to `oo-jobs` were already required; this commit did not edit `policy.rs`.

## a181 evidence inventory

`evidence/...a181/SP05/local-file-hosting/SHA256SUMS` has 6 data lines. Each line is a git blob, a sha256, and a path for one file in commit `755fb84`. This review re-hashed those six blobs: `author-sha256sums-gap.txt` records `source_hash_mismatches=0`. The a181 directory contains 295 files. That SHA256SUMS is a source-commit list. It is not a manifest of the reports, logs, or failed attempts. The coordinator match of those six hashes is a source match. It is not a full-inventory match.

`SHA256SUMS` in this directory lists every file under this review directory except itself.

## Host post-step

`oracle/launch.sh` exited 1 after the container had already exited 0. The post-step passed each whole `source/blobs.txt` line to `shasum` as a path. `oracle/host-post-exit.txt` records that. The product comparison was recomputed on the surviving snap and matched all six files. The container log was not overwritten.

## Still open

SP05-W01 and SP05-W06 stay open. REQ-SRC-05 stays open after `stored`. AT-FILE-01 and AT-FILE-05 are not closed by this local fixture. Still open, as the contract already lists: an S3 or MinIO backend and the 5 MiB part minimum, reaping `dest#N` leftovers, download, TTL, an auth proxy, HTTP, MIME sniffing, formula-safe output, retention, presigned URLs, a linearizable or multi-instance quota, and checking grant evidence instead of storing it. `docs/17-rust-implementation.md` still requires streaming large objects. The measured `complete` path collects each whole part.
