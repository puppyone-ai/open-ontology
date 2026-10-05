# Report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a234`. Unit M1-D14C / SP06-W01. This report records the Linux proof of the `reviewed-policy` command. It does not accept the unit, close SP06-W01, or release.

## Archive

Fixed commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497`. Contract commit `e21f20d2894d0ffc584fb68709c0e355d94201a6` (`gateway: freeze the reviewed-policy command contract`) on parent `1b54eb56`. Product commit `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d` (`gateway: add the reviewed-policy command`) on parent `e21f20d2`. The green build is the fixed archive plus the three owned files. It is not a build of the parent tree.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. `--pull never`. Memory 6442450944, NanoCpus 1000000000, cap-drop ALL, no new privileges, private network `oo-a234-eef831aa-net`, volumes `oo-a234-eef831aa-cargo` and `oo-a234-eef831aa-target`. No PostgreSQL. The PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` was not started. No host network, socket, or host home. `Cargo.lock` content SHA-256 stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. `cargo-update-invoked=no`.

The extracted `rust-toolchain.toml` names a newer channel. Each launch moves that file aside and sets `RUSTUP_TOOLCHAIN=1.94.0`. The git object of the fixed commit is unchanged.

Owned blobs, git hash-object, identical in the worktree, the archive overlay, and the commits:

- `contracts/gateway/reviewed-policy-cli-v1.md` `6ae87b2c56468793151ccceccbf2fc34e80125fe`, content SHA-256 `ba1fa49985396fd4a5d0d3f9f4b2265e23f31f9833dce2c79896ff91f3c69f77`
- `crates/oo-gateway/src/bin/reviewed-policy.rs` `34f7824250c39a58f177e70f21dc6db02b5527bd`, content SHA-256 `bb5005880446e8924ea2b7839d5a4fc5c3f77dcc5ee9e8022631058a6b2cdf1d`
- `crates/oo-gateway/tests/reviewed_policy_cli.rs` `de88e2dba6648fadf61d5f92884732317db13a1c`, content SHA-256 `5bad8fe463a6fda8dff530e0f1c1ef4cbdcd3966030ad8ab12847d56bdec84a1`

Fixed blobs checked on every overlay: `reviewed_policy.rs` `56b62fdc1ab9864b99d992ccb46782fc34c1a507`, `lib.rs` `b18192528299c4744170bb12d1b0c89088b9cb66`, `policy.rs` `84a86e85ef82bc2a459a5e2faef5827d077ca6cc`, `crates/oo-gateway/Cargo.toml` `8a78503420468aa9f99aee83a5401c42f91ee40a`.

## Pre-edit smoke

`linux/smoke` ran on the fixed archive before the command files were copied on. Container `626319a940e313f73e64aca33f6c82b56e049648b67de90c3428187ed96b90d0`, removed. docker-run exit 0, smoke.exit 0, `OOMKilled=false`. `cargo test --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1` reported `test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.40s`. The archive check refused `reviewed-policy.rs` and `reviewed-policy-cli-v1.md`. `cli-absent.txt` is `cli_absent`.

## Failed overlays, preserved

`linux/proof` compiled the producers and the command, and the six CLI tests passed. Clippy `-D warnings` then failed. Container `3f8118932da8a7d08d33272c619983c793b55c5180514145ef00d6ebd758c036`, removed. docker-run exit 101, proof.exit 101, test exit 0, clippy exit 101, `OOMKilled=false`. `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`. Clippy reported `redundant guard` at `reviewed-policy.rs:257` (`clippy::redundant_guards`) on `Self::Decision(reason) if reason.is_empty()`. `linux/proof/product-at-failure/reviewed-policy.rs` is that pre-fix blob `de8a11709da47e044f35bcf4ebffdfd8a9c615a7`. The console log is preserved.

`linux/retry1` used the fixed match `Self::Decision("")`. Tests and Clippy passed. The harness then treated `#![forbid(unsafe_code)]` as an unsafe token. Container `1bd446c43f14c3a5de078549c006ddbf1dea7b94eb13efb1446426e4d2bc4c44`, removed. docker-run exit 2, retry1.exit 2, test exit 0, clippy exit 0, unsafe exit 2, `OOMKilled=false`. `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s`. Clippy stderr is `Finished dev profile [unoptimized + debuginfo] target(s) in 0.38s`. `unsafe-lines.txt` is `9:#![forbid(unsafe_code)]`. Product blob `34f7824250c39a58f177e70f21dc6db02b5527bd` is the committed command. The console log is preserved.

## Green run

`linux/retry2` is the green run. The unsafe check filters out `forbid(unsafe_code)`. Container `bb86c87855289b18874b2289788101579b4625a035cc9ee01d273e91fd0c1a79`, removed (`container-removed.txt` is `oo-a234-retry2`). docker-run exit 0, retry2.exit 0, test exit 0, test-count exit 0, clippy exit 0, lock-hash exit 0, unsafe exit 0, `OOMKilled=false`, `Privileged=false`, `CapAdd=null`, `PortBindings={}`. `unsafe-lines.txt` is empty.

`test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s`. Clippy stdout is empty. Clippy stderr is `Finished dev profile [unoptimized + debuginfo] target(s) in 0.26s`.

Commands:

```text
cargo build --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos
cargo build --locked --offline -p oo-gateway --bin reviewed-policy
cargo test --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1
cargo clippy --locked --offline -p oo-gateway --bin reviewed-policy --test reviewed_policy_cli -- -D warnings
```

`OO_A234_OO_BUILDER=/target/debug/oo-builder`, `OO_A234_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos`, and `OO_A234_REVIEWED_POLICY=/target/debug/reviewed-policy`. The integration test fails if any variable is missing. The `ls` line about `libclang.so*` in the smoke console is the known glob miss; `libclang-19.so.19` is the loaded file and the libclang phase exits are 0. Retry2 reused `/cargo-home/a234-clang-root`.

## What the green fixture did

The test builds a sealed shop catalog through the compiled `oo-builder` captured-draft path and `reviewed-effective-oos`. Schema `public`, source `shop_src`. Tables `customer` (`id` int4, `email` text, `active` bool) and `purchase` (`id` int4, `customer_id` int4, `note` text). Roles `clerk` (six columns, explicit allow on `email` and `note`) and `peek` (four columns, no email or note) are two command invocations. `compile_column_acl_set` reports roles `oo_r_clerk` and `oo_r_peek`. Generation is 4. Successful policies have `row_filter` null and status `confirmed`. Email is `declared` with `explicit_allow` true. `none` omits `explicit_allow`. Stdout does not contain the owner string or the sample marker.

The same clerk document was run twice and the stdout bytes matched. The parsed `command` string equals the producer stdout. The test deserializes the command output with its own types, builds `ColumnAclPolicy` values, and checks them against `compile_reviewed_policies`. A hand-built claims `row_filter` converts and `compile_column_acl_set` returns `RowFilterRefused`.

Stale schema is the producer recompiled with the email decision schema hash set to `ef` repeated 32 times. The command exits 10 and prints `reviewed-policy: decision stale_schema`. Email with `explicit_allow` false, and `id` with `explicit_allow` true, each exit 12 `reviewed-policy: pii_allow`. A recanonicalized command that contains `samples` exits 14 `reviewed-policy: forbidden` and does not echo the marker. `{`, a duplicate key, 1048577 bytes, an argument, an extra field, a trailing newline on the command string, and an empty allowlist exit 4, 4, 3, 2, 5, 6, and 16. The oversize marker and the argument are not echoed.

Recipe origins are `origin/recipes.txt`. No dependency was added. a207 remains the only connector-manifest and root-lock writer.
