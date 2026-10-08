# SP12 product benchmark

Session `eef831aa-dba4-4af2-a78b-541bc9e0a389`. `benchmark_loop_pass` is false. `integration_passed` is false. `parent_acceptance` is false.

The 100000-row chain and the grok-4.7 meter stay the measured ones. The runner now binds the approved query policy id `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. A new `ontology-dialogue propose` on pin `e31635b213911c725159a23dbccc8463cc45cdd7` still exited 3 with stderr `ontology-dialogue: config` before HTTP and before `execute`. `benchmark_loop_pass` stays false. Whole-product keys stay `notrun`. This card does not accept integration, SP12, or a parent unit.

## Frozen inputs

Freeze SHA-256 `f500dda158ea5220c190c92adfbbfa4687ccfb644b0a86961e3858c6f14fa268`. `freeze_unchanged` is true.

Product archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. pg-builder `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`. ontology-dialogue `e31635b213911c725159a23dbccc8463cc45cdd7`. local-llm relay `bde5e4772bb83407de5e9a50f12c120ce5eca30e`. Fixture commit `f4381843c7f376f196bc30c78c18d9701dc29a6d`.

Import result rows: `shop.party` 2000, `shop.account` 8000, `shop.entry` 90000, total 100000. `rows.match` is true.

## Scorer proof

`linux/focused-4/focused.exit` is 0. `linux/propose/focused.exit` is 0 after the binding change. `linux/propose/test-count.txt` is `passed=7 failed=0 ignored=0`. `linux/propose/phases/clippy.exit` is 0. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Preserved failures: `linux/focused/focused.exit` 101 (score compare), `linux/focused-2/focused.exit` 101 (`needless_range_loop`), `linux/focused-3/focused.exit` 101 (`bool_comparison`).

## Chain

`linux/chain/chain.exit` is 0. `linux/chain/phases/oracle.exit` is 0. `pg/up.exit` is 0. The chain container `oo-a389-chain` (`4a1c0e1adb3e`) later exited 143, OOMKilled false, after `chain=ok` and during the first bench. That log is `failed-attempts/bench-sigterm-143/`. The database stayed up. The chain was not loaded again.

PostgreSQL container `oo-a389-pg` (`324a35c8a12c`), alias `oo-a317-pg`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

## Model meter

pg-builder generate used `grok-4.7` at `http://127.0.0.1:8317/v1/chat/completions`. Declared price status `unknown`, amount null, invoice false.

First document `score-input/builder-first.json`: top cache `miss`, three opened calls, HTTP 200.

| call | request SHA-256 | prompt | completion | total | outcome |
| --- | --- | --- | --- | --- | --- |
| 0 | `8b4c803d7f0676057150097f68271205aea1777970682f9ee846efa9e089ed2b` | 2438 | 2621 | 5059 | validated, stored |
| 1 | `75064fbfe230b57d038862a904e050113cb3d4974942c643f5d8b69dd3a41353` | 2483 | 3518 | 6001 | structural_draft, not stored |
| 2 | `d3cda9a2b656aa7d8170dc244ab3ab6ef4e7da43427f454e1e37e49783a72bac` | 2497 | 4447 | 6944 | validated, stored |

Second document `live/builder/second-partial.json`: calls 0 and 2 were cache hits with socket `not_opened`. Call 1 opened a socket, HTTP 200, completion 3413, total 5896, outcome validated, and was stored. Top cache `partial`.

Third document `score-input/builder-hit.json`: `generate-third.exit` is 0. Top cache `hit`, cache store `stored`, socket `not_opened`. All three calls are `hit` / `not_opened` / HTTP 200 / `grok-4.7` / cost `unknown`. That hit does not replace the opened calls. The scorer reads the fresh meter from `builder-first.json`: `fresh_socket=opened`, `cache_first=miss`, HTTP 200, input SHA-256 `8b4c803d7f0676057150097f68271205aea1777970682f9ee846efa9e089ed2b`. It reads the cache meter from `builder-hit.json`: `cache_hit=hit`, `cache_socket=not_opened`. `http_200_alone_is_e2e_pass` is false. The call list is also in `model-http.json`. The three generation documents were not regenerated.

