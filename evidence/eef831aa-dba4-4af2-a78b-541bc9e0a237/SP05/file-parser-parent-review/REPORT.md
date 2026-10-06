# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a237`. Reviewed source `bf30b717029d0715e0a1abb7fc8ec02ab7630534`, tree `9e97f9fc2569ef320fcb2a30693c551c6d731c1c`, parent `543580768dec60d127eb428c09e0a9a400b18272`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a220/SP05/file-worker-parent`. Date of this review: 2026-10-06.

The approval covers the file-worker parent at that commit for one caller-admitted local file: CSV, TSV, or JSONL, framed Arrow IPC, lineage checked, clean EOF, then `ParentParseComplete`. `admits_import`, `published`, and `pg_run_complete` stay false. `KERNEL_RESTRICTION` stays `Unsupported`. A `setsid` grandchild leaves the killed group and stays alive until this review kills it. Import, publication, the loader, and a public endpoint stay open. SP05-W02, AT-FILE-02, and the phase ledger stay unchecked. The worker approval is a224 at `6bd2455849da2527973c136968d4549ecd98d770`. The delimited reader approval is a199. The JSONL reader approval is a185. This review cites those approvals. It does not re-close them.

No product file, manifest, lock, ledger, or supply-chain policy was edited. `Cargo.lock` blob `2990f901ea98a7b465f462cce8742bc5c51c13aa` and `crates/oo-connectors/Cargo.toml` blob `84bb751a9e82ef4aa58d4123fdfb679d7bb1d4d5` match the parent. a207 remains the connector-manifest and root-lock writer. This directory is the review archive.

## What was read

