# SP07-W04 PostgreSQL builder report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Actor a328. Fixed product baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. This report does not accept SP07, the full plan, or a release.

Owned paths:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

The first owned commit is `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`. The per-table HTTP 426 and HTTP 503 packet is `bff580e87f51b46841b0dcaa11c35281758c1461`. This readiness packet is a later commit of the same three prefixes. No push.

## What this followup changed

`CONTRACT.md` keeps the deadline amendment from the previous packet. `deadline_ms` stays 1 to 30000 by default. A value from 30001 to 180000 is accepted only when `purpose` is `generate` and `endpoint.origin` is `http://127.0.0.1:8317`. Fault injection and every other origin keep the ceiling 30000. A value above 180000 is refused. Profile SQL still sets `statement_timeout` to 10 seconds. The catalog request still sends `connect_ms`, `statement_ms`, and `lock_ms` at 30000. This crate does not retune a parent snapshot budget.

Generation is one loopback completion per table. A table with more than eight columns is split into column batches of eight. `shop.account` has 4 columns, `shop.entry` has 5, and `shop.party` has 5, so each live table is one batch. The model input is the one-table card (`oo.pg-builder.model-input/v2`). The sealed three-table command stays on the compiler boundary. Prompt version is `oo.pg-builder.prompt/v2`. The prompt now states that `confidence`, `label`, `description`, and `grain` are JSON strings, `attnum` is a JSON number, and `caveats` and `open_questions` are JSON arrays. Cache keys are `oo.pg-builder.cache-key/v2` and include that prompt text, the table, the batch, and that table's request hash. `deadline_ms` and `max_completion_tokens` are outside the cache key. A timeout is not stored.

The loopback fixture reads the `Content-Length` body before it writes a response. `post_loopback` still writes the header and the body as two `write_all` calls. Product `http.rs` was not changed.

## Linux proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. No image pull. One builder at a time, 6GiB, 1 CPU, pids 256, no privilege, no host network, no Docker socket. `cargo-update-invoked=no`.

| Tree | docker-run.exit | Result |
| --- | --- | --- |
| `linux/smoke` | 1 | Rust 1.94 image has no clippy component. Preserved. |
| `linux/smoke-clippy` | 0 | Clippy installed inside that container only. |
| `linux/focused` | 1 | First product compile failed in `generate.rs`. Preserved. |
| `linux/focused-2` | 101 | Tests failed to compile. Preserved. |
| `linux/focused-3` | 101 | 10 integration tests passed; one clippy lint remained. Preserved. |
| `linux/focused-4` | 0 | 10 `pg-builder` tests, 2 fixture tests, clippy clean. Earlier than `command_bytes`, `prepared_bytes`, and the deadline amendment. |
| `linux/catalog` | 0 | Catalog chain for both passes. |
| `linux/focused-5/attempt-mount-125` | 125 | Docker Desktop missed a new `/tmp` bind. Preserved. |
| `linux/focused-5/attempt-stopped-prompt` | 143 | Stopped so the prompt could name every dataset-candidate key. Preserved. |
| `linux/focused-5/attempt-clippy-101` | 101 | 11 tests passed. Clippy `needless_borrow` in `generate.rs`. Preserved. |
| `linux/focused-5` | 0 | 11 `pg-builder` tests, 2 fixture tests, clippy `-D warnings` exit 0. This proof predates the JSON-string prompt sentence. |
| `linux/focused-6/attempt-test-101` | 101 | `loopback_timeout_redirect_and_chunked_body` returned `unavailable` where the fixture expected `redirect`. Preserved. |
| `linux/focused-6/attempt-redirect-102` | 101 | The same assertion at `tests/contract.rs:598` after a header-only read. Clippy exit 0. Preserved. `CAUSE.txt` records the two-write race. |
| `linux/focused-6` | 0 | Current source. 11 `pg-builder` tests passed, including the redirect case. Fixture tests 2 passed. Clippy `-D warnings` exit 0. |

Focused-6 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, container `92830a861b51`. `hostconfig.txt`: `oom=false`, memory 6442450944, cpu 1000000000, `priv=false`, network `oo-a328-eef831aa-net`, pids 256. Lock subset: builder `registry=208 path=6 missing=0 mismatch=0`, fixture `registry=0 path=1 missing=0 mismatch=0`. The fixed archive `Cargo.lock` hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after that container. The workspace `Cargo.lock` at report time is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. This unit did not edit it. Repo HEAD at the focused-6 launch was `1dce6af6af315652f67e0988abb202f0eacf5362`. Capacity before launch: `host_cargo=0 rust_builders=0 global=0 cap=10`.

Recipe origins in `linux/focused-6/recipe-origins.txt` are git hash-object ids: a200 `20b391304358e474c189203eeb525b46fc2ea194`, a204 `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`, a199 `e87e21d339c8fe3c6fff4f8e945c68bd11be6426`, a263 `1407cb4f1018764e9218980d0437d217cc2ba666`.

