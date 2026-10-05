# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a227`. Fixed archive `ed661809cf6c1e8df37af5842838dd1eb839c98a`. This handoff does not accept M1-D14B. It does not check a parent box. It does not release. M1-D14A and SP06-W01 stay open.

## Commit

Product `baf70c5c7b6d01183a178f53c77f65da4a7bb497` on parent `e02b3c8a8ad0e54d968791e244b444189c6bcfff`. The contract was already `bbc37839514a995100b176107549246605c49d36` and was not amended. The product commit is only:

- `crates/oo-gateway/src/reviewed_policy.rs` blob `56b62fdc1ab9864b99d992ccb46782fc34c1a507`, SHA-256 `d1a0b3e1f88715a0fab10fe7f08772f6c821a2fe0e818d9015e1e74687dc666a`
- `crates/oo-gateway/src/lib.rs` blob `b18192528299c4744170bb12d1b0c89088b9cb66`, SHA-256 `8a680dec57896e9a95fc8bdde7e353df8e8b770aeb67eb37d5ec34cc1faad10f`
- `crates/oo-gateway/tests/reviewed_policy.rs` blob `4ee107cc90db762d6c878a00dce3ec2e5b054ceb`, SHA-256 `064e297eda664525c058d2212e0397edb3047b47912eb0145a1734c4a5600948`

`lib.rs` versus the fixed commit is 5 insertions. The green container compiled those bytes. `source/product/` matches the commit. `source/blobs/` remains the fixed archive copies of `policy.rs`, `lib.rs`, `Cargo.toml`, and `Cargo.lock`.

Contract blob `23e58a66d0c4111a1253c4723998f026c24c2667`, SHA-256 `de58f632be17f5e2f77e35660fcd949cb4b0da360eff6ecce9584d7d9a0aa53b`.

## Read first

- `REPORT.md` — smoke, red, failed overlays, and the green commands.
- `FINDINGS.md` — seal reading for `draft_hash`, decisions, and open seams.
- `REVIEW.md` — same-session review of the frozen scope. Not an independent review.
- `linux/smoke/` — pre-edit `policy::tests`, exit 0. Do not overwrite `console.log`.
- `linux/red/` — missing adapter and the `LowerHex` compile error, exit 101.
- `linux/overlay/` — `E0282` on `Vec`, exit 101. Bins had already built.
- `linux/retry1/` — 21 tests passed, Clippy `dead_code` on `fingerprints`, exit 101.
- `linux/retry2/` — green test and Clippy. The script text still says `overlay_proof=pass`.
- `origin/recipes.txt` — a218, a217, a204, a200, and a199 recipe blobs and the edits.

## Reproduce

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--memory 6g`, `--cpus 1`, private cargo and target volumes. Archive `ed661809cf6c1e8df37af5842838dd1eb839c98a` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. Move the extracted toolchain file aside. Copy the four owned files onto that archive. No PostgreSQL. No new crate edge: the test shells out to the two bins.

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo test --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1
cargo clippy --locked --offline -p oo-gateway --lib --test reviewed_policy -- -D warnings
```

Set `OO_A227_OO_BUILDER` and `OO_A227_REVIEWED_EFFECTIVE` to those binaries. The green transcript is `linux/retry2/console.log`. `phases/test.exit` and `phases/clippy.exit` are 0.

## Still open

Authentication, grants, publication, live row security, and a non-superuser grantor. Physical-type reproof on a live server stays `load_policy_catalog`. The a218 grant proof is the reference and was not reclosed. M1-D14A, M1-D14B, and SP06-W01 stay open. No dependency was added.

## Cleanup

`oo-a227-smoke`, `oo-a227-red`, `oo-a227-proof`, `oo-a227-proof2`, and `oo-a227-proof3` were removed by their launch scripts. `cleanup/run.sh` removes any of those that remain, volumes `oo-a227-eef831aa-cargo` and `oo-a227-eef831aa-target`, network `oo-a227-eef831aa-net`, and `/tmp/oo-a227-eef831aa-dba4-4af2-a78b-541bc9e0a227`. The script checks owner `a227` and this session. It does not prune. The transcript is `CLEANUP.txt`. Pinned images stay.
