# Handoff

Actor a328. Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. Producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. `parent_acceptance` is false. Do not treat this unit as E47 acceptance, SP07 acceptance, full-plan completion, or a release.

## Owned commit

Commit only:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

No push. No root manifest or lock edit. Earlier unit commits are `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`, `bff580e87f51b46841b0dcaa11c35281758c1461`, and `149944271ef1c2cb555954d65248413df2166f1c`. The workspace `Cargo.lock` is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. Leave it. Nested `Cargo.lock` files are already tracked and were not regenerated.

## Read first

- `tools/pg-builder/CONTRACT.md` for the rebind protocol, the 180000 ms local-model deadline, and cache key `oo.pg-builder.cache-key/v2`
- `tools/pg-builder/src/rebind.rs` and `tools/pg-builder/src/bridge.rs`
- `REPORT.md` and `FINDINGS.md`
- `source-sha256.txt` and `SHA256SUMS`
- `linux/focused-10/` for the current 1.94 tests, clippy, and S9 run. Preserve `linux/focused-8/` and `linux/focused-9/`
- `linux/focused-7/` for the pre-edit smoke of `c23db28dc27f5cf20741dd36e8a8e88129ad1715`
- `linux/focused-6/` for the proof before the v2 rebind. Preserve `attempt-test-101` and `attempt-redirect-102`
- `linux/catalog/` and `live/pass1`, `live/pass2` for the 100000-row seals
- `live/llm/` for the historical 30000 ms timeouts
- `live/per-table/` for the preserved HTTP 426 and HTTP 503 calls
- `live/ready/` for the validated pass 1 drafts, the no-socket hit, and the pass 2 party rejection
- `live/s9/` for the stored-draft review, the two-role packs, and the timeout and unavailable zero-effect exits
- `pg/cleanup.txt`, `pg/cleanup-followup.txt`, `pg/cleanup-ready.txt`, and `pg/cleanup-s9.txt`

## Current behavior

`pg-builder generate` sends one completion per table, in column batches of eight. These shop tables are one batch each. The model is `grok-4.7` with no fallback. `deadline_ms` defaults to 30000. Values through 180000 are legal only for `purpose=generate` and origin `http://127.0.0.1:8317`.

`live/ready/pass1.json` is a validated aggregate. Three HTTP 200 completions have `finish_reason=stop` and consistent usage. Declared price is `unknown` with amount null. `live/ready/pass1-hit.json` is a new process. Its socket is `not_opened`, its attempt is 0, and its `candidate_sha256` matches pass 1. The 1000 ms run returned timeouts, stored nothing, and shares those cache keys.

`live/ready/pass2.json` is a cache miss. Account and entry validated. `shop.party` is `format_rejected` with refusal `evidence_identity` and was not stored. Do not rewrite that response into a valid table candidate. Do not pack pass 2.

`review` of a validated one-table generation rebinds the stored body onto description prompt v2 and then calls the fixed producer. The stored file is not rewritten. `live/s9` did that for the three pass 1 call documents. Rebind is `bound`. Reviewer `local-operator` dated `2026-10-07` approved the classified party and account columns and deferred `national_id` and every entry column. `authenticated` is false. `parent_auth` is `required`. Confidence is `recorded_only`. Live caveats and open questions are empty. The role file shows applied descriptions with the prefix `UNREVIEWED:` because the pack leaves `review.state` null and `live/s9/settings.json` lists `column_descriptions`. `live/settings.json` still lists only `dataset_descriptions`. The restricted role does not contain `display_name`, `phone`, or `national_id`. `national_id` is unclassified and is absent from both roles. Entry column prose was not applied. Grants stay `not_authorized`. Publication stays `unavailable`. `model_publication` is false.

Review or pack of a generation that is not validated exits 1 with `review.blocked` and writes no output file. The timeout aggregate and the unavailable aggregate did that. Curated bytes did not change.

## Field seam

a339 producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715` is the fixed archive this bridge compiles against. a348 records `APPROVE_LIMITED` for that producer and states that builder integration is not part of the verdict. Do not edit the producer crates. Do not compile `pg-builder` against a moving workspace API. New documents emit seam `description-candidate.column-v2`. Stored `live/ready` files keep their existing seam bytes.

`shop.entry.amount` numeric oid 1700 remains outside this unit. Do not treat a role-file description as an approved `review.state`. The compiler prefix is `UNREVIEWED:`.

## Cache

A validated draft is the only cache entry. The ready cache snapshot is `live/ready/cache-snapshot/pg-builder/`. It has the three pass 1 keys and the pass 2 account and entry keys. It does not have the pass 2 party key. Deadline and `max_completion_tokens` are outside `oo.pg-builder.cache-key/v2`. The S9 rebind reads the stored generation and does not write a new cache entry. Changing `field_seam` does not change the cache key.

## Cleanup already done

`oo-a328-pg` was removed in the first packet. The 426 followup and the readiness run removed the earlier cargo volume, target volume, network, and private tmp. This packet removed `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328` after focused-10 and `live/s9` had been copied into this evidence tree. `pg/cleanup-s9.txt` records the exits. The customer token file remains. a350 volumes and `oo-a350-eef831aa-net` remain. No prune and no foreign cleanup.

## Next independent review

Review the frozen contract, focused-10, `live/ready`, and `live/s9`. Pass 1 S5 drafts, the cache hit, the explicit operator review, the caller classification, and the two-role packs are in those directories. Focused-8 and focused-9 failures stay. The pass 2 party table rejection stays. `review.state` is null and the role prose is prefixed `UNREVIEWED:`. Numeric oid 1700 stays outside this unit. `parent_acceptance` stays false. a348 does not accept this consumer.
