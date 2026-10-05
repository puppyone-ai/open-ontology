# Column ACL set

Session `eef831aa-dba4-4af2-a78b-541bc9e0a218`. Task a218. One Rust builder and one private PostgreSQL. No subagents.

Author check of Frozen M1-D14A / SP06-W01. Verdict: AUTHORED, limited. This file does not accept M1-D14A. The acceptance line stays `未验收a218`. Counters were not edited. SP06-W01, M1-D14, M1-I01, and M1-I02 stay open.

## Commit

Product `543580768dec60d127eb428c09e0a9a400b18272`, parent `5394c70442f34dc54e023d2193d9a336193daa8a`, tree `ce4bb1a38305c672c906801a037af1bcd7fb9fcc`. Subject `gateway: install one column ACL set across mapped tables`. Author `Redux0223 <171701490+Redux0223@users.noreply.github.com>`. The parent subject is `evidence: record the a223 init-pg operator command`.

The commit changes only:

- `crates/oo-gateway/src/policy.rs`
- `crates/oo-gateway/tests/column_acl_set_pg_live.rs`
- `contracts/gateway/column-acl-set-v1.md`

`git diff-tree` shows `M` for `policy.rs` and `A` for the test and the contract. 2453 insertions, 8 deletions. `lib.rs`, `policy_catalog.rs`, `query_executor.rs`, `sql_guard.rs`, `Cargo.toml`, and `Cargo.lock` are absent from the commit.

Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, parent `cc3bb808c058cb66b088a76dd01725b918514909`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`. Saved in `source/COMMIT` before the overlay. Content SHA-256 of `policy.rs` at that archive and at the product parent is the same: `7557d789219bb0b86a8a071478e2e757df671f0b2636757014c3620c67d2fe60`. Git blob of the parent file is `5d881c0c263715c57bb1b09b4156180e21431d43`. `git diff --stat` from the archive to the parent is empty for `policy.rs`, `lib.rs`, `oo-gateway/Cargo.toml`, and `Cargo.lock`.

Blobs at the product commit, also `git hash-object` of the worktree files:

- `84a86e85ef82bc2a459a5e2faef5827d077ca6cc` `policy.rs` (47618 bytes), content SHA-256 `68d6a1639263f6e7083eb44244360dfd22f73897fb3eff2570052da6b508188d`
- `69897c46a1f3c5607936b0dffc3a63d3824ce2e4` `column_acl_set_pg_live.rs` (61937 bytes), content SHA-256 `68f96b3ce11c01f612329d150f64c88e1b087982bdd9a5a6e772156fad24d53b`
- `1bc75704e7930fea79b743c55a9f0721697fa3e2` `column-acl-set-v1.md` (5545 bytes), content SHA-256 `476cfe39d70b53bd9cf1f62f5047c757e36aa276ff13ee0b62b1600ac17b3366`

`linux/proof/product-sha256.txt` records the same `policy.rs` digest and the frozen companions. `lib.rs` `3e26f7495f3299e979b33e8cddd820951506afe3155d9c6c569037fa12c07dc7`. `policy_catalog.rs` `2920211ec2278d7d74a8022cdad166a11cf97b3eea3d258bb8cf91c131b0bd9d`. `query_executor.rs` `7604e5ee02bd493c5cc03e355c4c98f0c69f36bd601a07dcb6b76b67153df094`. `sql_guard.rs` `40ee2a5cd58d1c4c7460944312583ce69b20d2e8ce38fbcb8ff2bd648626df6c`. `oo-gateway/Cargo.toml` `0b04620fe3e65760500e8fb907b49e3fb4689dc5e10e392fed269747e9ddeee3`. `Cargo.lock` `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `pg-policy-v1.md` `481855d06bf2c03124af5886c9a6b9e5cf074a018e6ae3b7b8deb5291d7afce5`. `lock-hash.txt` says `unchanged` and that lock digest. `cargo-update-invoked.txt` says `no`.

The eight deletions extract the login lookup into `ensure_login`. `grant_membership` calls it, so `install_column_acl` still refuses a missing or superuser login at membership time. The set path also calls `ensure_login` before `CREATE ROLE`. The module comment now names the aggregate entry.

`install_column_acl_set` is `oo_gateway::policy::install_column_acl_set`. `lib.rs` already has `pub mod policy`. The prelude `pub use policy::{...}` was not edited and does not name the new items.

