# Handoff

Actor a328. Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. Do not treat this unit as SP07 acceptance, full-plan completion, or a release.

## Owned commit

Commit only:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

No push. No root manifest or lock edit. The first unit commit is `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`. The workspace `Cargo.lock` is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` at this writing. Leave it.

## Read first

- `tools/pg-builder/CONTRACT.md` for the 180000 ms local-model deadline and per-table cache key `oo.pg-builder.cache-key/v2`
- `REPORT.md` and `FINDINGS.md`
- `linux/focused-5/` for the current 1.94 tests and clippy. Preserve `attempt-mount-125`, `attempt-stopped-prompt`, and `attempt-clippy-101`
- `linux/focused-4/` for the earlier 10-test proof
- `linux/catalog/` and `live/pass1`, `live/pass2` for the 100000-row seals
- `live/llm/` for the historical 30000 ms timeouts. Do not rewrite those files
- `live/per-table/OBSERVATION.txt` for the per-table HTTP 426 and HTTP 503 calls
- `pg/cleanup.txt` and `pg/cleanup-followup.txt`

## Current behavior

`pg-builder generate` sends one completion per table, in column batches of eight. These shop tables are one batch each. The model is `grok-4.7` with no fallback. `deadline_ms` defaults to 30000. Values through 180000 are legal only for `purpose=generate` and origin `http://127.0.0.1:8317`.

The per-table run already called the proxy. `shop.party` on the 1000 ms deadline returned HTTP 426. Later calls with `deadline_ms` 180000 returned HTTP 503 in a few milliseconds. All of those documents are `structural_draft`, `cache_store=not_stored`, `repair=not_performed`. Declared price is `unknown` with amount null. Review of the aggregate exited 1 with `review.blocked`. Curated bytes did not change.

Do not send another unchanged request to `http://127.0.0.1:8317/v1` until a human provides a new endpoint or an explicit readiness signal. Do not edit auth stores, provider keys, or backups to get past HTTP 426.

## Field seam

a339's column-candidate producer is not a fixed delivered commit. `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a339/SP07/description-field-candidates/linux/smoke-3` exited 101, with `test-provider.exit` 101. Do not compile `pg-builder` against that moving workspace API. Dataset review and pack stay on the v1 compiler linked from baseline `8c2005d5`. Column text stays staged and unpublished. Resume the field pack only after that producer is delivered at a fixed commit.

## Cache

A validated draft is the only cache entry. The 1000 ms timeout and the 180000 ms pass 1 share `input_sha256` per table. The timeout was not stored, and the repeat opened a socket. `cache_hit_proof=false`. Pass 2 hashes differ because the untrusted comment changed the card and the one-table request.

## Cleanup already done

`oo-a328-pg` was removed in the first packet. This followup removed `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328` after copying the host binary hash into `live/per-table/host-meta/`. The customer token file remains. No prune and no foreign cleanup.

## Next independent review

Review the frozen contract, focused-5, and `live/per-table`. Validated S5, the cache hit, S9, and the role pack are withheld until a ready endpoint returns a real completion and a339's field producer is a fixed commit. `parent_acceptance` stays false.
