# enum-history proof

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a270`. Frozen child SEM-D02 / SP08-W01B. Owned paths are `tools/enum-history/**` and this evidence directory. Fixed archive `0bcd494457091d4b55c82fc6b9922b5bc5dc44c4`. Passing runs are `linux/proof-3` and `linux/review-1`.

This report does not accept SEM-D02, close SP08, or call a release. Database sweep, hourly scheduling, `oo_dict` writes, metric certification, and customer owner proof stay later gates.

## Contract

`tools/enum-history/CONTRACT.md` was written before the compiler. The crate is a standalone workspace. It calls `oo_core::oos::parse_enum_yaml` and adds interval, overlap, reserved-class, inferred-review, and exact observed-coverage rules. The library does not write the enum YAML. Declared `coverage` is copied into the report and is not the observed ratio.

## Builders

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0. Each run is one container, 6 GiB, 1 CPU, `--cap-drop ALL`, no new privileges, private network `oo-a270-eef831aa-net`, `HOME=/tmp`, no host home, no auth env. PostgreSQL was not started. `cargo update` was not run. The root lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

| run | role | result |
|---|---|---|
| `linux/preoverlay` | focused `oo-core` parse smoke before the tool existed on the archive | exit 0 |
| `linux/proof-1` | first overlay, lock subset, tests, clippy | tests 0, clippy 101 |
| `linux/proof-2` | boxed `Outcome` on the proof-1 lock | tests 0, clippy 1 |
| `linux/proof-3` | same lock, clippy component installed in the container | exit 0 |
| `linux/review-1` | fresh target volume, same sources and lock | exit 0 |

`linux/proof-1` and `linux/proof-2` are preserved.

## Preoverlay

Container `oo-a270-preoverlay` removed. `smoke.exit=0`, `fetch.exit=0`, `test.exit=0`, `docker-run.exit=0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. The archive did not contain `tools/enum-history`. `examples/schema/enum-ysk-zt.yaml` hashed to `07d7e26d32edaba62bc2287cfec6c22816c78ce784b22b5df42ef7d437214839`. `cargo test -p oo-core --test schema_contract documented_oos_examples_parse --locked --offline` passed 1 test. `cargo-update-invoked=no`.

## Lock

`linux/proof-1` copied the root lock into the child workspace and ran `cargo fetch` for `tools/enum-history` only. `lock-subset` printed `registry=115 path=2 missing=0 mismatch=0`. Path packages are `enum-history` and `oo-core`. The child lock SHA-256 is `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8`. proof-1's launch left it in the evidence directory and did not install it, because clippy had failed. It was copied to `tools/enum-history/Cargo.lock` before proof-2. proof-3 and review-1 used `--locked --offline` and the hash did not change.

## Tests and clippy

proof-3 and review-1 each report:

- library ratio tests: 3 passed
- CLI tests: 2 passed
- hand-computed fixtures: 18 passed
- doc-tests: 0
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0

proof-1 clippy stopped on `clippy::large_enum_variant` for `Outcome`. The passing tree boxes `Report`. proof-2 then failed because a new container does not keep the clippy component installed by proof-1. proof-3 and review-1 run `rustup component add clippy` when it is absent. review-1 compiled on an empty target volume; clippy finished in 15.75s after that cold build.

Host config for proof-3 and review-1: `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, network `oo-a270-eef831aa-net`, `OOMKilled=false`. Containers `oo-a270-proof3` and `oo-a270-review1` were removed by their launch scripts.

## Source hashes

These match the host tree, `linux/proof-3/source-sha256.txt`, and `linux/review-1/source-sha256.txt`:

| file | sha256 |
|---|---|
| `CONTRACT.md` | `01de18f2d1108bf24fabfd5e8bf3a8d712d89618bdef44eaffa96653bc348b63` |
| `Cargo.toml` | `82272ffb2b95e2b7647ed3f70cec106fb8cdfd2c41d4cad0311f7ef532eb28ae` |
| `Cargo.lock` | `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8` |
| `src/lib.rs` | `f1b979574b4bcd458aa7e61dd9f5ab651f1a94a797ec0dee84c3d2b98e382859` |
| `src/main.rs` | `98e948c4916e92e65fc49cb45b60750b73406e1c5afc32aac92d1d8c6ab56847` |
| `src/ratio.rs` | `d90e37a447739c414e1b3e362e6c22e8f07b4ec37b17d38cde416a60bbd12ad8` |

`source/blobs/enum-ysk-zt.yaml` matches the fixed example, hash `07d7e26d32edaba62bc2287cfec6c22816c78ce784b22b5df42ef7d437214839`.