## Toolchain

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`, `--pull never`. Observed compiler: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, host `aarch64-unknown-linux-gnu`, Debian 13.4 (trixie), Linux `6.12.76-linuxkit`.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`, recorded in `pg/image.txt`. Container `oo-a218-pg` id `431d3dd9d2a0158a6563cefc9fc023aa352aa3346c7695e97223ef166b363272`. `pg/up.exit` is 0. `pg/server-san.txt` is `subject=CN=oo-a218-pg` and `DNS:oo-a218-pg`. Private keys stayed under `/tmp/oo-a218-eef831aa-dba4-4af2-a78b-541bc9e0a218/secrets`. They are not in this tree. A fixed-string scan of this directory and the three product files against `admin.pass`, `login.pass`, `ca.key`, and `server.key` printed no hits. No PEM private-key banner is in this directory. The query-pg requests contain the key `password_file` and not the password bytes.

Each Rust run used bridge `oo-a218-eef831aa-net`, `--cap-drop ALL`, memory 6442450944, `NanoCpus` 1000000000, `CARGO_BUILD_JOBS=1`, no host port, no host network, no socket, no host home, and no shared cargo cache. Cargo home and target were `oo-a218-eef831aa-cargo` and `oo-a218-eef831aa-target`. `/tmp` in the builder was tmpfs `rw,nosuid,nodev,noexec,size=1g`.

Smoke host config: container `52abe1a39ae87bd0736d1a6484dbeb6e8bb42ba3d6b3758da25c66a10009f7a1`, `Privileged=false`, `OOMKilled=false`, `CapDrop=["ALL"]`, `NetworkMode=oo-a218-eef831aa-net`, `PortBindings={}`. The launch script removed `oo-a218-smoke`.

