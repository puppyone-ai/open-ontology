# a179 report — SQL admission review

Task `a179`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a179`. Repository `/Users/redux/puppy-one/open-ontology`. Fixed source `03512f91324ac33e772e9702fe806e8e04037ec4` (`gateway: admit one SELECT through the PostgreSQL parser`), parent `93e6b6a24fab71c10dd9966ee28d5258516bed63`. Author evidence commit `957a807610b8f330c64f5b92e9d94f927fdae532`.

Verdict: **APPROVE LIMITED**. Contract `contracts/gateway/sql-guard-v1.md`. SP06-W02 stays open. No release is claimed. No parent checkbox was changed.

The review read `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a175/SP06/sql-guard/REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`, the fixed diff (6 files, `Cargo.lock`, the contract, `oo-gateway` manifest, `lib.rs`, `sql_guard.rs`, `tests/sql_guard.rs`), the contract, and `docs/07-gateway.md` section 4.2 rules R1–R9.

## Archive

`git archive` of `03512f91` extracted to `/tmp/oo-a179-eef831aa-dba4-4af2-a78b-541bc9e0a179/archive`. `linux/smoke/archive-verify.txt` records `blobs=24805 missing=0 mismatch=0`. Source blobs of the six committed paths are under `source/blobs/` and match `git show 03512f91:path` byte for byte. `source/blobs.sha256` is the SHA-256 of those files. The probe copy compared those 24805 blobs again (`changed=0`) and added only `crates/oo-gateway/tests/sql_guard_a179_review.rs`. The immutable archive never contained that file. Product, lock, and gate files were not edited.

## Pre-overlay smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, id `0ae8c12495bcedfb1e2f32f7d8c0c86cb725bfbb55c1ca85bb8d59ff344a60d3`, name `oo-a179-smoke`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Private bridge `oo-a179-net`, cargo volume `oo-a179-cargo`, target volume `oo-a179-target`. `CapDrop=["ALL"]`, `CapAdd=null`, privileged false, memory 8g, no published port, `HOME=/tmp`, tmpfs `/tmp` `noexec`. Host cargo processes were 0 and no other rust builder was running.

`linux/smoke/smoke.exit` is 0. Phase exits, each 0: `libclang`, `libclang-load`, `clippy-component`, `fetch`, `lock-hash`, `crate-manifests`, `lib-test`, `sql-guard-test`, `clippy`, `arch-check`, `unsafe`. Library tests: 5 passed. Author `tests/sql_guard.rs`: 24 passed, live path not run (`OO_A175_LIVE` unset). Clippy `-D warnings` and `oo-arch-check` passed (`architecture check: pass`). `linux/smoke/overlay-absent.txt` is `absent`. This smoke finished before the review test existed.

Native `pg_query` 6.2.1 compiled in that run. The eight recorded Debian debs were checksum-checked and extracted. `libclang-ldd.txt` shows `libLLVM.so.19.1` and `libz3.so.4` resolved from the private tree. No apt and no added capability.

## Fresh oracle

One later container, id `e08831a671274517255b56ef18f3c9cf2fa6b82c3810fc090b56ad9b45880fe4`, name `oo-a179-probe`, same image, same network, same volumes, `CapAdd=null`. `OO_A179_LIVE=1`. `linux/probe/probe.exit` is 0. `oracle-test.exit` is 0: 5 passed. `clippy.exit` is 0.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a179-pg`, id `ad084b3e43d64f32410dda0d3d6d7664bb4c383a25b8b61884c3553af0df7aff`. `pg/version.txt` is `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Privileged false, network `oo-a179-net`, no published port. `CapDrop` is `ALL` and `CapAdd` is `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, which the official entrypoint needs in order to start. The rust builders did not receive those capabilities. TLS server certificate SAN is `DNS:oo-a179-pg`. Passwords and the private key stayed under `/tmp/oo-a179-eef831aa-dba4-4af2-a78b-541bc9e0a179/secrets` and are not in this directory. `live/oracle.txt` has no password and no hidden column name.

The shadow role `oo_a179_shadow` executed the guarded join and the guarded star. Results matched the administrator read (`west`, `ada`, and star `42` / `west`). Hidden column and insert returned `42501`. The logical name returned `42P01`.

## Dependency delta

`linux/lock/added-pairs.txt` lists the 35 new name/version pairs. `provenance.txt` records registry source and checksum for each. Removed pairs: 0. Checksum changes of existing pairs: 0. The author ran a precise `cargo update -p pg_query --precise 6.2.1` despite the a175 `cargoupdatenone` instruction. That deviation is recorded in `FINDINGS.md` section F4. This review did not rewrite the lock. `cargo deny` was not run.

## Cleanup

`cleanup.txt`. Own containers, volumes, network, and `/tmp/oo-a179-*` are removed there. Failed probe attempts remain in this directory. Other tasks' containers were not removed. `docker prune` was not used.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list.
