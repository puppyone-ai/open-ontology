# a241 review of install-column-acl

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a241`. Fixed commit `13566602dc36ce6ebd03fef25a9bd17aae7a281c`. Parent `8a758c17c51e88d64003ac27c7c3f65d48bbc4ff`.

Verdict: **APPROVE_LIMITED**.

The compiled `install-column-acl` binary from that commit installed two roles over two tables on a private TLS PostgreSQL 17.10, replayed the same clerk document, refused a stale generation and a bad second table, and the login received SQLSTATE 42501 for the hidden writes. After the fixture issued mapping-read grants and `ANALYZE`, compiled `query-pg` returned the fresh typed rows. A paste of the compiled `reviewed-policy` policy array was refused `policy stale` and left the clerk grants unchanged.

This review does not accept M1-D14D, the parent goal, or a release. No product, library, manifest, or lock file was edited. There is no `AGENTS.md` at the repository root.

## Owned product at the fixed commit

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/bin/install-column-acl.rs` | `6750e119476dca08b1acecbfb6397204cb2ad70a` | `9733c33e4ef7ec4e47489db23039bd01bfa3cf9f0f46b389820da32e27367c55` |
| `crates/oo-gateway/tests/install_column_acl_cli.rs` | `2514cc23efd8fba6a320372f4fdcb75129d90cdb` | `1a0cdfff01a5d965cd7c6ce49786d4bb874204badc09f2633e1c451aaa158af9` |
| `contracts/gateway/install-column-acl-cli-v1.md` | `772b1e2c11056eb869e6d9efda2c0228d7c42ef1` | `d0df87be3cd4e0bd49c8b23a9c17bd7a241445d2c704affa5b9110fe0f639eb4` |
| `crates/oo-gateway/src/bin/query-pg.rs` | `b91238462eb5097de0059ec75e73776ccb2488d0` | `431424a068a7184e71e9aa18ebf07240a03370e85e3626af2476946c3e196d11` |
| `crates/oo-gateway/src/bin/reviewed-policy.rs` | `34f7824250c39a58f177e70f21dc6db02b5527bd` | `bb5005880446e8924ea2b7839d5a4fc5c3f77dcc5ee9e8022631058a6b2cdf1d` |

`linux/smoke/archive-blobs.txt` matches those git blobs to the extracted archive. SHA-256 values are `linux/smoke/owned-sha256.txt`, matched again by `linux/overlay/phases/frozen-match.exit` 0. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

The review test is not in the commit. Its git blob is `1a945ebeb88d2f4fee461f27051f0bdc5cd5e917`. Its proof SHA-256 is `856398d8c02b13082d419de61aff18e2e78531ae10be07bdabca472b5a08e858`.

## Archive and recipes

Partial archive of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` from `13566602dc36ce6ebd03fef25a9bd17aae7a281c`. Listed files 959, extracted files 959. `rust-toolchain.toml` was moved aside on the private copy. The git object stayed `c9901407d5fdc851007a5fd87852ec1efa418e52`. `RUSTUP_TOOLCHAIN=1.94.0`.

Recipe origins and the a241 deltas are in `origin/recipes.txt`. Launch and report shape follows a235, a204, a200, and a199. The archive is the fixed commit itself. The smoke tree does not contain `install_column_acl_a241_review.rs`. The overlay copies only that file.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`. Container `oo-a241-smoke` id `85cdf7fa46967327f607b75b24456af6b798173f73a0b93d6655ec39795284c2`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, port bindings `{}`, network `oo-a241-eef831aa-net`. The launch script removed the container. `docker-run.exit` 0. `smoke.exit` 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`cargo test --locked --offline -p oo-gateway --test install_column_acl_cli -- --test-threads=1` wrote `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`. `OO_A235_LIVE` and `OO_A241_LIVE` were unset, so the committed live test returned before opening a connection. No `live/oracle.txt` was written. Capacity before launch: `host_cargo=0 rust_builders=0 global=0 cap=10`.

