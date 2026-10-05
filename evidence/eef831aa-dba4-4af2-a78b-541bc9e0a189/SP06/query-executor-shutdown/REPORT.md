# a189 report — executor shutdown and role bind

Task `a189`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a189`. Repository `/Users/redux/puppy-one/open-ontology`.

Source commit `15d23fe9f2cbc74e3f3278d70d0f56390568126e` (`gateway: run one permission-bound SELECT on a dedicated session`). Parent `c77013d5c67830ff4a504eba096c84f645d41727`. Tree `feec821ab54f0aaf0001218f90150befce2803b2`. Fixed baseline `8a289f0c699abb8c707ca45ad45cb036abc3fc05` is an ancestor. The a184 review evidence is `df6067e9c6098f874b857d53fec139b78d6107c8`.

This is a repair of the existing trusted `SELECT` executor. It is not a new delivery count. Contract `contracts/gateway/query-executor-v1.md`. SP06-W04, SP06-W05, and SP06-W06 stay open. No release is claimed. No parent checkbox was changed. `UNTRUSTED_ADMISSION` stays `Blocked`.

The repair has two measured parts. Dropping a stream whose portal is blocked on `ACCESS EXCLUSIVE`, or inside `pg_sleep`, cancels that backend and completes `partial` in a few milliseconds with a confirmed rollback. The first syntactically valid role binds before the guard and before audit admission, and a later different valid role is `RoleReuse`.

## Source

The source commit changes three paths:

- `crates/oo-gateway/src/query_executor.rs`
- `crates/oo-gateway/tests/query_executor_pg_live.rs`
- `contracts/gateway/query-executor-v1.md`

`git show --stat` on that commit: 3 files, 902 insertions, 235 deletions. Content SHA-256, also in `source/blobs.sha256`:

```
9ec249b212abf35b13093c69f057b4003cd9fdc8b9d61a1be0a9a76828871be9  crates/oo-gateway/src/query_executor.rs
7ce7dc6576199d9c862da5da49e0ccc602d4e164af7bbd21b4f3b15acceff0f5  crates/oo-gateway/tests/query_executor_pg_live.rs
0fd1b730d75065080fc7d1a8b8b6455e41785994aff5fece8a4d4ee79ccaa34f  contracts/gateway/query-executor-v1.md
```

The baseline blobs used for the pre-edit smoke (`linux/smoke/owned-sha256.txt`) are `ed3e1c4331d58fe79c449cdb06c6c39a40992bf20e3273d244566761c0605a1b`, `d583b85be6259eb56916f75e56b94390373cac209a9a390d89da79f431b3a80b`, and `a186c1f229a1dbdfcf070d32a09ee89511c694f7ad678a8456cf471331ff9146`. Exported blobs under `source/blobs/` match the source commit. The worktree copies of those three paths match that commit. `crates/oo-gateway/src/lib.rs` and the parser-worker files were not part of this commit.

## Archive and pre-edit smoke

`git archive` of `8a289f0c` extracted under the task temp directory. `linux/smoke/archive-verify.txt` records `blobs=25676 missing=0 mismatch=0`. Each build copied only the three owned paths onto that archive. `linux/smoke/overlay-scope.txt` and `linux/proof/overlay-scope.txt` both say `lib.rs=baseline` and name those three files. The archive `lib.rs` stayed the baseline export. A moving parser-worker file in the workspace could not enter the build.

One Linux arm64 smoke container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, id `9f284eedb94aec820f59656c0e3802c7daa08ec8cf2f84483551b2ff063a5602`, name `oo-a189-smoke`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Private bridge `oo-a189-net` (`8c7528549eb2e58fab458420a7aaaf1f211a17fd1cf550de49c8a81b2fc03923`), cargo volume `oo-a189-cargo`, target volume `oo-a189-target`. `CapDrop=["ALL"]`, `CapAdd=null`, privileged false, memory 8g, no published port, `HOME=/tmp`. `linux/smoke/capacity.txt` recorded host cargo 0, rust builders 0, global 0, cap 3.

`linux/smoke/smoke.exit` is 0. Phase exits, each 0: `libclang`, `libclang-load`, `fetch`, `lib-test`, `clippy-component`, `clippy`, `unsafe`. Library tests: 10 passed, 0 failed (5 policy, 5 `query_executor`). Clippy `-D warnings` on `--lib --test query_executor_pg_live` exited 0. This smoke ran on the baseline overlay, before the repair. libclang was extracted into the cargo volume. The pinned image does not contain clippy; the smoke downloaded that component into the container layer.

## Baseline reproduction

One later container, id `37e26ce77b6a9bbf2f67bf327fc9e88c6a5862bd2d02a472ad18b10a75779f83`, name `oo-a189-repro`, same image and network, baseline executor hash `ed3e1c4331d58fe79c449cdb06c6c39a40992bf20e3273d244566761c0605a1b`. `linux/probe/repro.exit` is 0. `linux/probe/oracle.txt` is the deciding baseline observation. PostgreSQL 17.10, `ssl=on`.

After a guard refusal of `INSERT`, `bound_role_after_guard=none`, `guard_admits=0`, and the session stayed open. A different valid role then ran: `other_role=allowed`, 1 row, status `succeeded`, bound `oo_r_west`, admits 1.

Drop during `pg_sleep` returned in 4 ms as `partial`, SQLSTATE absent, stop `protocol`, rollback `no`, socket dropped. Drop while `ACCESS EXCLUSIVE` was held returned in 5703 ms as `failed`, SQLSTATE `57014`, stop `timeout`, rollback `yes`. That elapsed is the server timer. `linux/probe/attempt-1/` is preserved: the harness panicked with `locked query never became active` (`repro.exit` 101) before it measured the lock drop. Its oracle still records the portal drop at 4 ms and `other_role=allowed`.

## Private proof

One later container, id `e5bc78ad7d9dd44cb4ee8357b5faf0e2108d9ff257e61aa41b29a983f40a70c6`, name `oo-a189-proof`, same image, `OO_A189_LIVE=1`. The overlay hashes match the source commit. `linux/proof/proof.exit` is 0. `linux/proof/capacity.txt` again recorded host cargo 0 and rust builders 0.

Phase exits, each 0: `unsafe`, `libclang`, `lib-test`, `clippy-component`, `clippy`, `live`. Library tests: 11 passed, 0 failed, including `role_bind_is_permanent_for_a_valid_role_only`. Clippy `-D warnings` finished in 13.62 s. The live test `live_permission_bound_select` passed, 0 failed, finished in 15.51 s. `live/oracle.txt` ends with `live_result=compared`.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a189-pg`, id `e0dbba5a73494d5290dc37f69d86e009d8ed861bf6c466f5ff5e621cd07571ed`. `pg/version.txt` is `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Privileged false, network `oo-a189-net`, `pg/ports.json` is `{}`. `CapDrop` is `ALL` and `CapAdd` is `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. TLS server certificate SAN is `DNS:oo-a189-pg`. `pg/up.exit` is 0. Passwords and private keys stayed under the task temp directory. A scan of 180 evidence files against the fixture password files found `leak_hits=0`.

