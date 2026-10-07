# Ontology dialogue independent review

Verdict: **APPROVE_LIMITED** for `tools/ontology-dialogue` at tree `1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0`, commit `63566b60aabdf29a29bfe55c4866edf3c4081cfd`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a332`. The author record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue`. That record is not this review. No product file, root lock, connector manifest, spec, or ledger was edited. Parent RT-D05 stays open. This file does not close SP11 and does not certify a release. Hosted Gateway authentication, untrusted parser admission, `runsc`, and the natural-language harness stay parent work. The local approval is an operator declaration. `authenticated_audit` stays false. Entailment and warning comprehension stay `not_judged`.

`AGENTS.md` is not in the repository. The frozen row is in `specs/open-ontology-v0.1-plan.md`, the prompt is `execution/prompts/v01-ontology-dialogue-active.md`, and the command contract is `tools/ontology-dialogue/CONTRACT.md`. `FROZEN.md` was written before the builder.

## Author manifest

`tools/manifest_check.rs` re-hashed `linux/proof-8/MANIFEST.sha256` from the author packet. The scan is `author-manifest/proof8-manifest.stdout`.

| Result | Value |
|---|---|
| entries | 439 |
| match | 438 |
| mismatch | 1 |
| missing | 0 |
| extra | 1 |

The mismatch is `linux/proof-8/manifest-final.txt`. The recorded digest is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, which is SHA-256 of the empty byte string. The file on disk is 164 bytes and is the manifest tool's stdout: `files=439`, then the empty and `abc` self-check lines. Proof-8 `launch.sh` writes `manifest-host.txt` (`files=432`) and does not write `manifest-final.txt`. A later walk hashed that path while it was empty, and the count text was written after the walk. The other 438 recorded digests still match the bytes on disk. The one extra path is `MANIFEST.sha256` itself, which the tool skips because it is the output file. This review does not call the 439-entry inventory wholly green, and it does not discard the proof-8 logs, oracle, or source hashes.

The author's `source.sha256` matches the committed tool: `entries=18 match=18 mismatch=0 missing=0`, both on the working tree and inside the pre-overlay archive (`linux/preoverlay/source-listed.stdout`).

## Provenance

The author's declared archive was `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. That commit has no `tools/ontology-dialogue`. Proof-8 mounted that archive and overlaid the tool. The tool was later committed as `e31635b213911c725159a23dbccc8463cc45cdd7` and is unchanged at `63566b60`. This review archived `63566b60` and did not overlay a working tree.

`provenance.txt` compares trees. The root `Cargo.lock` is the same object (`2990f901ea98a7b465f462cce8742bc5c51c13aa`). `tools/answer-citations`, `tools/pg-onboarding`, `tools/description-provider`, `crates/oo-pipeline`, `crates/oo-connectors`, `crates/oo-core`, `crates/oo-builder`, and `crates/oo-jobs` are the same objects. `Cargo.toml` adds unapproved workspace pins for axum, tower, and tower-http. `crates/oo-gateway` differs (`1ed61a25` at `96afe`, `42260587` at `63566b60`). `crates/oo-control` differs. The dialogue `Cargo.lock` checksums are a subset of the description-provider, root, and answer-citations locks (`checksum_subset=ok`). The pre-overlay container refused `rust-toolchain.toml`; that file at the commit asks for Rust 1.98.1, and the crate `rust-version` is 1.94.0. The build used the pinned 1.94 image.

The author proof-8 `query-pg` was built from the `96afe` gateway. This review built `query-pg` from the `63566b60` gateway. The fresh audit receipt hash is `3faa1cac8d0c8d74b74298abf23a7371ca8a11e459c7f046165f69719f220c05`. The author's saved receipt hash is `6ab9131dc3b01a6c26f8d6f8ba20dbac6b7b9b114dd1e0b8f5d013ed4ea62fdd`.

## Recipe

Origins and hashes are in `recipe-origin.txt`. Byte copies: a204 `sha256_manifest.rs` `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, a200 `select-debs.rs` `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644`, a200 `sha256.rs` `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e`, a200 `pg-wrap.sh` `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. Libclang extraction follows the a200 script with a332 paths and the Rust package selector. No Python process hashed a lock, a row, or an expected value.

## PostgreSQL

