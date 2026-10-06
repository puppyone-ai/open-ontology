# Handoff

Independent review of commit `13566602dc36ce6ebd03fef25a9bd17aae7a281c` is next. This session recorded `FINDINGS.md` for the frozen scope. It did not edit `specs/open-ontology-development-ledger.md` or the plan checkbox. M1-D14D stays open.

## Commit

Product `13566602dc36ce6ebd03fef25a9bd17aae7a281c` on parent `8a758c17c51e88d64003ac27c7c3f65d48bbc4ff`.

The commit is only:

- `crates/oo-gateway/src/bin/install-column-acl.rs` blob `6750e119476dca08b1acecbfb6397204cb2ad70a`, SHA-256 `9733c33e4ef7ec4e47489db23039bd01bfa3cf9f0f46b389820da32e27367c55`
- `crates/oo-gateway/tests/install_column_acl_cli.rs` blob `2514cc23efd8fba6a320372f4fdcb75129d90cdb`, SHA-256 `1a0cdfff01a5d965cd7c6ce49786d4bb874204badc09f2633e1c451aaa158af9`
- `contracts/gateway/install-column-acl-cli-v1.md` blob `772b1e2c11056eb869e6d9efda2c0228d7c42ef1`, SHA-256 `d0df87be3cd4e0bd49c8b23a9c17bd7a241445d2c704affa5b9110fe0f639eb4`

Fixed archive for both Linux runs: `a48db85fc13148874349d318affd995f7212188d`. `policy.rs`, `lib.rs`, `Cargo.toml`, `Cargo.lock`, `column-acl-set-v1.md`, `reviewed-policy-v1.md`, and `query-pg.rs` match that archive. The evidence commit adds this directory.

## Reproduce the green check

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, private cargo and target volumes, `--pull never`, `--memory 6g`, `--cpus 1`. Move `rust-toolchain.toml` aside on the private archive copy. One private PostgreSQL 17.10 container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, TLS name `oo-a235-pg`, no host port. Mount only `ca.crt`, `admin.pass`, and `login.pass` into the builder. Archive `a48db85fc13148874349d318affd995f7212188d` and copy in the three owned files. Set `OO_A235_LIVE=1`.

```text
cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1
cargo test --locked --offline -p oo-gateway --test install_column_acl_cli -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --bin install-column-acl --test install_column_acl_cli -- -D warnings
```

The green transcript is `linux/proof/console.log`. `proof.exit` is 0. The pre-overlay smoke is `linux/smoke`. Failed proof attempt 1 stays under `linux/proof/attempt-1/`.

## Review focus

- The binary calls `compile_column_acl_set` before it opens the password or CA, and calls `install_column_acl_set` on the TLS client. It prints static reason words. It does not print `PolicyError` display text.
- A stale generation and a missing second table roll back. Both roles are absent. Clerk install, peek install, and clerk replay are the live success path. Replay returns the same `tables` array.
- Column grants are 6 and 4. Table privileges are 0. `physical_mappings` column and table privileges for the two roles and the login sum to 0. The CLI does not issue grants. `policy.rs` was not edited.
- The query and the 42501 checks use the login connection and `SET LOCAL ROLE oo_r_clerk`. `query-pg` is not invoked. The test does not `ANALYZE`.
- Authentication, the audit WAL, publication, and row-level security are open. The tested grantor is superuser `oo_fixture_admin`.

## Still open

Authentication, row-level security, publication, the audit WAL, a non-superuser grantor, and `query-pg` over this install. M1-D14D remains open. No manifest or lock was edited.

`oo-a235-smoke` and `oo-a235-proof` were removed by their launch scripts. `cleanup/run.sh` removes `oo-a235-pg`, volumes `oo-a235-eef831aa-cargo`, `oo-a235-eef831aa-target`, and `oo-a235-eef831aa-pgdata`, network `oo-a235-eef831aa-net`, and `/tmp/oo-a235-eef831aa-dba4-4af2-a78b-541bc9e0a235`. The script checks owner `a235` and this session. It does not prune. The transcript is `CLEANUP.txt`.
