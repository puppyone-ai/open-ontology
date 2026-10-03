# Physical mapping report

Source commit `b5415544c9eb8d608eafd411884627dda9ee7c84`. Fixed archive base `02b9847159a68ced1443615cbb47bcb5872724b0`. Green run `87585907-7e08-4e9e-9bb0-23a379b98e3c`.

`0003_target_segment_preflight_expand.sql` stays the segment preflight migration. The binding is `crates/oo-pipeline/migrations/0004_physical_mapping.sql`, version `physical-mapping/v1`. Logical names stay complete `pgident/v1` strings. The new table stores the PostgreSQL identifier.

## Images and archive

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Both were already local. Pull was never. Platform `linux/arm64`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Server `17.10 (Debian 17.10-1.pgdg12+1)`.

Archive tar SHA-256 `00c699e87294b6c28de1abdfd070ddded4b39f5520ceccea0f6b31ae88b8e2bc`. Verification: `files=728`, `links=0`, `mismatched=0`, `overlay=5`. Frozen migration hashes inside the container matched `0000` `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208`, `0001` `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`, `0002` `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512`, `0003` `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`. `0004` file SHA-256 `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95`.

Capacity before the green launch: `builder_containers=0 host_cargo=0 own_postgres=0`.

## Commands

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test physical_mapping_live physical_mapping_live -- --ignored --exact
```

The live test ran only after fetch, the package test, and Clippy had all exited 0. `inside.sh` `elapsed_seconds=4`. The live result line is `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.16s`. That line occurs once in `linux/fixed/live-console.log`.

## Green exits

| file | exit |
|---|---|
| fetch.exit | 0 |
| test.exit | 0 |
| clippy.exit | 0 |
| live.exit | 0 |
| worst.exit | 0 |
| docker-run.exit | 0 |
| restart.exit | 0 |
| clippy-component.exit | 0 |

Package test results in that run: library `7 passed`; `physical_mapping_live` counted `1 passed` and `1 ignored` on the non-ignored invocation; the ignored invocation then passed the one live test.

After the builder exited 0, `docker restart` brought the same Postgres postmaster back. A new `psql` session read the golden row. `restart-compare.txt` says `match=1`, physical name `pn_d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1f`, digest `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`, server `17.10 (Debian 17.10-1.pgdg12+1)`.

## Oracles

`sha-oracles.txt`: empty `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, abc `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, a*200 `c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5`, golden payload `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`.

`cases.log` ends with the green sequence: long pair physical names `pn_f5cb921527f810b82edcb1508ad40c4338be0e3b819b87f3e329c8d03743` and `pn_dac559f034005c71ce1d7919f73a140f58adfa8e9d38294e0be8d5c4dea4`; unicode logical bytes 482; closed payloads `23514` with a 4096-byte name stored; absent generation `OO024`; column `pc_48f7de829df96644ef7e4fe0ae0f8e9b5a56fadd98e261cc6b2bf524f7bd`; conflict `OO022`; rollback `OO024`; concurrent `inserted+replay` and one row; agent and runtime deny `42501`; direct write `OO014`; fence `OO011` with `apply_receipts` count 0 and public user tables 0; second apply skip and checksum mismatch `OO001`; `live=ok`.

`functions.txt`: lookup is security definer, owner `oo_target_owner`, `strpos` of `pm_sha256` in its source is 0. `pm_sha256` is security invoker. Register is security definer and its source contains `pm_sha256` at offset 2586. `privileges.txt` is `false false false true false false` for agent select, runtime select, agent execute, runtime execute, runtime hash execute, and public execute.

## Earlier exits

The same slot was reused after each failure. Cargo and target volumes stayed until the green cleanup. Each failed run removed its Postgres volume.

| attempt | test | clippy | live | docker |
|---|---|---|---|---|
| 1 compile shadow of `physical_name` | 101 | 101 | 9 | 101 |
| 2 `clippy::useless_vec` | 0 | 101 | 9 | 101 |
| 3 locale column was `datcollate` | 0 | 0 | 101 | 101 |
| 4 plpgsql variable `offset` | 0 | 0 | 101 | 101 |
| 5 `pg_catalog.position(... in ...)` | 0 | 0 | 101 | 101 |

Raw consoles for those attempts are `linux/fixed/attempt-1` through `attempt-5`. `cases.log` is opened with append, so it also keeps notes from attempts 3 through 5 ahead of the green block.

## Cleanup

Green cleanup exits: container rm 0, postgres rm 0, network rm 0, cargo volume rm 0, target volume rm 0, postgres volume rm 0. No `oo.owner=a140` container or `oo-a140-eef831aa-dba4-4af2-a78b-541bc9e0a140` volume remained. Host `cargo`/`rustc` count was 0.
