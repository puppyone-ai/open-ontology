# Source inventory

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a340`. Product inputs are commit `21cc96241a841b47dbbcb116549030e38a643ba0`. The builder hashed these paths from the readonly archive. `phases/archive-hash.exit` is 0. A later host `shasum -a 256` of `.a340-work/archive` matched the same eight digests.

| SHA-256 | path |
| --- | --- |
| `3cf525604513d3a64be2e5689e9011986a079a342f1bac6c863438ae5d34a242` | `crates/oo-gateway/src/reviewed_policy.rs` |
| `46f7a8d79af146f5199896ad0ac820fe5415e39fd9530a6a887e7dbbc8286a6d` | `crates/oo-gateway/src/query_executor.rs` |
| `b6cd2f4e5a5af9d895a6c2ab9020ca6956afc4f61dd0b799a9cc297f54c13a22` | `crates/oo-gateway/src/bin/query-pg.rs` |
| `ac298ba546c7cea9595982dcb0341f2fe60b3dc9b3115ca89f070aad77251a2b` | `crates/oo-gateway/tests/a327_numeric.rs` |
| `63b28a2cd5da4914b124d7eac42d7d8b815033d96208118d3fb4c94b242f4325` | `crates/oo-gateway/src/policy.rs` |
| `1d914418d587a550984858e4f11874a80a0d0f5ea8a1b9861f9d789a4a60ca67` | `crates/oo-gateway/src/policy_catalog.rs` |
| `3334da9c6d6b523b0def7710a0fcea1129d9c0ba1cc06da2e7919b65073bb00b` | `crates/oo-gateway/src/mapping_reader_access.rs` |
| `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` | `Cargo.lock` |

The seven Rust paths are the numeric surface checked by `tools/gold.rs` and by the live harness in `query-pg.rs`. `Cargo.lock` is the lock inside the same commit. The a327 overlay lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` is recorded in `inside.sh` as unused.

## Archive members

`launch.sh` archives `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/gateway-token-store` from `21cc9624`. `tools/gateway-token-store/src/cache.rs` is present in the archive. That tree is a build input of `oo-gateway` `build.rs` on this commit. It is not a review-scope overlay.

`linux/proof-failed-1` used an archive without `tools/gateway-token-store`. The passing archive adds that tree from the same commit.

## Review-owned files

These files live only under this evidence directory:

- `tools/gold.rs`, the corrected stdout and source-boundary oracle.
- `tools/sha256.rs`, std-only, byte origin `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` from a327.
- `linux/proof/launch.sh`, `linux/proof/inside.sh`, `linux/proof/extract-libclang.sh`.
- `linux/proof-failed-1/` and `linux/proof-failed-2/` copies of the scripts that produced those attempts.
- `pg/up.sh`, `pg/capture.sh`, `pg/pg-wrap.sh`.
- `origin/recipes.txt`.

`pg-wrap.sh` SHA-256 matches the a327 origin `f6969f4f34a41bc857d4d6156622c0eb7201cf3846fb63055e978a3a624d95e9`.

## What this review did not change

No file under `crates/`, `Cargo.toml`, `Cargo.lock`, or `tools/gateway-token-store` was written by this review. `cargo update` was not invoked. Root Axum dependencies were not edited.