## Private PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Container `oo-a241-pg` id `24893364fb964eff7dded51e3ff4136680e5add636a500f51b2764031886502a`, alias `oo-a241-pg`, database `oo_a241` created by the review test. `up.exit` 0. Privileged false. Host port bindings `{}`. `autovacuum=off`. The builder mount listed only `ca.crt`, `admin.pass`, and `login.pass`.

## Overlay proof

Container `oo-a241-proof` id `64b599bdafcb6ed3bc0fa4e7ccb12af0aba108ac9034c2db8a4648992d425234`. Same Rust limits. `OO_A241_LIVE=1`. `docker-run.exit` 0. `overlay.exit` 0. Capacity before this launch: `host_cargo=0 rust_builders=0 global=0 cap=10`.

`cargo test --locked --offline -p oo-gateway --test install_column_acl_a241_review -- --test-threads=1 --nocapture` wrote `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s`.

`cargo clippy --locked --offline -p oo-gateway --bin install-column-acl --test install_column_acl_a241_review -- -D warnings` finished with no warning. `clippy.exit` 0.

Fresh rows are party `(41, ada, kept-local)` and `(42, bea, other-local)`, ledger memos `memo-seven` and `memo-eight`. `query-pg` stdout `live/query-clerk.stdout` has those two row lines and:

```text
{"record":"summary","turn":0,"status":"succeeded","truncated":false,"returned_rows":2,"result_bytes":31,"rollback_confirmed":true,"socket_dropped":false,"end_audit_ok":true,"original_sha256":"194eaf83b751e805b4f1e35213b69089cf0a773efb9ae209a364e7fcf4ee9b20","executed_sha256":"c2c657d39e0546a0fdeaa6452adf2db754703a79021d34bab0b556a7b4ca340c","stop":null,"sqlstate":null}
```

Live oracle `live/oracle.txt`:

```text
stale=policy stale
stale_role=absent
bad_second=policy mapping
bad_second_role=absent
clerk_effect=installed
grantor=oo_fixture_admin
replay_effect=replay
replay_tables=same
peek_effect=installed
column_grants_clerk=6
column_grants_peek=4
table_privileges=0
role_shape=nologin
allowed_party_id=41
hidden_write=42501
mapping_before=0
mapping_grant=explicit
reltuples_before=unknown
query_before_analyze=policy
reltuples_after=known
query_after_analyze=succeeded
query_rows=2
query_receipt=local
audit_wal=not_claimed
authentication=not_claimed
publication=not_claimed
rls=not_claimed
producer_effect=policy stale
producer_roles=unchanged
producer_grants=unchanged
```

## Failed attempts

Attempt 1 (`attempt-1/`) stopped in `producer_catalog` with a `json!` recursion limit. Exit 101. No oracle.

Attempt 2 (`attempt-2/`) completed install, replay, 42501, the mapping grants, `ANALYZE`, and `query-pg`. The producer assert then treated the captured-pg-draft evidence envelope as an owner leak. That envelope keeps the source catalog. Exit 101. Oracle through `rls=not_claimed` is preserved.

Attempt 3 (`attempt-3/`) repeated that oracle through `producer_effect=policy stale` and failed Clippy on `collapsible_if` in the harness `bin_path`. `test.exit` 0. `clippy.exit` 101.

Each attempt kept its `console.log`. The green console is `linux/overlay/console.log`.

## Cleanup

`cleanup/run.sh` removed `oo-a241-pg`, volumes `oo-a241-eef831aa-cargo`, `oo-a241-eef831aa-target`, and `oo-a241-eef831aa-pgdata`, network `oo-a241-eef831aa-net`, and `/tmp/oo-a241-eef831aa-dba4-4af2-a78b-541bc9e0a241`. Smoke and proof containers were already gone. No prune. Foreign containers were left in place. Transcript: `CLEANUP.txt`.