The host debug binary was built from the focused-6 archive after that container was gone. The build used the copied cargo volume and `cargo build --locked --offline --bin pg-builder`. Host rustc is `1.98.1 (48a229cea 2026-09-01)` Homebrew. `live/ready/host-meta/build.exit` is 0. Cargo finished the dev profile in 2m 12s. Binary SHA-256 `415d819219664fc612b39d155a6598560b1e53360bb8b2ded87460b9c0d873b8`. The archive bind was visible to Docker on attempt 1. The older per-table binary hash `24916b614180249ebbbc262b6b8ebc0a3d2118b12cdc4e606e2bb1ce76cb7f05` remains in `live/per-table/host-meta/`.

## Seal and profile

The private PostgreSQL load is unchanged from the first packet. Party 2000, account 8000, entry 90000. Pass 1 evidence hash `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence hash `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash both passes `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Seal `prepare_status=request`. Pass 1 `command_bytes=45766`, `prepared_bytes=5343`. Pass 2 `command_bytes=45768`, `prepared_bytes=5344`. Sample policy `none`. Histogram bounds and most-common values stay the withheld marker. Comments stay `untrusted_data`. No new database was started for this followup. `shop.party.national_id` remains a catalog column name on the card. It is not an S10 classification.

## Historical model failures

`live/llm/` is unchanged. `live/llm/success.json` SHA-256 before and after the ready run is `4106d4da79ec0f45e2faece12c907861bbab3c6dfcedf93ef4d03ac7fc36b816`. Those documents remain structural drafts from the 30000 ms deadline. `cache_hit_proof=false` there.

`live/per-table/` is the earlier per-table run and was not rewritten. Its `timeout.json` SHA-256 before and after the ready run is `f3d9d8e36fe3ddaed4d62fa33b3785526a5c81ff1a2105867680ec949d2086dd`. That run recorded HTTP 426 for `shop.party` on the 1000 ms deadline, body SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`, then HTTP 503 on the 180000 ms calls, body SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec`. Those drafts stayed `cache_store=not_stored`. An unauthenticated `GET /v1/models` during that packet returned HTTP 401, body length 27, SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`.

`live/ready-probe.txt` is a later authenticated probe, not an S5 candidate. Request model `grok-4.7`, HTTP 200, latency 2776 ms, `finish_reason=stop`, envelope model `grok-4.7-build`, usage 1256 prompt / 42 completion / 1298 total.

## Ready model chain

`live/ready/` is the run after focused-6. Configs name `bearer_file` and do not embed the token. The token file is 6 bytes. Model id on the request is `grok-4.7`. Origin is `http://127.0.0.1:8317`, path `/v1/chat/completions`. Declared price status is `unknown`. The script does not rewrite model bytes. `repair=not_performed` on every generation document. `live/ready/gate.exit` is 0.

| Run | Exit | Elapsed seconds | Deadline | Result |
| --- | --- | --- | --- | --- |
| unavailable, port 9 | 0 | 0 | 30000 | Three calls, `refusal=unavailable`, `cache_store=not_stored`, empty body hash |
| timeout, port 8317 | 0 | 4 | 1000 | Three calls, `refusal=timeout`, latencies 1002, 1002, and 1001 ms, `cache_store=not_stored` |
| review of `unavailable.json` | 1 |  |  | `review.blocked`. No `failure-review.json` |
| pass1 | 0 | 94 | 180000 | Three calls, HTTP 200, `finish_reason=stop`, call outcome `validated`, `cache_store=stored` |
| pass1-hit, new process | 0 | 0 | 180000 | `cache=hit`, socket `not_opened`, attempt 0, same `candidate_sha256` |
| pass2 | 0 | 85 | 180000 | Cache miss, socket opened. Account and entry validated and stored. Party table `format_rejected` |

