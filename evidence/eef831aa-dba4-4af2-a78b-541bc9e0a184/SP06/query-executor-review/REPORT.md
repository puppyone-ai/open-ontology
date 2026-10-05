# a184 report — SELECT executor review

Task `a184`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a184`. Repository `/Users/redux/puppy-one/open-ontology`. Fixed source `8a289f0c699abb8c707ca45ad45cb036abc3fc05` (`gateway: run one permission-bound SELECT on a dedicated session`), parent `5abe6bdcfa75c9b6f70a7f756481bb25ddc193d9`. Author evidence commit `ce040593f6af6a61a5c3e29abd494b1a863f32c5`. Tree `fbbfe506668cf4263d670a0909c1d5b664f7c81d`.

Verdict: **APPROVE LIMITED**. Contract `contracts/gateway/query-executor-v1.md`. SP06-W04, SP06-W05, and SP06-W06 stay open. No release is claimed. No parent checkbox was changed.

The review read `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a180/SP06/query-executor/REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`, the fixed executor and contract, the a179 approval of `03512f91324ac33e772e9702fe806e8e04037ec4` (that commit is an ancestor), and `docs/07-gateway.md` (the 30 s query default at line 139, and rule R4 at line 100).

## Archive

`git archive` of `8a289f0c` extracted to `/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184/archive`. `linux/smoke/archive-verify.txt` records `blobs=25676 missing=0 mismatch=0`. Source blobs of the contract, `lib.rs`, `query_executor.rs`, and `tests/query_executor_pg_live.rs` are under `source/blobs/` and match `source/blobs.sha256`. The probe copy compared those 25676 blobs again (`changed=0`) and added only `crates/oo-gateway/tests/query_executor_a184_review.rs`. The immutable archive never contained that file. Product, lock, and gate files were not edited.

## Pre-overlay smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, id `44ee0c7baa71ef5db6fa444804095a4d157e497028393f0cc3896549c1611eb2`, name `oo-a184-smoke`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Private bridge `oo-a184-net` (`125c1ea62c9a`), cargo volume `oo-a184-cargo`, target volume `oo-a184-target`. `CapDrop=["ALL"]`, `CapAdd=null`, privileged false, memory 8g, no published port, `HOME=/tmp`. `linux/smoke/capacity.txt` recorded host cargo 0 and rust builders 0.

`linux/smoke/smoke.exit` is 0. Phase exits, each 0: `libclang`, `libclang-load`, `fetch`, `lib-test`, `clippy-component`, `clippy`, `unsafe`. Library filter `query_executor`: 5 passed (`bootstrap_oids`, `cancel_gate`, `byte_cap`, `error_codes`, `role_shape`). `OO_A180_LIVE` was unset. `tests/sql_guard.rs` was not run. Clippy `-D warnings` on `--lib --test query_executor_pg_live` exited 0. `linux/smoke/overlay-absent.txt` is `absent`. `linux/smoke/allow-sites.txt`: `run_producer` 12 arguments, `drive` 10, `read_pages` 8, each with the allow attribute; `classify_pg(..., false)` has 7 call sites. libclang was the tree already extracted into the cargo volume at `/cargo-home/clang-root/usr/lib/llvm-19/lib`. This smoke finished before the review test was copied onto the probe tree. `linux/smoke/attempt-1/` is the preserved pre-cargo failure.

## Fresh oracle

One later container, id `dcc16fe41cb7543e7aad52ebccfbfa121727711250ab65803a6262a09457cd79`, name `oo-a184-probe`, same image, same network, same volumes, `CapAdd=null`. `OO_A184_LIVE=1`. `linux/probe/probe.exit` is 0. `oracle-test.exit` is 0: 1 passed. `oracle-scan.exit` is 0 (`oracle_scan=clean lines=85`). `clippy.exit` is 0.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a184-pg`, id `085949aac6f97ea5b539e802052fcf370c63c3cbe1054884ca474a2d543f3134`. `pg/version.txt` is `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Privileged false, network `oo-a184-net`, `pg/ports.json` is `{}`. `CapDrop` is `ALL` and `CapAdd` is `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. TLS server certificate SAN is `DNS:oo-a184-pg`. Passwords and the private key stayed under `/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184/secrets` and are not in this directory.

The fixture is `depot.pallet` with roles `oo_r_a184_east` and `oo_r_a184_west`, not the author's shipment fixture. `live/oracle.txt` is the deciding run. Typed int2/int4/int8 extrema, empty text, unicode, a newline, bool, and null compared equal to the database. Numeric OID 1700 and varchar OID 1043 were refused with call count 0. The whole-query client deadline delivered 1 row at 4543 ms with SQLSTATE absent. A separate server timeout kept SQLSTATE 57014 at 1590 ms and 0 rows. A late cancel was `not_sent` and the next query succeeded. An in-flight cancel was `sent`, SQLSTATE 57014, status failed. Immediate drop completed `partial`. Drop during `ACCESS EXCLUSIVE` completed `failed` at 2713 ms with SQLSTATE 57014. Details are in `FINDINGS.md`.

`linux/probe/attempt-1/` through `attempt-7/` are preserved harness misses. The deciding probe logs remain in `linux/probe/`.

## Cleanup

`cleanup.txt`. Own containers, volumes, network, and `/tmp/oo-a184-eef831aa-dba4-4af2-a78b-541bc9e0a184` are removed there. Failed attempts remain in this directory. Other tasks' containers were not removed. `docker prune` was not used.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list.
