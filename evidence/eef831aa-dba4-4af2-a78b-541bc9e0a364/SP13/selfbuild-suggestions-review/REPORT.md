# E50 limited review, a364

Verdict: APPROVE LIMITED E50-D1/1 candidate production only.

Reviewed commit: `d22187ea0fce8e420323660841f2da3b447d5de8` (`Add the generated-only S6–S8 suggestion producer.`). The product tree `tools/selfbuild-suggestions` still matches that commit (`git diff d22187ea -- tools/selfbuild-suggestions` is empty). This review did not edit the product, the root manifests, or any acceptance count.

This packet withholds SP13 parent acceptance, authentication acceptance, certification, publication, S6 entity acceptance, and goal acceptance. A suggestion candidate is not a merged entity. `knowledge_verified` stays false. `knowledge_verified_count` is 0.

The positive gateway result is one mapped text row, value `a364-mapped-row`. It is not a 100k load. PostgreSQL 17.10 reported `mapped_rows=1`, `loaded_rows=1`, and `not_100k_load=true`. The compiled `query-pg` process returned one NDJSON row and `returned_rows` 1. `result_bytes` is 15, the length of that text value.

## What ran

Linux arm64 Rust 1.94.0 (`4a4ef493e`, 2026-03-02, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8) in `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Smoke ran before any evidence checker was edited. Images were already local (`--pull never`).

Focused suite, one builder, `--locked`, no `cargo update` and no lockfile generation:

- library tests: 3 passed
- producer tests: 7 passed, 1 ignored (`gateway_probe_is_refused_before_one_admitted_select`, not executed)
- `cargo clippy --offline --locked --all-targets -- -D warnings`: exit 0, `Finished dev in 2.30s`, no warning lines
- independent probes: 6 passed

Private PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, one container `oo-a364-pg`, 1 GiB, 1 CPU, pids 256, no host port. Server version `17.10 (Debian 17.10-1.pgdg12+1)`. SAN `DNS:oo-a364-pg`.

`query-pg` was compiled from source commit `7a6f7eac8b8041eae215c087d1a80930437871ce`, not from HEAD. Binary SHA-256 `68ddb8ecdb031cff8db5b0764f3916c0593aad43eda2158257b1c5667a365c30`. The frozen `Cargo.lock` compared equal after the build. The builder container was removed. This review did not copy another session's binary.

## Gateway receipt

`selfbuild-suggestions produce --config` exited 0. The producer spawned the compiled binary twice through a capture wrapper. The wrapper copies the binary's stdout, stderr, exit code, request JSON, and audit file. It adds no row and no audit line.

| Spawn | Exit | Stdout | Audit |
| --- | --- | --- | --- |
| request-1, `UPDATE shop.account SET label = label` | 6 | empty | absent |
| request-2, `SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1` | 0 | one row line, then one summary line | admit, then complete |

Probe stderr is the binary's line `query-pg: guard`. There is no request-3.

The row line is exactly:

```
{"record":"row","turn":0,"cells":[{"kind":"text","value":"a364-mapped-row"}]}
```

Summary and audit agree: `status` succeeded, `truncated` false, `returned_rows` 1. `original_sha256` is `8918a40c1d36449a8fc5b4ee64581e584c35442ef2b09dcc7580c77d57cd7e4c` on both the summary and the audit. That is the SHA-256 of the SQL above with no trailing newline. `executed_sha256` is `d7f0d5ba3a0ba3c1feb5f1a8b0e33f0c2a7dba4cec2c5227e512260b640a0cb9` on both. The two hashes differ because the guard rewrites the physical name. The audit does not contain the executed SQL text, and this packet does not invent it.

The request body for request-2 contains the full statement, including `SELECT party_key`, `FROM shop.account`, `WHERE party_key IS NOT NULL`, and `LIMIT 1`.

Outcome effects: `published` false, `grants_issued` false, `curated_written` false, `metrics_certified` false, `auto_merge` false, `knowledge_verified_count` 0. The outcome contains `knowledge_verified` false and does not contain `knowledge_verified` true. Gateway block: probe refused, `probe_exit` 6, admitted 1, spawned 2, configured true. Example `account_party_fk` is `validation` admitted and `gateway_admitted` true. Example `entry_account_fk` is `validation` not_run and `gateway_admitted` false. `max_validated` is 1, so the second example was not sent.

A later judge-only container recompiled the checker and judged these same files. `oracle.exit` is 0 and `oracle.txt` is `oracle ok`. SHA-256 of the capture, `outcome.json`, and `apply.txt` matches `live/capture-sha256-before-judge.txt` and `live/capture-sha256-after-judge.txt`.

## Closed suggestions on the live command

The live command is project `a364`, source `a364`, with two declared foreign keys and no feedback items, no negative pairs, and no money or quantity columns. The outcome therefore has one entity suggestion and two example suggestions, and empty metric and feedback arrays. Queue order is score descending, then `queue_id` ascending:

- unrun example, score 3600 (impact 4, uncertainty 900)
- admitted example, score 1600 (impact 4, uncertainty 400)
- entity suggestion, score 1600 (impact 8, uncertainty 200)

Every queue item has `trusted_metric` false and `knowledge_verified` false. The entity suggestion names primary `shop.party`, provenance `declared_fk`, domain `commerce`, `auto_merge` false, `strong_key_status` unknown. That candidate is not an accepted entity.

Metric templates, negative pairs, weak keys, and bounded feedback are proven by the independent probes and the author suite, which are recorded in FINDINGS.md. The scripted child in those probes uses the value `scripted-not-a-receipt`. Those bytes are not this receipt.

## Failed attempts

`failed-attempts/live-1`: the checker used a Rust 2024 let-chain under `rustc --edition 2021`. Produce was not started. No NDJSON was written.

`failed-attempts/live-2`: produce had already exited 0 and the wrapper had saved both spawns. The checker opened `/out/gold/select.sql` while the SQL files were mounted at `/gold`. The error was `No such file or directory (os error 2)`. The receipt was left in place. The judge-only rerun copied the gold files beside the evidence root and checked those saved bytes.

Owned cleanup is recorded in CLEANUP.md after the one handle snapshot. Raw exit files are indexed in EXITS.md.
