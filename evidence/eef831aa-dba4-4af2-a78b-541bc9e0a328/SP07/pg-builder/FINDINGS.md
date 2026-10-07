# Findings

`parent_acceptance` is false. Independent review has not been run. This followup does not accept SP07.

## Proven

- Rust 1.94 focused-5, after the per-table and 180000 ms amendment: 11 `pg-builder` tests passed, 2 `pg-fixture` tests passed, clippy `-D warnings` exit 0. `docker-run.exit` 0. Builder 6GiB, 1 CPU, pids 256, `oom=false`, `priv=false`. Earlier failed focused-5 attempts are preserved: mount 125, stopped prompt 143, clippy 101.
- Focused-4 remains the earlier green proof of 10 tests, before `command_bytes`, `prepared_bytes`, and this amendment.
- The catalog chain and the 100000-row private load are unchanged. Pass 1 evidence `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Pass 1 seal `command_bytes=45766`, `prepared_bytes=5343`. Pass 2 `command_bytes=45768`, `prepared_bytes=5344`.
- Profile `statement_timeout` is still 10 seconds. Catalog `connect_ms`, `statement_ms`, and `lock_ms` are still 30000. The 180000 ms ceiling applies only to a generate call whose origin is `http://127.0.0.1:8317`.
- Live per-table generation exits 0 with `structural_draft` and `cache_store=not_stored`. The 1000 ms run recorded two timeouts and one HTTP 426. The 180000 ms runs recorded HTTP 503. Usage is null. Declared cost status is `unknown`, amount null, invoice false.
- The 1000 ms drafts and the 180000 ms pass 1 drafts share cache keys. Nothing was stored, so the repeat opened a socket. Pass 2 keys differ. `cache_hit_proof=false`.
- Aggregate review exited 1 with `review.blocked`. Curated bytes and `live/llm/success.json` are unchanged. `bearer_scan=clean`.

## Withheld

- A validated S5 table candidate and a validated field candidate. No completion body with `finish_reason=stop` was received.
- A durable cache hit of a validated draft. `candidate_bytes_same=true` in `live/per-table/cache-hit.txt` compares two empty structural drafts.
- Live S9 decisions and an S10 role pack. Pack was not called. `classification.missing` was not reached.
- Field publication through a339. That producer is uncommitted, and `linux/smoke-3` exited 101 on the provider tests. This unit did not compile that moving API.

## Proxy metadata already observed

Unauthenticated `GET /v1/models` returned HTTP 401, body length 27, SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`.

Authenticated `POST /v1/chat/completions` for `shop.party` during the 1000 ms run returned HTTP 426, latency 546 ms, body SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`. The body bytes and response headers were not retained.

Later authenticated posts in the same script returned HTTP 503, body SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec`. No new model request was issued after that script.

## Seams

- `description-candidate.dataset_only`. Required extension, owned by a339 and not delivered as a fixed commit: admit column candidates (`label`, `description`, `confidence`, `caveats`, `open_questions`) on `oo.builder.description-response/v2` and apply them only through explicit column review. This unit does not fake that pack.
- `description-review.single-request`. One review command carries one request hash, so the three-table aggregate is not a review.
- Numeric oid 1700 for `shop.entry.amount` remains outside this unit.
- Production provider HTTPS and 16-byte bearer rules were not weakened. The 6-byte token is accepted only by `loopback_http_local_dev`.