The deciding oracle, after a query had been active for 3100 ms under an 8000 ms wall:

- lock drop: 3 ms, `partial`, SQLSTATE absent, stop `protocol`, rollback `yes`, socket `yes`, end audit `yes`
- `pg_sleep` page drop: 6 ms, `partial`, SQLSTATE absent, stop `protocol`, rollback `yes`, socket `yes`, end audit `yes`

Both backends were still active at 3100 ms, so the server limit had not fired before 3 s. A 2000 ms wall on a lock completed `failed` / `Timeout` with SQLSTATE `57014` at 1796 ms. The same wall on `fixture.page_hold()` completed the same way at 1798 ms. The handle from that finished query was `not_sent` against the next backend, which stayed active for another 400 ms. A successful query's drop left the next `page_hold` active; that next drop was `partial` at 6 ms. An in-flight `CancelHandle` stayed `failed` / `Canceled` with SQLSTATE `57014`. An immediate drop whose complete hook failed stayed `partial` with `end_audit_ok=false`.

Role timing on the same run: a guard refusal of `INSERT` bound `oo_r_reader_a` and the other valid role was `RoleReuse`; the same role then ran. An invalid shape, an identity mismatch, and `max_rows` 2001 did not bind. A `LOGIN` flag-check failure closed the socket, and the next call was `SessionClosed`. An admit failure bound the role and then refused the other role with the audit count unchanged. Details are in `FINDINGS.md`.

`linux/proof/attempt-1/` is preserved (`proof.exit` 101, rustc `E0106` on `open_read_only`). `linux/proof/attempt-2/` is preserved (`proof.exit` 2, `clippy component missing`, after 11 library tests passed). The deciding proof logs remain in `linux/proof/`.

## Cleanup

`cleanup.txt`. Own containers, volumes, network, and `/tmp/oo-a189-eef831aa-dba4-4af2-a78b-541bc9e0a189` are removed there. The exited container `oo-a190-rust194` was left in place. Failed attempts remain in this directory. `docker prune` was not used. Images were not removed.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list.
