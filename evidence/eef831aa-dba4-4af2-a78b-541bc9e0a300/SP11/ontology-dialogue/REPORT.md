# Ontology dialogue

Session `eef831aa-dba4-4af2-a78b-541bc9e0a300`. Fixed archive `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. Owned paths are `tools/ontology-dialogue/**` and this evidence directory. The root `Cargo.toml` and `Cargo.lock` were not edited. The dialogue lock is the committed subset already accepted by `cargo fetch --locked` in proof-2 and later. Its SHA-256 is `724c2850cde36b8ebd76b098a44117fc4d1999022896acf1d9a71a309873e595`.

This packet records the trusted-operator command `ontology-dialogue`. It does not accept the parent ledger, a release, hosted Gateway authentication, untrusted parser admission, `runsc`, or an NL harness.

## Command

`ontology-dialogue propose --config <path>` sends one natural-language question and one role Pack's public files to one approved HTTPS chat completion. The model returns one SQL candidate. That SQL is untrusted and is not passed to `execute_trusted_select`.

`ontology-dialogue execute --config <path>` starts the compiled `query-pg` binary only after a local approval file names the current proposal hash, SQL hash, tuple hash, and Pack digest. The approval is a local operator declaration. `authenticated_audit` stays false. Entailment and warning comprehension stay `not_judged`. The database password stays in the query-pg request password file. This command does not open that file. The bearer stays in its file and in the Authorization header.

The citation object is the `answer-citations` report built from the query-pg audit receipt. `query_id` is copied from that report when it admits one id. It is not minted here.

## Proof-8

Linux arm64, Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `oo-a300-proof-8`, id `cf24f4e72b199ac87f592ecb81be5ec2f02de7542d8f5b4c4496ef1822e4c92d`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, Privileged false, NetworkMode `oo-a300-eef831aa-net`, PortBindings empty, OOMKilled false. Docker exit 0. Phase exits under `linux/proof-8/phases/` are 0, including lock-check, fetch-dialogue, test-dialogue, clippy-dialogue, test-citations, libclang, build-query, build-onboarding, test-live, live-oracle, and manifest.

PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a300-pg` was started once (`pg/up.exit` 0) and reused. No second database was created for proof-8.

Pre-edit smoke is `linux/smoke/`, launch exit 0. It was not rerun.

Offline `cargo test` in proof-8: `execute_approval` 3 passed, `propose` 6 passed. The live test function returns before Postgres when `OO_A300_LIVE` is unset, so that offline run records 1 passed in 0.00s. The Postgres run is `test-live`, `OO_A300_LIVE=1`, 1 passed in 56.94s. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. A second `execute_approval` run against the built `answer-citations` binary passed 3 tests.

`query-pg`, `install-column-acl`, `provision-mapping-reader`, and `pg-onboarding` were built from the fixed archive with `--locked --offline`. Libclang 19 resolved `libLLVM.so.19.1` from the private cargo root (`linux/proof-8/libclang-ldd.txt`).

## Oracle

`linux/proof-8/live/oracle.txt` ends with `live_done=1`.

| Key | Value |
|---|---|
| `pg_onboarding_help_exit` | 2 |
| `install_north` | 0 |
| `install_south` | 0 |
| `provision_north` | 0 |
| `cli_propose_equal` | true |
| `north_exit` | 0 |
| `north_rows` | 3 |
| `north_query_id` | `q_3782300da36af286` |
| `cli_execute_exit` | 0 |
| `truncated_exit` | 0 |
| `truncated_disclosure` | `querytruncated` |
| `stale_pack` | `approval` |
| `sql_tamper` | `approval` |
| `write_exit` | 6 |
| `write_receipt` | `absent` |
| `south_exit` | 6 |
| `south_receipt` | `absent` |
| `http_failure` | `provider` |
| `http_execute_called` | false |
| `count_exit` | 0 |
| `count_receipt` | `clean` |

North SQL is `SELECT headcount FROM depot.counter ORDER BY headcount`. The saved execution `linux/proof-8/live/north-execution.json` has `query_receipt` `clean`, three int cells `1`, `2`, and `3`, `spawned` true, `child_exit` 0, `authenticated_audit` false, `entailment` `not_judged`, and `warning_comprehension` `not_judged`. `query_id` is the citation report's admitted id `q_3782300da36af286`. The rendered answer is the status line, the truncated line, the three cell lines, and `[q_3782300da36af286]`.

`SELECT count(*) FROM depot.counter` also ran. Exit 0, receipt `clean`, and the test required the cell value `3`.

`pg-onboarding --help` wrote `pg-onboarding: usage usage` and exited 2. The grants and the mapping reader came from `install-column-acl` and `provision-mapping-reader`.

Stale Pack and tampered SQL returned `approval` before the child. Write SQL and the south role's hidden column returned child exit 6 with receipt `absent`. An HTTP 500 from the fixture model returned `provider` and did not call execute.

The model body saved at `linux/proof-8/live/north-model-body.json` is the public Pack context and the question. The oracle writer scans saved bytes for the fixture passwords and refuses a match.

## Recipes

`linux/proof-8/recipe-origins.txt` records the copies:

- a200 launch `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`
- libclang extract, same bytes as a198, `23d9cd6a4e7033f22a98afc9243c2c9dc2d7e60f5246e9ddcbc8aaa949c2c289`
- pg-wrap, same bytes as a198, `3e01de0bf7cea1d0d4c8591b4c40abf5e473e20ed17aa9d53699735369f4bf67`
- sha256 manifest, same bytes as a261, `1e8593e3bd3de3366644dc4b39fd08ea35915e5a99a1e04ec285b02a1456bf6b`
- pg up copy differs from a198: copy `0778b6ac1b7cce0c2bba877b0537661c72c452a7b9caec4b6a6c9d77f0cd87a5`, origin `75a42e46efb50666788135ecfd40c6db049332d22fd4c3de717cdeab07c203d8`

Source hashes are `source.sha256`. The proof-8 host inventory is `linux/proof-8/MANIFEST.sha256` (432 files at container exit, including `console.log`). The in-container copy is `MANIFEST.in-container.sha256`.

## Failed attempts kept

| Directory | Exit | Stop |
|---|---|---|
| `linux/proof` | 101 | `cargo fetch --locked` rejected the first assembled lock |
| `linux/proof-2` | 101 | `parse_value` received a `Fault` value instead of `fn() -> Fault` |
| `linux/proof-3` | 101 | `CARGO_BIN_EXE_ontology_dialogue` is unset; the target name is `ontology-dialogue` |
| `linux/proof-4` | 101 | `Vec<u8>` has no `trim_end_matches`; `Finished` had no `Debug` |
| `linux/proof-5` | 101 | the fixture accepted one TLS request; the CLI's second call returned `provider` |
| `linux/proof-6` | 101 | Clippy `manual_is_multiple_of` in the test base64 decoder |
| `linux/proof-7` | 101 | execute returned `binary` before spawn |

Proof-7 had already installed north and south grants and recorded `cli_propose_equal=true`. The debug `query-pg` file was 160236912 bytes. The reader cap was 128 MiB, so the hash was not compared. `src/read.rs` now caps the binary read at 256 MiB. Proof-8 hashed that same binary and spawned it.
