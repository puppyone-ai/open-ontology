# SP07-W04 PostgreSQL builder report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Actor a328. Fixed product baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. This report does not accept SP07, the full plan, or a release.

Owned paths:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

The first owned commit is `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`. This followup is a later commit of the same three prefixes. No push.

## What this followup changed

`CONTRACT.md` was amended before the Rust change. `deadline_ms` stays 1 to 30000 by default. A value from 30001 to 180000 is accepted only when `purpose` is `generate` and `endpoint.origin` is `http://127.0.0.1:8317`. Fault injection and every other origin keep the ceiling 30000. A value above 180000 is refused. Profile SQL still sets `statement_timeout` to 10 seconds. The catalog request still sends `connect_ms`, `statement_ms`, and `lock_ms` at 30000. This crate does not retune a parent snapshot budget.

Generation is one loopback completion per table. A table with more than eight columns is split into column batches of eight. `shop.account` has 4 columns, `shop.entry` has 5, and `shop.party` has 5, so each live table is one batch. The model input is the one-table card (`oo.pg-builder.model-input/v2`). The sealed three-table command stays on the compiler boundary. Prompt version is `oo.pg-builder.prompt/v2`. Cache keys are `oo.pg-builder.cache-key/v2` and include the table, the batch, and that table's request hash. `deadline_ms` is outside the cache key. A timeout is not stored.

## Linux proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. No image pull. One builder at a time, 6GiB, 1 CPU, pids 256, no privilege, no host network, no Docker socket. `cargo-update-invoked=no`.

| Tree | docker-run.exit | Result |
| --- | --- | --- |
| `linux/smoke` | 1 | Rust 1.94 image has no clippy component. Preserved. |
| `linux/smoke-clippy` | 0 | Clippy installed inside that container only. |
| `linux/focused` | 1 | First product compile failed in `generate.rs`. Preserved. |
| `linux/focused-2` | 101 | Tests failed to compile. Preserved. |
| `linux/focused-3` | 101 | 10 integration tests passed; one clippy lint remained. Preserved. |
| `linux/focused-4` | 0 | 10 `pg-builder` tests, 2 fixture tests, clippy clean. Earlier than `command_bytes`, `prepared_bytes`, and this deadline amendment. |
| `linux/catalog` | 0 | Catalog chain for both passes. |
| `linux/focused-5/attempt-mount-125` | 125 | Docker Desktop missed a new `/tmp` bind. Preserved. |
| `linux/focused-5/attempt-stopped-prompt` | 143 | Stopped so the prompt could name every dataset-candidate key. Preserved. |
| `linux/focused-5/attempt-clippy-101` | 101 | 11 tests passed. Clippy `needless_borrow` in `generate.rs`. Preserved. |
| `linux/focused-5` | 0 | Current source. `pg-builder` tests 11 passed, `pg-fixture` tests 2 passed, clippy `-D warnings` exit 0. |

Focused-5 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `hostconfig.txt`: `oom=false`, memory 6442450944, cpu 1000000000, `priv=false`, network `oo-a328-eef831aa-net`, pids 256. Lock subset: builder `registry=208 path=6 missing=0 mismatch=0`, fixture `registry=0 path=1 missing=0 mismatch=0`. The fixed archive `Cargo.lock` hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after that container. The workspace `Cargo.lock` at report time is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. This unit did not edit it.

Recipe origins recorded in `linux/focused-5/recipe-origins.txt` are git hash-object ids: a200 `20b391304358e474c189203eeb525b46fc2ea194`, a204 `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`, a199 `e87e21d339c8fe3c6fff4f8e945c68bd11be6426`, a263 `1407cb4f1018764e9218980d0437d217cc2ba666`.

The host debug binary was built from that focused-5 archive after the container was gone. Host rustc is 1.98.1 Homebrew. Binary SHA-256 `24916b614180249ebbbc262b6b8ebc0a3d2118b12cdc4e606e2bb1ce76cb7f05`. `live/per-table/host-meta/build.exit` is 0. The private tmp tree that held the binary was removed after this hash was copied into evidence.

## Seal and profile

The private PostgreSQL load is unchanged from the first packet. Party 2000, account 8000, entry 90000. Pass 1 evidence hash `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence hash `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash both passes `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Seal `prepare_status=request`. Pass 1 `command_bytes=45766`, `prepared_bytes=5343`. Pass 2 `command_bytes=45768`, `prepared_bytes=5344`. Sample policy `none`. Histogram and most-common values stay withheld. Comments stay `untrusted_data`. No new database was started for this followup.

## Live model chain

Historical `live/llm/` is unchanged. `live/llm/success.json` SHA-256 before and after this run is `4106d4da79ec0f45e2faece12c907861bbab3c6dfcedf93ef4d03ac7fc36b816`. Those documents remain structural drafts from the 30000 ms deadline. `cache_hit_proof=false` there. This followup did not rewrite them.

`live/per-table/` is the per-table run. Configs name `bearer_file` and do not embed the token. Model id is `grok-4.7`. Declared price status is `unknown`. An unauthenticated `GET http://127.0.0.1:8317/v1/models` returned HTTP 401. Body length 27. Body SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`. Date header `Wed, 07 Oct 2026 15:11:23 GMT`.

