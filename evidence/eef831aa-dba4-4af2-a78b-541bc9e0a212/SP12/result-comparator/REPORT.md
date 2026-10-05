# Result comparator proof

Session `eef831aa-dba4-4af2-a78b-541bc9e0a212`. Fixed product archive `a661dcdf72ff91561ae6b7cb79916e582884d08a`. This is the SP12-W03 primitive B-D01: one versioned expected typed result and one actual typed result become one exact comparison report.

This report does not accept B-D01, the 300-question bank, a quality benchmark, or a release.

## What was built

`benchmarks/result-comparator` is its own Cargo workspace. It is not a root workspace member. The root manifest, root lock, and product dependencies were not edited. `oo-core` is a path dependency. The proof compiles that path against a `git archive` of the fixed commit, with the archive's `rust-toolchain.toml` renamed so the image stays on 1.94.0.

The standalone lock was created with `cargo generate-lockfile`, not `cargo update`. Every registry name, version, and checksum in that lock is already in the fixed root lock. `lock-drift.txt` is empty. The pins are serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The verified lock is `benchmarks/result-comparator/Cargo.lock`.

## Proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0. One CPU, 6 GiB, cap-drop ALL, no new privileges, private network `oo-a212-eef831aa-net`, private cargo and target volumes. No PostgreSQL. The container was removed after the run. No prune.

Smoke, before the comparator sources were archived: `oo-core` decimal tests, exit 0. Recorded in `linux/smoke/`.

Implementation attempts 1 through 4 are preserved and failed:

| Attempt | Exit | Cause |
| --- | --- | --- |
| `linux/impl` | 2 | `child()` `String` passed where `array` expects `&str`; rustfmt diff; unused `mut` |
| `linux/impl-2` | 2 | column-name closure needed `Vec<Column>` |
| `linux/impl-3` | 2 | `Comparison` lacked `Debug`, required by `expect_err` |
| `linux/impl-4` | 2 | binary env var keeps the hyphen: `CARGO_BIN_EXE_result-comparator` |
| `linux/impl-5` | 0 | tests, rustfmt, clippy, and CLI fixtures |

Attempt 5 container `291f2a9deaa594c06d0130052aff052a1c2d5adb3f2afe8dced8298740cf7eac`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. Phase exits: rustc, source hash, lock, lock drift, test, rustfmt, clippy, CLI, and docker are all 0.

`cargo test --locked --offline -- --test-threads=1`: 35 passed, 0 failed. That is 16 bound tests, 4 CLI tests, 12 gold tests, and 3 FIPS 180-4 tests through `oo_core::canonical::hash_bytes`. Doc tests and crate unit tests are empty.

The debug binary then compared every handwritten fixture. Match fixtures exited 0. Mismatch fixtures exited 1. Stderr was empty and stdout matched the handwritten report bytes.

The attempt 5 archive copy of the crate, excluding the generated lock, matches the crate sources that were tested. The lock file copied into the crate is the lock that attempt 5 verified.

## Behavior the tests cover

Equivalent exact decimals match, including a negative scale and two non-negative zero coefficients at different scales. `120` at scale 2 does not match `121` at scale 2. Null does not match missing. An unordered multiset keeps duplicate multiplicity. An ordered swap does not match. The same swap with `unordered` matches. Partial does not match complete in either direction. `not_found`, warnings, and `failed` are compared. `set` is rejected. Duplicate keys, non-canonical int64 spellings, a fractional scale, a JSON-number coefficient, the document byte cap, 2001 rows, and the payload byte cap are config.

## Inventory

`tools/sha256_manifest.rs` is the untouched a199 hasher. `linux/inventory/` compiles it in the same pinned image and writes `crate-sha256.txt` and `SHA256SUMS.txt`.
