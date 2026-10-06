# Verdict

APPROVE_LIMITED

Session `eef831aa-dba4-4af2-a78b-541bc9e0a267`. This verdict covers the question-bank to sql-runner suite compiler in `benchmarks/question-bank-suite` built on fixed archive `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. The same session wrote the crate and read the preserved Linux logs. There is no second reviewer.

The bound is one public bank role group compiled into an existing `oo.benchmark.sql-runner/v1` suite. Admission is `oo_question_bank::evaluate` from the fixed question-bank crate. `tuple.model` is `none`. `tuple.harness` is `sql-runner`. The planned manifest records `quality_claim` `none` and `authentication` `not_claimed`. B-D07 stays unchecked. The public 300-question bank, human annotation, a customer holdout, and a release stay open. a207 remains the only writer of the root manifest and the root lock. `git diff --stat -- Cargo.toml Cargo.lock` was empty when these documents were written.

## Frozen commit and tree

`linux/impl-4/fixed-commit.txt` is `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. `linux/impl-4/repo-head.txt` is `efdffed87f4e19070536cbd6a72fa1a6583e639e`. When these documents were written, `git rev-parse HEAD` was `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. `fixed-match.exit` is 0. The host crate matches `linux/impl-4/crate-snapshot` (`diff -rq` exit 0), including `Cargo.lock`.

The crate lock was generated inside the archive with `cargo generate-lockfile --offline`. `linux/impl-4/lock-offline.exit` and `lock-drift.exit` are 0. `lock-drift.txt` is empty. Smoke `linux/smoke/lock-pins.txt` records serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` for both question-bank and sql-runner. The crate `Cargo.toml` pins the same two versions with `arbitrary_precision` and path dependencies on `oo-core` and `oo-question-bank`.

## What ran

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0. Each new builder had 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), CapDrop ALL, privileged false, no new privileges. Smoke and impl containers used network `oo-a267-eef831aa-net`. Manifest containers used network none. Volumes `oo-a267-eef831aa-cargo` and `oo-a267-eef831aa-target` were reused across impl attempts and are removed only at own-cleanup. Zero Postgres containers. Nothing was pulled. Nothing was pruned.

| Attempt | Container | Id | docker-run.exit | test.exit | clippy.exit |
| --- | --- | --- | --- | --- | --- |
| smoke | `oo-a267-smoke` | `5ec1d891cf416fc8e01a682cbf3fc48a68f0a4acf218bcb69a8f11c3aef411db` | 0 | question-bank 0, sql-runner 0 | not run |
| impl | `oo-a267-impl` | `3cf019c45678ff2eab7910e19a4dc41efb8c75e0cce8382f3cb157816837ca38` | 101 | 101 | 101 |
| impl-2 | `oo-a267-impl-2` | `faac307fd68693b88734ba9302c9a8c8390dacb5a8650949c0941da253549848` | 101 | 0 | 101 |
| impl-3 | `oo-a267-impl-3` | `14acf7c5be754629911c0f1ec72eea7f19491035adee98e509901999cfcd4d29` | 101 | 0 | 101 |
| impl-4 | `oo-a267-impl-4` | `60a443bb88a6aa746c2c8f662f8a7d7936199bb6bc09c71557b86710ff5566f0` | 0 | 0 | 0 |

Smoke capacity `own=0 global=0 host_cargo=0`. `suite-absent.exit` 0 and `toolchain-absent.exit` 0. Offline question-bank fetch exited 101, then the online fetch exited 0. sql-runner offline fetch exited 0. The focused bank filter `approved_decimal_stays_lexical_and_rescales_exactly` passed, with 9 tests filtered out. The sql-runner `sha256` test passed 2 of 2. The question-bank-suite path was absent from that archive.

impl, impl-2, and impl-3 kept `fmt.exit` 0, `lock-drift.exit` 0, `lock-offline.exit` 0, `fixed-match.exit` 0, `source-sha256.exit` 0, and `stub-fmt.exit` 0. Their containers were removed. The failing logs stay under `linux/impl`, `linux/impl-2`, and `linux/impl-3`.

impl-2 capacity was `own=0 global=1 host_cargo=0`. The global cap is 10, so the launcher ran. The foreign rust-builder was left alone. impl-3 and impl-4 capacity were `own=0 global=0 host_cargo=0`.

impl-4 host config: ExitCode 0, OOMKilled false, privileged false, network `oo-a267-eef831aa-net`, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. `cargo test --locked --offline -- --test-threads=1` passed 3 CLI tests, 12 compile tests, 1 sql-runner proof in 25.67s, and 2 SHA-256 tests. Clippy `--locked --offline --all-targets -- -D warnings` finished with `clippy.stderr` ending `Finished dev profile [unoptimized + debuginfo] target(s) in 0.60s`. The three runner binaries and the query-pg test double were prebuilt under `/target/a267-deps` before `cargo test`, so the proof did not take the cargo lock twice. Prebuild exits are 0.

## Proof counts

`tests/proof.rs` compiles the hand bank at k=2. Planned ids are `pub-table`, `pub-scalar`, `pub-topk`, `pub-absent`, and `pub-note`. `pub-west` is the other role and is excluded. Compiler report `question_count` 5 and `trial_count` 10. sql-runner exit 1. Report `pass_count` 6, `fail_count` 2, `reannotation_count` 2, `not_run_count` 0, `timeout_count` 0, `passed` false, `trial_count` 10, `question_count` 5. Stderr empty. Stdout equals `report.json`. Each planned id appears twice.

`pub-table`, `pub-scalar`, and `pub-topk` are pass / match. `pub-absent` is fail / mismatch. `pub-note` is needs_reannotation with comparator verdict match. The table adapter stdout contains coefficient `120` and omits the characters `1.20`. The absent adapter stdout contains `"kind":"rows"` and `"rows":[]` and omits `not_found`. `questions/pub-absent/gold.json` still contains `"kind":"not_found"`. The trial audit file is present and non-empty.

The query-pg double is `tests/support/query_pg.rs`, rustc'd with `--edition 2021 --deny warnings`. It is not a module of the test crate. The adapter and comparator are the fixed-archive binaries.

## Inventory

`crate-sha256.txt` hashes `benchmarks/question-bank-suite`. `MANIFEST.sha256` hashes this evidence directory. Both are written by `linux/manifest/launch.sh`, and `MANIFEST.sha256` is rewritten by `linux/manifest/refresh.sh` after `console.log`, `hostconfig.txt`, and `CLEANUP.txt` exist. The refresh stdout stays under `/tmp/oo-a267-eef831aa-dba4-4af2-a78b-541bc9e0a267` and is removed with that directory.