## Failure controls

`q-wrong-model` verdict `pass`. The frozen class is `must_fail`. `wrong-model.exit` is 1. Stderr: `oo.pg-builder/v1/config.endpoint the endpoint path or model is not the builder endpoint`. The refusal matches the freeze, so the question score is pass.

`q-hidden` verdict `pass`. The chain `query-wide-hidden` exit is 6 and stdout is empty, which is the frozen expectation.

`q-citation-false` verdict `pass`. `citation-invalid.exit` is 1. `citation_status` is `invalid`, `missing_citation` is true. `authenticated_audit` false remains on that report and is the producer's standing field. The invalid status is the frozen expectation.

Fixture cells for null, qty, fk, phone, narrow party, and party count match the freeze. Wide count summary `end_audit_ok` is true. Review is unauthenticated. Hidden column `shop.party.national_id` reason `classification.absent`. Wide installed and narrow replay are present.

## Dialogue

The first propose bound the role-pack label `pol-a317`. That config is preserved at `live/builder/dialogue-config.pol-a317.json`. Exit `bench/phases/propose.pol-a317.exit` is 3.

The runner now copies `identity.policy_id` from `live/run/queries/wide-count.request.json`. `live/builder/dialogue-config.approved.json` and `score-input/dialogue-config.json` bind policy id `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`, role `oo_r_wide`, project `pubfix`, knowledge id `know-a317`. `linux/propose/phases/propose-config.exit` is 0. `linux/propose/phases/propose-approved.exit` is 3. Stderr `ontology-dialogue: config`. `linux/propose/bearer-file-bytes.txt` is 6. The producer checks the binding before it reads the bearer, so the 6-byte file was not the fault this run reached.

Dialogue `parse_config` requires `binding.policy_id` to be 64 lowercase hex and `binding.knowledge_id` to be a slug with no hyphen, then requires both to equal the pack manifest. The pack manifest still has policy id `pol-a317` and knowledge id `know-a317`. The knowledge id fails the slug check first. The role-pack compiler accepts a hyphen and rejects an id longer than 63 bytes, so it cannot emit this query digest. query-pg requires `identity.policy_id` to equal the policy `config_digest`, which is that digest. No `dialogue-execution.json` was written. `q-entry-count` verdict is `fail`. The 100k chain was not loaded again and pg-builder was not called again.

## Whole product

`public_auth`, `session_close_api`, `natural_language_harness`, `runsc`, `wal`, `semantic_publication`, and `customer_uat` are `notrun`.

Machine report: `report/report.json`. Readable copy: `report/report.md`. The report from commit `6cec19fa15932fd5e2ee94070857ead464887f97` is `report/report.before-approved-binding.json`. `bench/phases/score-third.exit` is 1. `bench/phases/score-approved.exit` is 1. `bench.exit` is 1. Seven whole-product keys remain `notrun`, so integration is not accepted and the product loop is not closed.

## Quota-restored recheck

At 2026-10-08T07:46:38Z this session collected its own leftovers and read the producer sources. No container, volume, or network with label `oo.owner=a389` was present. `chain.exit` is still 0. `bench.exit` is still 1. `propose-approved.exit` is still 3. `score-approved.exit` is still 1. `linux/propose/focused.exit` is still 0. Question verdicts are unchanged, including `q-entry-count` fail and the three frozen `must_fail` passes.

`reviewed_effective.rs` blob `7345b036685401d543e174a45a414e0e0db19613` and `role_pack.rs` blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f` still use `check_slug` with a 63-byte cap. Dialogue `grammar.rs` blob `d753913cf81afe2c97f02c555a26c1a7eb65ba92` and `propose.rs` blob `7d614158e0ecf8ea40d81198e2a0ae2baefb4fbf` match pin `e31635b213911c725159a23dbccc8463cc45cdd7`. The approved query digest is 64 hex characters. `know-a317` contains a hyphen. The bearer rule is still 16 to 256 bytes, and this propose never reached it. The wide manifest was left as `pol-a317` / `know-a317`.

The 100k chain, the stored grok-4.7 documents, and the cache hit were not run again. No builder was started. `benchmark_loop_pass` stays false.
