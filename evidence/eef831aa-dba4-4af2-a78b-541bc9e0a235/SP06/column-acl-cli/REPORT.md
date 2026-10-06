# a235 install-column-acl CLI

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a235`. Fixed archive `a48db85fc13148874349d318affd995f7212188d`. Product commit `13566602dc36ce6ebd03fef25a9bd17aae7a281c`.

The command reads one operator policy-set document, calls `compile_column_acl_set`, then `install_column_acl_set` on one TLS session. The role and login are administrative binding fields. The library installer was not edited. M1-D14D is not accepted.

## Owned product

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/bin/install-column-acl.rs` | `6750e119476dca08b1acecbfb6397204cb2ad70a` | `9733c33e4ef7ec4e47489db23039bd01bfa3cf9f0f46b389820da32e27367c55` |
| `crates/oo-gateway/tests/install_column_acl_cli.rs` | `2514cc23efd8fba6a320372f4fdcb75129d90cdb` | `1a0cdfff01a5d965cd7c6ce49786d4bb874204badc09f2633e1c451aaa158af9` |
| `contracts/gateway/install-column-acl-cli-v1.md` | `772b1e2c11056eb869e6d9efda2c0228d7c42ef1` | `d0df87be3cd4e0bd49c8b23a9c17bd7a241445d2c704affa5b9110fe0f639eb4` |

SHA-256 values are the green proof `linux/proof/owned-new-sha256.txt`. Blobs are `git hash-object` of the same bytes.

## Archive

Partial archive of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` from `a48db85fc13148874349d318affd995f7212188d`. Listed files 953, extracted files 953. `rust-toolchain.toml` was moved aside on the private copy. The git object stayed `c9901407d5fdc851007a5fd87852ec1efa418e52`. `RUSTUP_TOOLCHAIN=1.94.0`. The three CLI files were absent from the archive before and after the proof copy.

Frozen file SHA-256 from smoke, matched again on the proof tree:

| Path | SHA-256 |
|---|---|
| `crates/oo-gateway/src/policy.rs` | `68d6a1639263f6e7083eb44244360dfd22f73897fb3eff2570052da6b508188d` |
| `crates/oo-gateway/src/lib.rs` | `8a680dec57896e9a95fc8bdde7e353df8e8b770aeb67eb37d5ec34cc1faad10f` |
| `crates/oo-gateway/Cargo.toml` | `0b04620fe3e65760500e8fb907b49e3fb4689dc5e10e392fed269747e9ddeee3` |
| `Cargo.lock` | `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` |
| `contracts/gateway/column-acl-set-v1.md` | `476cfe39d70b53bd9cf1f62f5047c757e36aa276ff13ee0b62b1600ac17b3366` |
| `contracts/gateway/reviewed-policy-v1.md` | `de58f632be17f5e2f77e35660fcd949cb4b0da360eff6ecce9584d7d9a0aa53b` |
| `crates/oo-gateway/src/bin/query-pg.rs` | `431424a068a7184e71e9aa18ebf07240a03370e85e3626af2476946c3e196d11` |

`linux/smoke/lock-hash.txt` and `linux/proof/lock-hash.txt` both say `unchanged 16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`, memory 6442450944, NanoCpus 1000000000, CapDrop ALL, no host port, network `oo-a235-eef831aa-net`. Container `oo-a235-smoke` removed by the launch script. `docker-run.exit` 0. `smoke.exit` 0.

`cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1` wrote `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out`.

## Private PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a235-pg`, alias `oo-a235-pg`, database created by the test as `oo_a235`. `up.exit` 0. Host port bindings `{}`. Privileged false. The builder mount listed only `ca.crt`, `admin.pass`, and `login.pass`.

## Proof

Green container `91705c0611e6cfebc24a214f0d00cc8491854198f1bab678a136486f5e797f9f`, same Rust limits as smoke. `OO_A235_LIVE=1`. `docker-run.exit` 0. `proof.exit` 0.

`cargo test --locked --offline -p oo-gateway --test install_column_acl_cli -- --test-threads=1 --nocapture` wrote `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.76s`.

`cargo clippy --locked --offline -p oo-gateway --bin install-column-acl --test install_column_acl_cli -- -D warnings` finished with no warning. `clippy.exit` 0.

Attempt 1 is under `linux/proof/attempt-1/`. Its tests passed and Clippy exited 101 on unread `LedgerMap` fields. The green test asserts the ledger physical columns. That attempt's `console.log` was not overwritten.

Live oracle `live/oracle.txt`:

```text
malformed_scan=scan
malformed_request=request
malformed_roles=unchanged
stale=policy stale
stale_role=absent
bad_second=policy mapping
bad_second_role=absent
clerk_effect=installed
grantor=oo_fixture_admin
peek_effect=installed
clerk_replay=replay
replay_tables=same
column_grants_clerk=6
column_grants_peek=4
table_privileges=0
mapping_select=0
publication=0
role_shape=nologin
audit_wal=absent
authentication=not_claimed
query_party=1
hidden_write=42501
```

The evidence tree was scanned for both password files and did not contain them.

## Recipes

Origins are in `origin/recipes.txt`. Byte-identical copies: `tools/sha256.rs` blob `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, `tools/select-debs.rs` blob `565787c25c25bf451343cf4bc0431619c0611899`, `pg/pg-wrap.sh` blob `e12a368394c724f8f9d3436ad5f4873e139baa66`.