Empty failure bodies hash to `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. Fail-cache file count is 0. The 1000 ms timeout and pass 1 share `input_sha256` per table because the deadline is outside the key. The timeout was not stored, so pass 1 missed and opened a socket.

Pass 1 aggregate outcome `validated`, protocol `oo.pg-builder.generation/v1`, `candidate_sha256` `558466ffa534633d9e4aac732a59532b38dd74ff294ae0a28461b57aa7b5d221`. The description compiler reports dataset success as outcome `candidates`. Field outcome is `validated`. Usage is the provider usage. Declared cost status `unknown`, amount null, invoice false, billing certificate false.

| Table | input_sha256 | request_input_sha256 | response_sha256 | prompt / completion / total | latency ms | HTTP |
| --- | --- | --- | --- | --- | --- | --- |
| shop.account | `681fe64efbb169b1473c5354db37181584000d6ce55afae0b890d3a2d29b2973` | `ecceaf60cdd1228066e8d58151cc044a7545f14daccaa5333af273dfb793e431` | `31e08bb07385f13449bfdbb0bb4d83bd6454ff7f51a12d524d534acdb5a69d15` | 2473 / 5351 / 7824 | 45401 | 200 |
| shop.entry | `c1ebd20685b233a10ccca49a8520fa703d92a24cee11d9dd547ca155c92f404f` | `38881a01da268738d1ec18c9973184f506f72b6493221b1a601ac04369d6bbd1` | `f880493ee35429347e96e0e96fdd3e5b3b80750ab22f036a9e7271abafff7b17` | 2524 / 3396 / 5920 | 26129 | 200 |
| shop.party | `3b79b90db87aa7bc292d387beec8731ffdee473bb1c2e9724e827b41ab695745` | `98778062646b2f5e9a8a4f65a9478c343162da9d120cd5bf38821c98b0194439` | `97d25425f8af966f1df246ae7758a0af281d9d4c0ee4c1d2da2874fb079525b7` | 2519 / 3869 / 6388 | 22722 | 200 |

The aggregate usage is 7516 / 12616 / 20132. Each call has `prompt_tokens + completion_tokens = total_tokens`. Field candidates: account 4, entry 5, party 5. Confidence values are JSON strings. `attnum` values are JSON numbers. The hit document repeats those candidate bytes. Its file SHA-256 differs from `pass1.json` because the cache and socket fields differ: pass 1 file `1ce9f61c11104b135fa834181a082de0670ce7b0e7fd6c86d584ed55cd59da22`, hit file `c168e5bce18fa539d3eab29f232a8962af6edd832bb3cc0b0a3910abaf9f988b`. `candidate_sha256` matches. Five cache files were copied to `live/ready/cache-snapshot/pg-builder/`.

Pass 2 input hashes differ from pass 1. Account `f8a66ef81b4c1648b79a29285fc63bc90fdcc6c1f78c2d3a984695848b28197a`, entry `c90196174a52e13a82aefda7980e1834175e20a83bf10b5107d2172153648d90`, party `085b6aacfa9ea9ccbf14d7095dcd00b19d773bed7374918be77ae9d04c835180`. Account and entry are HTTP 200, `finish_reason=stop`, outcome `validated`, and stored. Party is HTTP 200, `finish_reason=stop`, usage 2524 / 3275 / 5799, response SHA-256 `20f4d6ae6d9afeadbac15b3d820a5e408d7ba9c8d675d6b44a9ae0d924bebbac`, table outcome `format_rejected`, refusal `evidence_identity`, call outcome `structural_draft`, `cache_store=not_stored`. The aggregate is `structural_draft` with `cache_store=partial`. The untrusted pass 2 comment was left as card data. The rejected party response was not repaired into a valid table candidate. A second model id was not sent. Config rejects every model id other than `grok-4.7`. The live scope miss is the changed comment. The unit test `stale_evidence_misses_and_secret_in_config_opens_no_socket` covers a stale card.

## Review, pack, and side effects

Review of the unavailable structural draft exited 1: `oo.pg-builder/v1/review.blocked review requires a validated generation`. `failure-review.json` is absent. No `pack-*.json` was written. Curated SHA-256 before and after is `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. No role ACL change and no publication. Statements on the party rejection say curated not written, publication unavailable, pack not compiled, and PII and semantic classification not performed. `bearer_scan=clean` for `live/ready`.

S9 reviewer decisions and the S10 two-role pack were not run. The field producer has no fixed delivered commit, so this packet does not apply column text through review or pack.

## Field producer seam

Seam id `description-candidate.dataset_only`. The linked compiler in the fixed archive admits dataset candidates. Column rows are staged on the generation document and are not applied. Required extension, owned by a339: admit column candidates (`label`, `description`, `confidence`, `caveats`, `open_questions`) and apply them only through explicit review. This unit did not edit `crates/oo-builder`, `tools/description-review`, `tools/description-pack`, or `tools/description-provider`, and it did not compile the moving workspace API.

Observed and left in place: a339 `linux/proof-5` `docker-run.exit` 101 with `clippy-review.exit` 101 (`column_stale` has 8 arguments). a339 `linux/proof-6` `docker-run.exit` 101 with `clippy-pack.exit` 101 (collapsible `if` in `description-pack`) and `clippy-review.exit` 0. a344 `linux/smoke/docker-run.exit` 0, `linux/chain/docker-run.exit` 0, `chain.exit` 0, `pg/up.exit` 0, `pg/actual/capture.exit` 0. `oo-a344-pg` was running during collection and was not touched.

Combined multi-table review stays at seam `description-review.single-request` because one review command carries one request hash. `shop.entry.amount` numeric oid 1700 remains a query-executor seam owned outside this unit. Production provider HTTPS and 16-byte bearer rules were not weakened. The 6-byte token is accepted only by `loopback_http_local_dev`.

## Cleanup

`pg/cleanup.txt` records the earlier removal of `oo-a328-pg`. `pg/cleanup-followup.txt` records the removal after the 426 packet. This readiness run recreated `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, and `oo-a328-eef831aa-net`, copied the cache snapshot and host meta into `live/ready`, then removed those three resources and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328`. No prune. No foreign name was passed to `docker volume rm`. The customer token file remains. Details are in `pg/cleanup-ready.txt`.

## Withheld

S9 review documents, an S10 classification, and a two-role compiler pack are withheld until a339 is a fixed delivered commit. The pass 2 `shop.party` table candidate stays `format_rejected` / `evidence_identity` and is not stored. `parent_acceptance` is false. Independent review has not been run.