Every generate exited 0 and wrote `structural_draft`, `cache=miss`, `cache_store=not_stored`, `repair=not_performed`. Usage is null. Declared cost status `unknown`, amount null, invoice false, billing certificate false. Empty response bodies hash to `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

| Run | Deadline | What the three table calls did |
| --- | --- | --- |
| unavailable, port 9 | 30000 | `refusal=unavailable`, socket opened, latency 0, empty body hash |
| timeout, port 8317 | 1000 | `shop.account` timeout at 1003 ms, `shop.entry` timeout at 1001 ms, then `shop.party` HTTP 426 at 546 ms |
| pass1, port 8317 | 180000 | HTTP 503 at 4 ms, 6 ms, and 6 ms |
| pass1 repeat | 180000 | HTTP 503 at 6 ms, 4 ms, and 5 ms. Sockets opened. |
| pass2, port 8317 | 180000 | HTTP 503 at 5 ms, 5 ms, and 6 ms. Request hashes differ from pass 1. |

HTTP 426 response SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`. The generation document stores that hash and the status code. It does not store the response headers or body. HTTP 503 response SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec` on every later chat completion in this run. No further request was sent to `127.0.0.1:8317` after these documents were written. A new endpoint or an explicit readiness signal is still required before another unchanged model call.

Cache keys for the 1000 ms timeout and the 180000 ms pass 1 are the same per table, because the deadline is outside the key. Account `input_sha256` `ac643e6dab306dc3ff983ae38e4cdda4ce27765893e4820a26017e8d852bc647`. Entry `71a30a7a83db8f4b56f7b1b98950a9668a6e8d37d7889295f54f1a892b2ec272`. Party `7e0644065db00605b15079acec32c1394571c33989fde072329ded2ccb93b06c`. The timeout was not stored. The repeat therefore missed and opened a socket. `live/per-table/cache-hit.txt` records `candidate_bytes_same=true` because both documents have empty candidate arrays. `cache-hit-proof.txt` records `cache_hit_proof=false`. Fail-cache file count is 0. Success-cache file count was 0. Pass 2 account `input_sha256` is `b917d9a61775a77b0b02f13d347ca74ce66ef63cb5d9cd6541156191a8844b42`, a different key.

Request input hashes, pass 1: account `a94bc8396b8813b0107287bf3456db45336dd53f16f6e8a54ad4a933c8b6f563`, entry `8b5f12a5f94c91650e7c771ab2ecd5e001a38879a09a883c7efadb72b63dd79a`, party `36bc82f1b492d57b695a76f15a5dc165a95c1a08d9f2e9966d7fe1bcd0029159`. Pass 2: account `4403d732cc490db964521cf51611ca1702a8be638776526a8fef40058557a10b`, entry `1a2b0dad033e6cc5025236365c642fd8d4b08c8329c36a1c7953463d666973e2`, party `90fd162613134d921233b7c097122b0a4f392b5c98ef7e668c5b50555dcd927c`.

## Review, pack, and side effects

Aggregate review of `pass1.json` exited 1: `review.blocked`, `review requires a validated generation`. No per-table review file and no pack file were written, because no call had `outcome=validated`. Curated SHA-256 before and after is `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. No role ACL change and no publication. `wrapper.exit` is 1. The acceptance gate stopped on `set -e` before `gate.exit` was written.

`bearer_scan=clean` for `live/per-table` and for this evidence tree. The token file is 6 bytes. Config JSON contains the private path only.

## Field producer seam

Seam id `description-candidate.dataset_only`. The linked compiler in the fixed archive admits dataset candidates. Column rows are staged on the generation document and are not applied. a339 has uncommitted edits in `crates/oo-builder`, `tools/description-review`, `tools/description-pack`, and `tools/description-provider`, plus `FREEZE.md` for `oo.builder.description-response/v2`. Its latest smoke, `linux/smoke-3`, has `docker-run.exit` 101 and `test-provider.exit` 101. That producer is not a fixed delivered commit. This unit did not edit those trees and did not compile against them. Combined multi-table review stays at seam `description-review.single-request` because one review command carries one request hash. `shop.entry.amount` numeric oid 1700 remains a query-executor seam owned outside this unit.

## Cleanup

`pg/cleanup.txt` records the earlier removal of `oo-a328-pg` and the first cargo, target, and network resources. This followup recreated `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, and `oo-a328-eef831aa-net` for focused-5 and the host build, then removed those three and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328`. No prune. No foreign name was passed to `docker volume rm`. The customer token file remains. Details are in `pg/cleanup-followup.txt`.

## Withheld

Validated S5 table candidates, validated field candidates, a durable cache hit, S9 review documents, and an S10 role pack are withheld. The already-started per-table calls ended as provider failures: HTTP 426 on `shop.party` during the short-deadline run, then HTTP 503 on the 180000 ms runs. No model body was rewritten into a candidate. `parent_acceptance` is false.