- a220 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`
- `contracts/connectors/file-worker-parent-v1.md` and the parent, worker, delimited, and JSONL sources saved under `source/blobs/`
- a199 `tools/sha256_manifest.rs`, a224 `tools/sha256_files.rs`, and the a204, a200, and a224 launch shapes

`recipes/ORIGIN.txt` lists the origin commit, path, and SHA-256. The preoverlay container rechecked `recipes/origin.list` with the Safe Rust hasher (`origin.exit` 0). a200's smoke and proof launches start PostgreSQL and were not copied. This review starts no PostgreSQL.

The executed tree is a `git archive` of `bf30b717`. Repo HEAD at the preoverlay launch was `56c506b8b061783873634486cbc39a3888b0e6bc`. Repo HEAD at the overlay-4 launch was `d1753f730eba1fedb7dba4ecb0743413e99bf8ba`. `git diff bf30b717` is empty for the parent, the author test, `lib.rs`, the contract, the worker, the connector manifest, `Cargo.lock`, `delimited.rs`, and `jsonl.rs`. The process that was tested is the sealed archive.

## Pre-overlay smoke

`linux/preoverlay` ran before the review harness existed in an archive. `blob-cmp.exit` 0. `overlay-absent.exit` 0. `parent-present.exit` 0. `hash-compare.exit` 0. `origin-rustc.exit` 0. `origin.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, Debian 13, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `0295df92840ec44066887bfb2044e1e7d3483f2cc1d8461604a8ed60ecdf7364`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a237-eef831aa-net`. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. `/src` was read-only. Capacity at launch was `own=0 global=0 host_cargo=0`. `/usr/bin/setsid` and `/bin/setsid` are both present.

`fetch-offline.exit` 101 is the cold private cargo volume. The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `test.exit` 0 is the author `file_worker_parent` suite: `ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s`. The author stall line in that stdout is `stall_elapsed_ms=806 kind=TimedOut exit=None signal=Some(9) group=Sent reaped=true alive=false`. `clippy-component.exit` 0. `clippy.exit` 0 on `--lib --test file_worker_parent -- -D warnings`, ending `Finished dev profile [unoptimized + debuginfo] target(s) in 28.30s`. `clippy.stdout` is empty. `docker-run.exit` 0. The two `sqlx-postgres` `unfulfilled_lint_expectations` warnings are the locked vendor. Clippy still exited 0. No supply audit ran. The lock blob is unchanged from the parent, so this review did not repeat a207's crate fetch.

## Decisive integration run

`linux/overlay-4` copied `overlay/file_worker_parent_a237_review.rs` onto a fresh archive of `bf30b717` as `crates/oo-connectors/tests/file_worker_parent_a237_review.rs`. The product blobs match `source/blobs`. `blob-cmp.exit` 0. `parent-present.exit` 0. `fetch.exit` 0 with `cargo fetch --locked --offline`. `clippy-component.exit` 0. `test.exit` 0. `hash-rustc.exit` 0. `hash-compare.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.17s`.

Container `c0e34504663b413648d375181ee472ea2269eca544b34c58c602d47d28b73c86`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `Privileged=false`. `OOMKilled=false`. Network `oo-a237-eef831aa-net`. `ReadonlyRootfs=false`. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. `/src` was read-only. Capacity at launch was `own=0 global=0 host_cargo=0`.

The production cases spawn `CARGO_BIN_EXE_oo-file-parser-worker`. The parent clears the child environment and passes no arguments. Profile digests and config identities are `DelimitedProfile` and `JsonlProfile` `profile_digest` and `config_identity`. The harness SHA-256 self-checks the empty digest and `abc`. `tools/sha256_files.rs` `check-oracle` and `/usr/bin/sha256sum` both match the five saved fixtures (`linux/overlay-4/hash-compare.txt`). The author suite was not run again in this container.

Clippy is `cargo clippy --locked --offline -p oo-connectors --test file_worker_parent_a237_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.24s` after the same two vendor warnings. `clippy.stdout` is empty.

## Fresh fixtures

| Fixture | Rows | Batches | Observed values | Object SHA-256 |
|---|---:|---:|---|---|
| CSV `a237-lot` | 3 | 1 | `甲`, empty sku, `鹿`; `i64::MIN`, `i64::MAX`, `0`; decimal coefficients `3140`, `3140`, `-10`; quoted empty, null `~`, quoted `x\ny` | `5c1f3ff40c4c4e2023639f63d098ddb3a7e86cbfc1df190f534516325364d825` |
| TSV `a237-tab` | 2 | 1 | `鹿` and `b,c`; quoted `q\tt` and `北京`; `-7` and unquoted `-0` stored as `0` | `52b23b89cd9d79ef686907f22f0c1b9a12a3e0c0d5f8dba46f82882fbd9ea50b` |
| JSONL `a237-notes` | 3 | 3 | `丙` / null label / missing label; flags `true`, `false`, `true`; `n` is `i64::MIN`, `1`, `4` | `6f3bea2e4a200553bde6dca510a4189abcc8cf3a465fcc30fcc8dfb23964f847` |
| Empty JSONL | 0 | 0 | `arrow_ipc=false`, columns 3 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Recovery JSONL | 1 | 1 | `丁`, flag `true`, `n` `2`, after the deadline kill | `d9bae52486aece478d26cc4cb51c47a7345eab837eba6bf47ac6a0a9ad5b4f5b` |

CSV `raw_bytes` equals the file length minus the header. TSV `raw_bytes` equals the file length. JSONL `raw_bytes` excludes the trailing newline. The quoted-newline note records line 4, the line of its first byte. Price metadata is `oo.delimited.logical=decimal` and `oo.delimited.scale=3`. Label metadata is `oo.jsonl.logical=utf8`. A null label offset is the opening quote of `"label"`. A missing label offset is `{`. Lineage `arrow_bytes` and the decoded `get_array_memory_size` sum are each non-zero and within `batch_bytes + 65536` on the row batches. This review does not require those two sums to be equal. Receipts report `admits_import=false`, `published=false`, and `pg_run_complete=false`. `KERNEL_RESTRICTION` is `Unsupported`. The protocol string is `oo.source.file-worker-parent/v1`.

## Refusal, missing completion, trailer, deadline, recovery

Deadline `0` and `120001` return `Config("deadline")`. A relative worker path returns `Config("worker_path")` and the display hides `relative-worker`. A symlink of the worker returns `Config("worker_path")`. `/tmp/../etc/passwd` returns `Config("path_form")`. A symlink input returns `Config("not_regular")`. A JSONL profile with a delimited budget returns `Config("profile_budget")`.

A flipped object hash still starts the worker. The pull ends `FailureKind::HashMismatch`, token `hash_mismatch`, exit `Some(6)`, reaped. The display hides the path and `a237-secret-cell`. An unterminated quote exits `Some(4)`, reaped, and the kind is not `TimedOut`. The display hides the path and `a237-open`.

A fixture that writes nothing and exits 0 is `FailureKind::Ended`, exit `Some(0)`, reaped. A tag-3 frame whose payload is `a237-not-a-receipt\n` followed by the byte `Q` is `FailureKind::Trailer`, reaped. The display hides the path and that payload text. `Ok(None)` is not the receipt. `ParentParseComplete` is `finish` after a verified clean EOF.

The stall fixture sleeps. Parent deadline 800 ms. Overlay-4 prints `stall_elapsed_ms=835 kind=TimedOut exit=None signal=Some(9) group=Sent reaped=true alive=false`. Wall time is at least 700 ms and below 5000 ms. The next call is a real one-row JSONL through `oo-file-parser-worker`, and that call returns `ParentParseComplete` with the private flags false.

## Setsid limitation

The fixture reads the request JSON, which carries the absolute input path because the parent passes no arguments, and spawns `/usr/bin/setsid` on `a237-hold`. Overlay-4 prints `setsid hold_pid=189 session=189 direct_reaped=true direct_alive=false group=Sent kernel=unsupported`. The direct child is reaped and `/proc` for that pid is gone. The hold's session equals its pid, so it is outside the killed group. This review then sends `/bin/kill -KILL -- 189` and observes `/proc/189` disappear. `GroupSignal::Sent` records that `/bin/kill -KILL -- -<pid>` succeeded for the owned group. It does not mean every descendant died. `KERNEL_RESTRICTION` remains `Unsupported`.

## Preserved attempts

- `linux/overlay` container `36320a81d02d1416489df80df841d645122c10db7a40a38d2d4db989a9408666`, `test.exit` 101, `clippy.exit` 101. The harness borrowed a temporary `schema()`. `hash-compare.exit` is `not_run`.
- `linux/overlay-2` container `da25d89cdac39d9c6ae0315e41b4bc3e3ac3541fc1b9203ac095b47a45b6a9ac`, `test.exit` 101, `clippy.exit` 101. The harness shadowed the schema-identity binding with an `Arc<Schema>`. `hash-compare.exit` is `not_run`.
- `linux/overlay-3` container `1d86f3510db3029ae3dd74c9acb7b110623fb982a92cbb2f3e8f14b5e6b93a59`. Tests passed (`5 passed`, 5.13s) and `hash-compare.exit` 0. Clippy denied `while_let_loop` and `let_and_return` in the harness (`clippy.exit` 101).

Those three are harness compile and lint failures. The product archive stayed `bf30b717`. The style-only harness edit between overlay-3 and overlay-4 did not change the assertions. Overlay-4 re-ran them.

## Manifest and cleanup

`linux/manifest/launch.sh` compiles `tools/sha256_manifest.rs` with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest` inside the pinned image. `/tmp` is `noexec`. The first container's stdout is copied to `linux/manifest/console.log` after that process exits, so a later container rewrites `MANIFEST.sha256`. The final container's stdout stays outside this directory and that container writes no other file here. `CLEANUP.txt` records removal of this review's network, cargo volume, target volume, and `/tmp` extract. `docker prune` is not used. The pinned Rust image stays. Foreign containers stay.
