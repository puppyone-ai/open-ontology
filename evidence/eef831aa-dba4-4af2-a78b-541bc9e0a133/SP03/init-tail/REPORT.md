# a133 init_mut receive tail

Session `eef831aa-dba4-4af2-a78b-541bc9e0a133`. Fixed input `79515fcf7e7032c8c212540eaae4b48a5957e3a2` (tree `b78f84497484f6232aecf0b51ffcc6d27be9fd7c`). Source commit `bd77b2520bcc3d63517bc49309002f25b58544b1` on parent `6a6d2166ae6843a1d9bee043f1b3f4443fb30fbd`. That commit contains only:

- `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` `b56fe942de40260bfaf39e83f32af8630ded0d739bc758480940099124cfe7d5`
- `crates/oo-connectors/vendor/PATCH.md` `20a4e7f4c6ecccedbd82d3d26f5de596d6a306935852ac26f1d672fe73187ab6`
- `crates/oo-connectors/tests/pg_wire_tls_tail.rs` `3f1349747e156999bbb96903aea9ce2401196db7cb854c4d44d514f5585e1721`

The proof compiled a git archive of `79515fcf` plus those three files. `stream.rs` was absent from that snapshot. The Arrow snapshot stream on later commits was left untouched.

Images, both linux/arm64, `--pull never`:

- Rust 1.94.0 `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- PostgreSQL 17.10 `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Builders used one container at a time, `--cap-drop ALL`, no new privileges, memory 8g, 4 cpus, bridge `oo-a133-eef831aa-net`. The database used the same bridge, memory 1g, no host port, caps `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. `OOMKilled=false` on the green clippy run and the green TLS read.

## What passed

Pre-edit smoke, not repeated: lib 55 and scoped clippy, exits 0. Empty-catalog fixture sha256 `044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c`.

On the patched snapshot, not repeated after they went green:

- `oo-connectors` lib 55, bounds 13, accounting 12, portal 6, plaintext view 1. Exits 0.
- `pg_wire_tls_tail` deterministic tests, 8 passed, the TLS test filtered. Nocapture peaks: append, drop, retain, eof, oversize `8192`; cancel after `init_mut` and no-data cancel `16384`. All under the 24 KiB plateau. bytes lock identity `1.12.1` checksum `fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`. Product blob sha256 matches `buffered.rs`.

Embedded `sqlx-core` tests, isolated copy outside the workspace, bytes pinned `=1.12.1` with that checksum: `committed_init_len_matches_the_read_contract` and `init_mut_small_frames_shrink_the_receive_view` passed, along with the two pre-existing lib tests (`test_read_buf_bytes_mut`, `assert_pool_traits`). 4 passed, 0 failed.

Scoped clippy `cargo clippy -p oo-connectors --all-targets -- -D warnings` exit 0. The 1.94.0 clippy component was installed in the container the same way smoke installed it. `sqlx-postgres` still warns that two `#[expect(dead_code)]` lints are unfulfilled. Those warnings are in the dependency and did not fail the build.

Private TLS read, verify-full, `fetch_all` of `SELECT id FROM oo_a133.small ORDER BY id`, 60000 rows:

- count 60000, order 1..=60000, sum `1800030000`, bit_xor `60000`
- row SHA-256 of big-endian i32 ids: `954a76695912c8cb349b45dee86d4fccf0e278ffd20545a3e5beaf0c4b0b9533`
- log line `a133_pg_hash=b504c603796248be6f29c923aae8e57b6f8c01b235cea61b161b00ac834a5b60` is SHA-256 of that 32-byte digest
- `a133_pg_view_peak=8192`, `a133_pg_rss_kib=14384`
- reader log lines counted by the test: 2, write lines 0
- reader statements: `SELECT id FROM oo_a133.small ORDER BY id` and the count/sum/bit_xor select
- test exit 0 in 0.41s, container exit 0

## Failures kept

| Attempt | Exit | Result |
| --- | --- | --- |
| Proof 1, cargo flag before the subcommand | 1 | No tests ran. `proof/console-attempt1.log` |
| Proof 2 | 101 | Lib, bounds, accounting, portal, and view passed. Tail failed to compile: `sqlx::query` requires `'static` SQL. `proof/attempt2/` |
| Proof 3 | 101 | Tail tests passed. `cargo test -p sqlx-core --features ...` rejected features for a non-member. `proof/attempt3/` |
| Rest | 101 | Tail nocapture passed. `cargo test -p sqlx-core` refused because the package has dev-dependencies and is not a workspace member. `proof/rest-console.log`, `proof/attempt4/` |
| Embed lock | 101 | Offline resolution looked up optional `async-fs`, which is not in the warm cache. `proof/attempt5/` |
| Embed tests then clippy | 1 | Embedded tests passed. `cargo-clippy` was missing from the fresh image. `proof/embed-console.log`, `proof/attempt6/` |
| TLS read 1 | 101 | Reader `SET log_statement` is superuser-only. `proof/attempt7/` |

The workspace root, `Cargo.toml`, and `Cargo.lock` were not edited. `sqlx-core` was not added as a workspace member.

## Gate

The production bounded driver integration gate stays closed. This run does not open it. An independent review of `bd77b252` is still required. This report does not claim the whole reader.
