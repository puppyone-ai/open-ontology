# Report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a227`. Unit M1-D14B / SP06-W01. This report records the Linux proof. It does not accept the unit, close SP06-W01, or release.

## Archive

Fixed commit `ed661809cf6c1e8df37af5842838dd1eb839c98a`. Contract commit `bbc37839514a995100b176107549246605c49d36` (`gateway: freeze the reviewed policy adapter contract`), blob `23e58a66d0c4111a1253c4723998f026c24c2667`. Product commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497` on parent `e02b3c8a8ad0e54d968791e244b444189c6bcfff`. The parent is not the fixed commit. The green build is the fixed archive plus the owned adapter files. It is not a build of the parent tree.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. `--pull never`. Memory 6442450944, NanoCpus 1000000000, cap-drop ALL, no new privileges, private network `oo-a227-eef831aa-net`, volumes `oo-a227-eef831aa-cargo` and `oo-a227-eef831aa-target`. No PostgreSQL. The PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` was not started. No host network, socket, or host home. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `cargo-update-invoked=no`.

The extracted `rust-toolchain.toml` names channel 1.98.1. Each launch moves that file aside and sets `RUSTUP_TOOLCHAIN=1.94.0`. The git object is unchanged.

## Pre-edit smoke

`linux/smoke` ran before the adapter files existed. Container `dcb316723a0bbe12209671fabda4246983cea530c9c1099a28db84cc986b559c`, removed. docker-run exit 0, smoke.exit 0, `OOMKilled=false`. `cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1` reported `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out`.

## Red, then failed overlays

`linux/red` is the test-first run. Container `a870753002421a71684be13aeed8cc511f9a6b5cbc826b5e57092589b2e495fb`, removed. docker-run exit 101, `OOMKilled=false`. The test could not read `reviewed_policy.rs` (`include_str` os error 2), the import `oo_gateway::reviewed_policy` was unresolved (`E0432`), and `format!("\\u{ch:04x}")` failed `char: LowerHex` (`E0277`).

`linux/overlay` compiled `oo-builder` and `reviewed-effective-oos` (build exit 0) and failed in the test compile. Container `f0bf0b2f810cefb14842dd5378be317f17fdf1d20787b302be6fb1b9d809264d`, removed. docker-run exit 101, test exit 101, `OOMKilled=false`. `error[E0282]` at `let mut policies = Vec::new()` in `group_policies`.

`linux/retry1` ran the real compiler bins. Container `3156ec011550b41dd521d2eb441e2a3c9207587727510d84bcd58ad1bf2f8e40`, removed. docker-run exit 101, test exit 0, clippy exit 101, `OOMKilled=false`. `test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.42s`. Clippy `-D warnings` reported `field fingerprints is never read` in the test. The library check had finished.

Those three console logs are preserved.

## Green run

`linux/retry2` is the green run. The launch script still prints `phase=overlay` and `overlay_proof=pass` because it was copied from the overlay script. The directory, the container name `oo-a227-proof3`, and the phase files are retry2. Container `7b718b7e702515db6f208fa1c37cec275409ec0171943781368685c320491aa1`, removed (`container-removed.txt` is `oo-a227-proof3`). docker-run exit 0, test exit 0, clippy exit 0, lock exit 0, `OOMKilled=false`, `Privileged=false`, `CapAdd=null`, `PortBindings={}`.

`test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.40s`. Clippy log: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.61s` with no warning. The owned-file SHA-256 values in `linux/retry2/owned-sha256.txt` match commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497`.

Commands:

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo test --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1
cargo clippy --locked --offline -p oo-gateway --lib --test reviewed_policy -- -D warnings
```

`OO_A227_OO_BUILDER=/target/debug/oo-builder` and `OO_A227_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos`. The integration test fails if either variable is missing. The `ls` line about `libclang.so*` is the same cosmetic miss as smoke; `libclang` phase exits are 0 and the root was reused.

## What the green fixture did

The test builds a sealed shop catalog through the compiled `oo-builder` captured-draft path and `reviewed-effective-oos`. Schema `public`, source `shop_src`. Tables `customer` (`id` int4, `email` text, `active` bool) and `purchase` (`id` int4, `customer_id` int4, `note` text). Roles `clerk` (six columns, explicit allow on `email` and `note`) and `peek` (four non-PII columns) are two adapter calls. `compiled.role()` is `oo_r_clerk` and `oo_r_peek`. Generation is 4. Every returned policy has `row_filter: None` and `ClassStatus::Confirmed`.

Stale schema, stale fact, rename, unknown name, hidden-after-bind, rejected column, missing decision, PII without an explicit allow, duplicate allow, duplicate decision, mixed identity, raw sample, credentials, `row_filter`, RLS, float4, unsupported semantic types, pretty JSON, protocol-only JSON, seal tamper, scanner failure, and an empty allowlist are refused. The empty allowlist is `Policy(EmptyPolicySet)` after the document checks.

The a218 live grant proof for `bool`, `int2`, `int4`, `int8`, and `text` is referenced and was not run again.