One private server, container `oo-a332-pg`, id `98419da3e7e58861f8c3289e2d631e2278e17c90936b3dd6eea0b2d6012aab28`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/up.exit` is 0. `pg/version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)`. `pg/ports.json` is `{}`. Network `oo-a332-eef831aa-net`. CapDrop `ALL`, with the reviewed `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, and `SETUID` adds. Privileged false. Certificate subject `CN=oo-a332-pg`, SAN `DNS:oo-a332-pg`. Passwords and keys stayed under `/tmp/oo-a332-eef831aa-dba4-4af2-a78b-541bc9e0a332`. `leak-admin.stdout` and `leak-login.stdout` both report `leak=0`.

## Pre-overlay

Container `oo-a332-preoverlay`, id `beb8bf2a3861ace84ae01394d72a6e3b9adf2ef4f1d0b64d2b72d98c8fb328cd`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a332-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, PortBindings `{}`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `capacity.txt` recorded `host_cargo=0 rust_builders=1 global=1 own=0`. The other builder was `oo-a335-preoverlay`. This task's builder was removed after the run.

`tests/execute_approval.rs`: 3 passed. `tests/propose.rs`: 6 passed. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0 in 57.85s. The dialogue tree hash was unchanged. `OO_A300_LIVE` was unset and the live test binary was not started.

## Live chain

Container `oo-a332-live`, id `c37b06ca3710ad0a30f62bebbdadbb8bd9427fa42a63e7aa53d2aaf415cb7c21`. Same HostConfig shape: ExitCode 0, OOMKilled false, Privileged false, Memory `6442450944`, NanoCpus `1000000000`, PortBindings `{}`. One builder. The frozen test reads `OO_A300_*`; the values point at `oo-a332-pg` and this session's fixture files.

| Phase | Exit |
|---|---|
| fetch-citations, build-citations, test-citations | 0 |
| libclang, libclang-includes, libclang-load | 0 |
| fetch-workspace, build-query | 0 |
| fetch-onboarding, build-onboarding | 0 |
| test-live | 0 |
| artifact-check, leak-admin, leak-login | 0 |

`test-citations` passed 3 tests, including the citations binary comparison. `build-query` finished `oo-gateway` in 2m 15s. `libclang-19.so.19` resolved `libLLVM.so.19.1` from `/cargo-home/a332-clang-root`. The console records `ls` failing on a `libclang.so*` glob under the multiarch directory. The load check still exited 0.

`test-live` is `live_trusted_operator_dialogue`, 1 passed in 57.02s. `oracle.txt` ends with `live_done=1`. North SQL is `SELECT headcount FROM depot.counter ORDER BY headcount`. `artifact_check` recomputed its SHA-256 as `beab58df6cd5599039e8b70e4f612fc12923b300479ee2563f021c1c338c4957` and matched `sql_sha256` and `original_sha256` in `north-execution.json`. The execution has `query_receipt` `clean`, three int cells `1`, `2`, and `3`, `spawned` true, `child_exit` 0, `auto_execute` false, `trust` `local_operator_declared`, `authenticated_audit` false, `entailment` `not_judged`, and `warning_comprehension` `not_judged`. `query_id` is `q_3782300da36af286`, copied from `citation.admitted`. The proposal hash is `8b2bbc84952f23ff040bacbf4a6c7ed3f2886f8bd6a071e2ff2a82f0099e4a0b`. The model body contains the question, the north headcount text, and `"temperature":0`. It does not contain `secret_note`, `postgres://`, `password=`, the fixture bearer, or `PRIVATE KEY`.

Oracle lines from the same process: `cli_propose_equal=true`, `cli_execute_exit=0`, `truncated_exit=0`, `truncated_disclosure=querytruncated`, `stale_pack=approval`, `sql_tamper=approval`, `write_exit=6`, `write_receipt=absent`, `south_exit=6`, `south_receipt=absent`, `http_failure=provider`, `http_execute_called=false`, `count_exit=0`, `count_receipt=clean`, `pg_onboarding_help_exit=2`. The frozen test does not save the count execution or the truncation execution. `artifact_check` records `count_cell_persisted=false` and `truncation_json_persisted=false`. The test source asserts the count cell `3` and the `querytruncated` disclosure before it writes those oracle lines. `execute_trusted_select` does not occur in the fifteen Rust files. `src/lib.rs` and `src/main.rs` carry `#![forbid(unsafe_code)]`. No scanned line contained another `unsafe` token.

`propose.rs` does not spawn a process. `auto_execute` is the constant false. `execute` checks the approval, the current Pack digest, the tuple, the SQL hash, and the request SQL before it reads the `query-pg` bytes. The child argv is `--request` with a cleared environment. The HTTP-failure test returns `provider` from `propose` and then checks a marker that `execute` was never asked to spawn. That marker does not observe a refused spawn. The source of `propose` is what keeps the model call off the database.