Proof host config: container `54a2a10ddd2003a8ad00426df6326b50b8ff6a112ab1ef46304a57691d2e2bdc`, `Privileged=false`, `OOMKilled=false`, `CapAdd=null`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a218-eef831aa-net`, `PortBindings={}`. The launch script removed `oo-a218-proof`.

Proof capacity at launch: `host_cargo=0 rust_builders=0 global=0 cap=10`. Smoke capacity: `host_cargo=0 rust_builders=1 global=1 cap=10`.

Recipe origins and the edits made for a218 are in `origin/recipes.txt`. `origin/recipe-hashes.txt` is a draft made once with host `python3`. It is not an oracle. `pg/pg-wrap.sh` is the a200 file, content SHA-256 `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. The other copied origins are listed in that note, including a200 smoke launch `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163`, a200 proof launch `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`, a204 preoverlay launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`, and a199 preoverlay launch `bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55`.

## Pre-edit smoke

`linux/smoke` is a `git archive` of `c2db9c430b822a824a1062b6c3c7345924df7dd8` with no product overlay. `docker-run.exit` is 0. `console.log` ends `preoverlay_smoke=ok`. The archive has no `install_column_acl_set`. The smoke script refuses that name and does not run clippy.

`cargo fetch --locked` exited 0. `cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1` exited 0. Compile of the test profile finished in 2m 16s. The filter passed 5 tests in 0.00s, with 15 filtered out. `lock-hash` exited 0. `ls` printed `cannot access '/cargo-home/a218-clang-root/usr/lib/aarch64-linux-gnu/libclang.so*'` and `phase_exit=0 phase=libclang` still held. `libclang-19.so.19` is what the later load used.

## Proof

`linux/proof` is a fresh archive of the same commit plus the three owned files. `proof.exit` is 0. `docker-run.exit` is 0. `console.log` ends `proof=ok`. Every phase exit under `linux/proof/phases/` is 0: `smoke-gate`, `sha256-tool`, `source`, `product-hash`, `overlay-diff`, `frozen-match`, `libclang`, `lib-test`, `lib-count`, `test`, `test-count`, `oracle`, `clippy-component`, `clippy`, `lock-hash`.

`cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1` finished the test profile in 2.02s and passed 6 tests in 0.00s, 15 filtered out. The sixth test is `policy::tests::a_set_refuses_empty_mixed_duplicate_bounds_and_metadata`.

`cargo test --locked --offline -p oo-gateway --test column_acl_set_pg_live -- --test-threads=1 --nocapture` with `OO_A218_LIVE=1` finished the test profile in 6.53s. `two_tables_one_role_column_acl_set` passed in 0.99s. 1 passed, 0 failed.

`cargo clippy --locked --offline -p oo-gateway --lib --test column_acl_set_pg_live -- -D warnings` exited 0. `clippy.log` says the dev profile finished in 0.75s. That run reused the private target from attempt 3.

The live oracle is `live/oracle.txt`. Physical tables: party `pt_6708f7876808ea22372a5ec37c9a9f00623a6d12b6a6a0839a3788c9ff90`, ledger `pt_1785f5a09d6dbb961f09654175c1597fb1bb92649bb47de5c37933251d93`. `digest_a_max=7`, `digest_b_max=9`, `stale_cross_digest=StaleGeneration`. `legacy_second_table=GrantMismatch`, `legacy_set_widen=GrantMismatch`, `legacy_party_columns=1`. `bad_second_mapping=MappingAbsent`, `bad_second_role=absent`, `bad_type=TypeRefused`, `bad_type_role=absent`, `same_backend_after_rollback=true`. `clerk_effect=Installed`, `peek_effect=Installed`, `clerk_replay=Replay`, `old_api_one_table_replay=Replay`, `ledger_grants_after_old_replay=unchanged`. `partial_refuse=GrantMismatch`, `changed_refuse=GrantMismatch`, `membership_refuse=MembershipMismatch`, `grants_after_refuse=unchanged`. `membership=inherit=false set=true admin=false`, `membership_grantor=oo_fixture_admin`, `grantor=oo_fixture_admin`, `role_shape=nologin`, `table_privileges=0`, `public_column_privileges=0`, `metadata_select=false`, `schema_usage=usage_without_create`. `publication=0`, `authentication=not_claimed`, `rls=open`. `reltuples_before=unknown`, `reltuples_after=known`. The test calls `ANALYZE` after the grant checks and before `query-pg`. The installer does not.

`loader_mapping_grant=login_usage_select_execute` is a fixture grant on the login, copied from the reviewed `query_pg_cli.rs` helper, applied after the column-grant assertions. `install_column_acl_set` does not grant mapping `SELECT` to the login.

Clerk `query-pg` exit 0. The first stdout row is turn 0, int `1`, text `甲公司`, text `secret-note`. Count and star turns succeed. Peek exit 0. Peek stdout has `甲公司` and does not have `secret-note` or `memo-a`. Guard exit 6. `live/peek-peek-guard-audit.jsonl-stderr.txt` is `query-pg: guard` plus a newline. No guard audit file was written. Direct hidden reads and writes, and raw `SELECT *`, record `42501`. Reader backend pid 98.

## Preserved non-zero exits

| Path | Exit | Meaning |
|---|---:|---|
| `linux/proof/attempt-1/proof.exit` | 101 | `column_privileges` count for `_oo_pos` was 5. The rows were the table owner's. The assertion now counts fixture roles and `PUBLIC`. |
| `linux/proof/attempt-1/phases-run/test.exit` | 101 | Same panic at `column_acl_set_pg_live.rs:561`, left 5, right 0. Lib tests had passed. |
| `linux/proof/attempt-2/proof.exit` | 101 | Clerk `query-pg` exit 5. Stderr is `query-pg: policy` plus a newline, 17 bytes. |
| `linux/proof/attempt-2/phases/test.exit` | 101 | Catalog load runs as the `NOINHERIT` login before `SET ROLE`. The fixture mapping grant was added after this attempt. |
| `linux/proof/attempt-3/phases/test.exit` | 0 | Live test and oracle passed. |
| `linux/proof/attempt-3/phases/oracle.exit` | 0 | Required oracle lines were present. |
| `linux/proof/attempt-3/phases/clippy.exit` | 101 | `too_many_arguments` 8/7 on `register` at the then line 1424. The arguments moved into `MapKey`. No `allow` attribute. |
| `linux/proof/attempt-3/proof.exit` | 101 | Clippy failed, so the proof script did not write exit 0. |

Attempt logs, notes, and the partial live outputs stay in `linux/proof/attempt-1`, `attempt-2`, and `attempt-3`.

## Inventory

`MANIFEST.sha256` is a host `shasum -a 256` of every file in this directory except itself, written after this report, `FINDINGS.md`, and `HANDOFF.md`. It is not a cargo run and not a Python digest.

At the time these three reports were written, `oo-a218-pg` was still up. `cleanup/run.sh` had not run. Smoke and proof containers were already removed by their launch scripts.
