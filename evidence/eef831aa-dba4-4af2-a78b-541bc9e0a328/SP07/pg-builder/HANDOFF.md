# Handoff

Actor a328. Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. Do not treat this unit as SP07 acceptance, full-plan completion, or a release.

## Owned commit

Commit only:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

No push. No root manifest or lock edit. The first unit commit is `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`. The 426 packet is `bff580e87f51b46841b0dcaa11c35281758c1461`. The workspace `Cargo.lock` is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. Leave it.

## Read first

- `tools/pg-builder/CONTRACT.md` for the 180000 ms local-model deadline and per-table cache key `oo.pg-builder.cache-key/v2`
- `REPORT.md` and `FINDINGS.md`
- `linux/focused-6/` for the current 1.94 tests and clippy. Preserve `attempt-test-101` and `attempt-redirect-102`
- `linux/focused-5/` for the proof before the JSON-string prompt sentence
- `linux/catalog/` and `live/pass1`, `live/pass2` for the 100000-row seals
- `live/llm/` for the historical 30000 ms timeouts
- `live/per-table/` for the preserved HTTP 426 and HTTP 503 calls
- `live/ready/` for the validated pass 1 drafts, the no-socket hit, and the pass 2 party rejection
- `pg/cleanup.txt`, `pg/cleanup-followup.txt`, and `pg/cleanup-ready.txt`

## Current behavior

`pg-builder generate` sends one completion per table, in column batches of eight. These shop tables are one batch each. The model is `grok-4.7` with no fallback. `deadline_ms` defaults to 30000. Values through 180000 are legal only for `purpose=generate` and origin `http://127.0.0.1:8317`.

`live/ready/pass1.json` is a validated aggregate. Three HTTP 200 completions have `finish_reason=stop` and consistent usage. Declared price is `unknown` with amount null. `live/ready/pass1-hit.json` is a new process. Its socket is `not_opened`, its attempt is 0, and its `candidate_sha256` matches pass 1. The 1000 ms run returned timeouts, stored nothing, and shares those cache keys.

`live/ready/pass2.json` is a cache miss. Account and entry validated. `shop.party` is `format_rejected` with refusal `evidence_identity` and was not stored. Do not rewrite that response into a valid table candidate.

Review of a structural draft exits 1 with `review.blocked` and writes no review file. Curated bytes did not change.

## Field seam

a339's column-candidate producer is not a fixed delivered commit. Proof-5 clippy-review exited 101. Proof-6 clippy-pack exited 101. Do not compile `pg-builder` against that moving workspace API. Dataset candidates in this packet used the v1 compiler linked from baseline `8c2005d5`. Column text stays staged and unpublished. Resume S9, S10, and the two-role pack only after that producer is delivered at a fixed commit, and consume that fixed SHA rather than the moving tree.

## Cache

A validated draft is the only cache entry. The ready cache snapshot is `live/ready/cache-snapshot/pg-builder/`. It has the three pass 1 keys and the pass 2 account and entry keys. It does not have the pass 2 party key. Deadline and `max_completion_tokens` are outside `oo.pg-builder.cache-key/v2`. Prompt text is inside the key, so this prompt does not reuse the earlier per-table keys.

## Cleanup already done

`oo-a328-pg` was removed in the first packet. The 426 followup removed the cargo volume, target volume, network, and private tmp. This readiness run removed `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328` after the cache snapshot and host binary hash were copied into `live/ready`. The customer token file remains. No prune and no foreign cleanup.

## Next independent review

Review the frozen contract, focused-6, and `live/ready`. Pass 1 S5 drafts and the cache hit are in that directory. S9, S10, and the role pack are withheld. The pass 2 party table rejection is preserved. `parent_acceptance` stays false.
