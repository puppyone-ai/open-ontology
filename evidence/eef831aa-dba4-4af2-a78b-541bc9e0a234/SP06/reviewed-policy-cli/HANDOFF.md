# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a234`. Fixed archive `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. This handoff does not accept M1-D14C. It does not check a parent box. It does not release. SP06-W01 stays open.

## Commits

Contract `e21f20d2894d0ffc584fb68709c0e355d94201a6` on parent `1b54eb56`. Product `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d` on parent `e21f20d2`. The parent is not the fixed commit. The green container compiled the fixed archive plus these blobs:

- `contracts/gateway/reviewed-policy-cli-v1.md` blob `6ae87b2c56468793151ccceccbf2fc34e80125fe`, SHA-256 `ba1fa49985396fd4a5d0d3f9f4b2265e23f31f9833dce2c79896ff91f3c69f77`
- `crates/oo-gateway/src/bin/reviewed-policy.rs` blob `34f7824250c39a58f177e70f21dc6db02b5527bd`, SHA-256 `bb5005880446e8924ea2b7839d5a4fc5c3f77dcc5ee9e8022631058a6b2cdf1d`
- `crates/oo-gateway/tests/reviewed_policy_cli.rs` blob `de88e2dba6648fadf61d5f92884732317db13a1c`, SHA-256 `5bad8fe463a6fda8dff530e0f1c1ef4cbdcd3966030ad8ab12847d56bdec84a1`

`source/product/` matches those files. `source/COMMIT` is the fixed archive. `source/blobs/` holds the fixed copies of `reviewed_policy.rs`, `policy.rs`, `lib.rs`, `Cargo.toml`, and `Cargo.lock`.

## Read first

- `REPORT.md` — smoke, the clippy failure, the unsafe-grep failure, and the green commands.
- `FINDINGS.md` — the DTO seam and the 1 MiB envelope.
- `REVIEW.md` — same-session review of the frozen scope. Not an independent review.
- `linux/smoke/` — pre-edit 21 `reviewed_policy` tests, exit 0. Do not overwrite `console.log`.
- `linux/proof/` — six tests passed, Clippy `redundant_guards`, exit 101. `product-at-failure/` is the pre-fix bin blob `de8a11709da47e044f35bcf4ebffdfd8a9c615a7`.
- `linux/retry1/` — tests and Clippy green, harness grep matched `forbid(unsafe_code)`, exit 2.
- `linux/retry2/` — green test, Clippy, lock, and unsafe check, exit 0.
- `origin/recipes.txt` — a227, a204, a200, and a199 recipe blobs and the edits.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. Move the extracted toolchain file aside. Copy the three owned files onto that archive. No PostgreSQL.

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo build --locked --offline -p oo-gateway --bin reviewed-policy
cargo test --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1
cargo clippy --locked --offline -p oo-gateway --bin reviewed-policy --test reviewed_policy_cli -- -D warnings
```

Set `OO_A234_OO_BUILDER`, `OO_A234_REVIEWED_EFFECTIVE`, and `OO_A234_REVIEWED_POLICY` to those binaries. The green transcript is `linux/retry2/console.log`. `phases/test.exit` and `phases/clippy.exit` are 0.

## Still open

The command's JSON types are local. `ColumnAclPolicy` has no serde. The test converts a second DTO and calls `compile_column_acl_set`. `query-pg` does not import either DTO. The operator document cannot carry three max-size adapter documents. Hidden-after-bind is `seal`. Authentication, grants, publication, live row security, and an audit WAL are outside this command. M1-D14C and SP06-W01 stay open. No dependency was added.

## Cleanup

`oo-a234-smoke`, `oo-a234-proof`, `oo-a234-retry1`, and `oo-a234-retry2` were removed by their launch scripts. `cleanup/run.sh` removes any of those that remain, plus `oo-a234-hash` and `oo-a234-hash2`, volumes `oo-a234-eef831aa-cargo` and `oo-a234-eef831aa-target`, network `oo-a234-eef831aa-net`, and `/tmp/oo-a234-eef831aa-dba4-4af2-a78b-541bc9e0a234`. The script checks owner `a234` and this session. It does not prune. The transcript is `CLEANUP.txt`. Pinned images stay. The inventory containers run after that transcript and remove themselves. `linux/hash/note.txt` records that order.
