# SP07-W04 PostgreSQL builder report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Actor a328. Fixed product baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. Producer consumed at fixed commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. `parent_acceptance` is false. This report does not accept E47, SP07, the full plan, or a release.

Owned paths:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

Prior owned commits, left as they are: `0d1a9e5a6c62f19cc71494e55b04d1942aa6bcf9`, `bff580e87f51b46841b0dcaa11c35281758c1461`, `149944271ef1c2cb555954d65248413df2166f1c`. `149944271ef1c2cb555954d65248413df2166f1c` is an ancestor of `c23db28dc27f5cf20741dd36e8a8e88129ad1715` and of the tree this packet commits onto. No push.

## What this followup changed

The deadline amendment is unchanged. `deadline_ms` stays 1 to 30000 by default. A value from 30001 to 180000 is accepted only when `purpose` is `generate` and `endpoint.origin` is `http://127.0.0.1:8317`. Profile SQL still sets `statement_timeout` to 10 seconds. The catalog request still sends `connect_ms`, `statement_ms`, and `lock_ms` at 30000. This crate does not set `lock_timeout` to 1 second and does not set a 300 second snapshot budget.

Generation stays one loopback completion per table, in column batches of eight. `shop.account` has 4 columns, `shop.entry` has 5, and `shop.party` has 5, so each live table is one batch. The owned model prompt stays `oo.pg-builder.prompt/v2`. Cache keys stay `oo.pg-builder.cache-key/v2`. `deadline_ms` and `max_completion_tokens` stay outside the cache key. A timeout is not stored.

This packet adds the consumer bridge onto the fixed producer. `tools/pg-builder/src/rebind.rs` copies stored descriptive bytes onto one captured body for `oo.builder.description-prompt/v2`. Prompt SHA-256 is `ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2`. Protocol is `oo.pg-builder.rebind/v1`. The stored generation file, its candidate bytes, and the cache file stay in place. `field_seam` on newly emitted documents is `description-candidate.column-v2` with producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Stored `live/ready` generation files keep the seam bytes they already had.

Review and pack run one command per table. The compiler reseal copies an approved column label and description onto the effective column and records an override review. It does not set `review.state`. This consumer does not invent that state. `live/s9/settings.json` lists `column_descriptions` in `include_unreviewed`, so the role file renders the description with the compiler prefix `UNREVIEWED:`. `live/settings.json` still lists only `dataset_descriptions` and was not overwritten. A deferred column receives no description. Confidence, caveats, and open questions stay on the review record with `confidence_effect` `recorded_only`. `authenticated` stays false. `parent_auth` stays `required`.

No new model call was made. The stored pass 1 bodies bound. `model_bytes` is `copied`. `stored_generation` is `unchanged`.

## Linux proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. No image pull. One builder at a time, 6GiB, 1 CPU, pids 256, no privilege, no host network, no Docker socket. `cargo-update-invoked=no` on focused-10.

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
| `linux/focused-6/attempt-redirect-102` | 101 | The same assertion at `tests/contract.rs:598` after a header-only read. Clippy exit 0. Preserved. |
| `linux/focused-6` | 0 | 11 `pg-builder` tests, 2 fixture tests, clippy exit 0. Predates the v2 rebind. |
| `linux/focused-7` | 0 | Pre-edit smoke. `overlay=none`, base `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. 11 `pg-builder` tests, 2 fixture tests, clippy exit 0. |
| `linux/focused-8` | 101 | `test-builder.exit` 101 and `clippy-builder.exit` 101. Clippy: `provenance` had 10 arguments. Fixture tests and fixture clippy exited 0. `s9.exit` is `skipped`. Preserved. |
| `linux/focused-9` | 101 | Clippy exit 0. Fixture exit 0. `test-builder.exit` 101. Two tests saw `transport.calls` left 0 right 1 because `/cargo-home/tmp` reused the focused-8 temp names. `s9.exit` is `skipped`. Preserved. |
| `linux/focused-10` | 0 | Current source. 12 `pg-builder` tests passed in 1.10s. Fixture tests 2 passed. Clippy `-D warnings` exit 0. `s9.exit` 0. |

Focused-10 `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, container name `oo-a328-focused-10`, uname hostname `a887ed069460`. `hostconfig.txt`: `oom=false`, status exited, exit 0, memory 6442450944, cpu 1000000000, `priv=false`, network `oo-a328-eef831aa-net`, pids 256. Every phase exit is 0: overlay, fetch, root-lock, lock-subset, offline fetch, test-builder, clippy-component, clippy-builder, lockgen-fixture, test-fixture, clippy-fixture, build-s9, s9. Lock subset: builder `registry=208 path=6 missing=0 mismatch=0`, fixture `registry=0 path=1 missing=0 mismatch=0`. Workspace `Cargo.lock` before and after that container is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. This unit did not edit it. Repo HEAD at the focused-10 launch was `7a6f7eac8b8041eae215c087d1a80930437871ce`. Capacity before launch: `host_cargo=0 rust_builders=0 global=0 cap=10`. `preedit.txt` says `overlay=workspace` and `base=c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

The 12 tests include `approved_column_text_reaches_only_the_allowlisted_role`, `validated_draft_caches_and_pack_omits_field_text`, `stale_evidence_misses_and_secret_in_config_opens_no_socket`, `unclassified_role_column_is_refused_before_pack`, and `failures_do_not_cache_or_review`. The approval test requires the compiler prefix `UNREVIEWED:` and keeps the caveat and open-question assertions. The omission test still expects `calls` equal to 1 and field text absent when `include_unreviewed` is only `dataset_descriptions`.

Recipe origins in `linux/focused-10/recipe-origins.txt` are git hash-object ids: a200 `20b391304358e474c189203eeb525b46fc2ea194`, a204 `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`, a199 `e87e21d339c8fe3c6fff4f8e945c68bd11be6426`, a263 `1407cb4f1018764e9218980d0437d217cc2ba666`, plus focused-6 `25ca54f147afbbe1b8c14349fbb27a05da776803`.

The host debug binary from the focused-6 era remains SHA-256 `415d819219664fc612b39d155a6598560b1e53360bb8b2ded87460b9c0d873b8`. The older per-table binary hash `24916b614180249ebbbc262b6b8ebc0a3d2118b12cdc4e606e2bb1ce76cb7f05` remains in `live/per-table/host-meta/`. Focused-10 compiled inside the container. This recovery did not rebuild the host binary.

## Seal and profile

The private PostgreSQL load is unchanged. Party 2000, account 8000, entry 90000. Pass 1 evidence hash `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence hash `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash both passes `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Sample policy `none`. Histogram bounds and most-common values stay the withheld marker. Comments stay `untrusted_data`. No new database was started. `shop.party.national_id` remains a catalog column. The S10 fixture omits it, so its classification stays null.

## Historical model failures

`live/llm/success.json` SHA-256 is `4106d4da79ec0f45e2faece12c907861bbab3c6dfcedf93ef4d03ac7fc36b816`. Those documents remain structural drafts from the 30000 ms deadline. `cache_hit_proof=false` there.

`live/per-table/timeout.json` SHA-256 is `f3d9d8e36fe3ddaed4d62fa33b3785526a5c81ff1a2105867680ec949d2086dd`. That run recorded HTTP 426 for `shop.party` on the 1000 ms deadline, body SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`, then HTTP 503 on the 180000 ms calls, body SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec`. An unauthenticated `GET /v1/models` during that packet returned HTTP 401, body length 27, SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`.

`live/ready-probe.txt` is a later authenticated probe, not an S5 candidate. Request model `grok-4.7`, HTTP 200, latency 2776 ms, `finish_reason=stop`, envelope model `grok-4.7-build`, usage 1256 prompt / 42 completion / 1298 total.

`live/s9/protected-before.sha256` and `live/s9/protected-after.sha256` are identical. `protected-unchanged.exit` is 0 and `protected-unchanged.txt` is `unchanged`. That set covers the ready drafts, the cache snapshot, curated, operator, roles, classifications, `live/settings.json`, `live/llm/success.json`, and `live/per-table/timeout.json`.

## Ready model chain

No new completion was requested. The facts below are the stored `live/ready` documents.

| Run | Exit | Elapsed seconds | Deadline | Result |
| --- | --- | --- | --- | --- |
| unavailable, port 9 | 0 | 0 | 30000 | Three calls, `refusal=unavailable`, `cache_store=not_stored`, empty body hash |
| timeout, port 8317 | 0 | 4 | 1000 | Three calls, `refusal=timeout`, latencies 1002, 1002, and 1001 ms, `cache_store=not_stored` |
| review of `unavailable.json` | 1 |  |  | `review.blocked`. No `failure-review.json` |
| pass1 | 0 | 94 | 180000 | Three calls, HTTP 200, `finish_reason=stop`, call outcome `validated`, `cache_store=stored` |
| pass1-hit, new process | 0 | 0 | 180000 | `cache=hit`, socket `not_opened`, attempt 0, same `candidate_sha256` |
| pass2 | 0 | 85 | 180000 | Cache miss, socket opened. Account and entry validated and stored. Party table `format_rejected` |

Empty failure bodies hash to `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. Pass 1 file SHA-256 `1ce9f61c11104b135fa834181a082de0670ce7b0e7fd6c86d584ed55cd59da22`. Hit file SHA-256 `c168e5bce18fa539d3eab29f232a8962af6edd832bb3cc0b0a3910abaf9f988b`. Aggregate `candidate_sha256` `558466ffa534633d9e4aac732a59532b38dd74ff294ae0a28461b57aa7b5d221`. Aggregate usage 7516 / 12616 / 20132. Declared cost status `unknown`, amount null, invoice false, billing certificate false.

| Table | input_sha256 | candidate_sha256 | response_sha256 | prompt / completion / total | latency ms | HTTP |
| --- | --- | --- | --- | --- | --- | --- |
| shop.account | `681fe64efbb169b1473c5354db37181584000d6ce55afae0b890d3a2d29b2973` | `fad062220149123ab2f978fa7db4444f7710143a6fd34d0c8a0650662b88edeb` | `31e08bb07385f13449bfdbb0bb4d83bd6454ff7f51a12d524d534acdb5a69d15` | 2473 / 5351 / 7824 | 45401 | 200 |
| shop.entry | `c1ebd20685b233a10ccca49a8520fa703d92a24cee11d9dd547ca155c92f404f` | `57c79505b23f4304169e9ff72c2d9410da5f1063371f8913296ef9b442fec9a1` | `f880493ee35429347e96e0e96fdd3e5b3b80750ab22f036a9e7271abafff7b17` | 2524 / 3396 / 5920 | 26129 | 200 |
| shop.party | `3b79b90db87aa7bc292d387beec8731ffdee473bb1c2e9724e827b41ab695745` | `1064ccef6200c2838fee04ef7bcdbbc3e240d926a3768ab5002e1d19163aeb14` | `97d25425f8af966f1df246ae7758a0af281d9d4c0ee4c1d2da2874fb079525b7` | 2519 / 3869 / 6388 | 22722 | 200 |

Pass 2 input hashes differ. Account `f8a66ef81b4c1648b79a29285fc63bc90fdcc6c1f78c2d3a984695848b28197a`, entry `c90196174a52e13a82aefda7980e1834175e20a83bf10b5107d2172153648d90`, party `085b6aacfa9ea9ccbf14d7095dcd00b19d773bed7374918be77ae9d04c835180`. Party is HTTP 200, usage 2524 / 3275 / 5799, response SHA-256 `20f4d6ae6d9afeadbac15b3d820a5e408d7ba9c8d675d6b44a9ae0d924bebbac`, table outcome `format_rejected`, refusal `evidence_identity`, `cache_store=not_stored`, `repair=not_performed`. Pass 2 was not reviewed and was not packed. Curated SHA-256 stayed `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`.

## S9, S10, and the two-role pack

`live/s9/run.exit` is 0. The script reviews and packs each pass 1 call document: `shop.party`, `shop.account`, and `shop.entry`. Reviewer label `local-operator`, reviewer date `2026-10-07`. `authenticated` is false on every column record and on every pack document. `parent_auth` is `required`. `model_publication` files are `false`. Rebind files are `bound`.

| Table | review exit | pack exit | rebind | after outcome | field_candidates_applied | model_publication |
| --- | --- | --- | --- | --- | --- | --- |
| shop.party | 0 | 0 | bound, model bytes copied, stored generation unchanged | packed | true | false |
| shop.account | 0 | 0 | bound, model bytes copied, stored generation unchanged | packed | true | false |
| shop.entry | 0 | 0 | bound, model bytes copied, stored generation unchanged | reviewed_unchanged | false | false |

Stored prompt version on the rebind is `oo.builder.description-prompt/v1`. Rebound prompt version is `oo.builder.description-prompt/v2`. Producer is `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Protocol is `oo.pg-builder.rebind/v1`.

Party decisions: `party_key`, `display_name`, `active`, and `phone` are `approve` and `applied=true`. `national_id` is `defer` and `applied=false`. Account decisions: `account_id`, `party_key`, `opened`, and `label` are `approve` and `applied=true`. Entry decisions: `entry_id`, `account_id`, `qty`, `amount`, and `memo` are `defer` and `applied=false`. `confidence_effect` is `recorded_only` on every column record. Live `candidate_caveats` and `candidate_open_questions` are empty arrays. The party queue has one item, `national_id` reason `deferred`. The account queue is empty. The entry queue has six `deferred` items: one dataset-level item with a null column, then `account_id`, `amount`, `entry_id`, `memo`, and `qty`. The unit test `approved_column_text_reaches_only_the_allowlisted_role` is the proof that a non-empty caveat and a non-empty open question are queued. This live model response did not contain those arrays.

Review confidence strings, copied from the stored candidates: party `party_key` `0.8`, `display_name` `0.7`, `active` `0.75`, `phone` `0.7`, `national_id` `0.7`; account `account_id` `0.86`, `label` `0.5`, `opened` `0.64`, `party_key` `0.9`; entry `entry_id` `0.8`, `account_id` `0.9`, `qty` `0.55`, `amount` `0.7`, `memo` `0.66`.

Effective columns have `review` null. Classified columns have `classification.state` `confirmed`, `by` `local-operator`, `at` `2026-10-07`. `national_id` has `classification` null, `pii` null, and `semantic_type` null. Party approved descriptions on the effective column match the stored text: `party_key` "Required text key, distinct on every row.", `display_name` "Text display name, absent on some rows.", `active` "Required boolean with two observed values.", `phone` "Required text in the phone column, distinct on every row." Account approved descriptions: `account_id` "Unique numeric account identifier.", `party_key` "Party key for the related shop party.", `opened` "Boolean opened indicator.", `label` "Text label; some rows have no value." Entry descriptions stay off the effective columns.

The narrow role file `tables/shop.party.md` in the party pack renders those four descriptions with the prefix `UNREVIEWED:` and the labels `Party key`, `Display name`, `Active`, and `Phone`. It also contains the compiler dataset note `UNREVIEWED: not an approved review, and not a grant.` The restricted role file for that pack contains `party_key` and `active` only. `display_name`, `phone`, and `national_id` are absent from the restricted role text in the party pack, the account pack, and the entry pack. `national_id` is absent from both roles in all three packs.

The account pack applies the account column text. Its narrow `tables/shop.account.md` shows the four account descriptions with `UNREVIEWED:`. Its `tables/shop.party.md` still uses the catalog name as the label, because that pack command applied the account review and the party column overlay belongs to the party pack. The entry pack before and after `tables/shop.entry.md` are both 678 characters for `oo_r_narrow` and both 678 characters for `oo_r_restricted`. Entry `diff.changed` is false. Entry `field_text_absent` is true. Party and account `field_text_absent` are false.

Pack statements on all three after documents: `authentication` `not_claimed`, `authorization` `not_claimed`, `database` `not_used`, `grants` `not_authorized`, `llm` `not_used`, `publication` `unavailable`, `semantic_publication` `not_claimed`. Party and account `reseal` is `compiler` and `source_artifact` is `not_mutated`. Entry `reseal` is `not_required`.

`live/s9/run.sh` reviews `live/ready/timeout.json` and `live/ready/unavailable.json`. Both review CLI exits are 1 and both pack CLI exits are 1. Stderr is `oo.pg-builder/v1/review.blocked review requires a validated generation`. `timeout-review.json`, `timeout-pack.json`, `unavailable-review.json`, and `unavailable-pack.json` are absent. `timeout-zero-effect.exit` and `unavailable-zero-effect.exit` are 0.

## Field producer

Seam id on new documents is `description-candidate.column-v2`. The producer commit is `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. This unit did not edit `crates/oo-builder`, `tools/description-review`, `tools/description-pack`, `tools/description-provider`, or root manifests. Independent review a348, evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a348/SP07/description-field-review/`, records verdict `APPROVE_LIMITED` for that producer seam. That verdict says E47 is not accepted, SP07 is not accepted, and builder integration is not part of the verdict. This packet is the consumer integration. It does not accept E47.

`description-review.single-request` is consumed as one review command and one pack command per table. Each pack command still compiles the whole artifact, so a pack for one table still lists allowlisted columns of the other tables from the catalog.

`shop.entry.amount` numeric oid 1700 remains a query-executor seam owned outside this unit. It was not retested. Production provider HTTPS and 16-byte bearer rules were not weakened. The 6-byte token is accepted only by `loopback_http_local_dev`. Configs name `bearer_file`. The token file is 6 bytes and was left in place.

## Other actors observed

Read only. No files under these trees were edited, and their containers, volumes, and networks were not removed.

a350 `linux/smoke/docker-run.exit` is 0. `linux/proof/docker-run.exit` is 101 and `linux/proof/clippy.exit` is 101. `linux/proof-2/docker-run.exit` is 101 and `linux/proof-2/clippy.exit` is 101. `linux/proof-3/docker-run.exit` is 101 and `linux/proof-3/clippy.exit` is 101. `linux/proof-3/libclang.exit` contains the text `reused`. No `HANDOFF.md` or `REPORT.md` was present. Volumes `oo-a350-eef831aa-cargo` and `oo-a350-eef831aa-target` and network `oo-a350-eef831aa-net` were still present after a328 cleanup.

a351 `frontend/FE-01/raw/types.exit` is 0, `fonts.exit` is 0, `lint.exit` is 0, and `test.exit` is 1. No `HANDOFF.md` or `REPORT.md` was present. This unit did not create a frontend CLI and did not write G2 paths.

## Cleanup

`pg/cleanup.txt`, `pg/cleanup-followup.txt`, and `pg/cleanup-ready.txt` record the earlier removals. This packet removed `oo-a328-eef831aa-cargo`, `oo-a328-eef831aa-target`, `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328`. `pg/cleanup-s9.txt` records `volume_rm_exit=0`, `network_rm_exit=0`, `tmp_rm_exit=0`, `tmp=absent`, `token_file=present`, `token_bytes=6`, `a328_volumes=0`, `a328_network=0`, `a350_cargo=1`, `a350_target=1`, `a350_network=1`. No prune. No foreign name was passed to `docker volume rm` or `docker network rm`.

## Source inventory

`source-sha256.txt` lists the two owned crates. Changed hashes: `CONTRACT.md` `cc630b3578625c82ab8c46fd2f71c827fce175d74924d9dae9bc8c2aaea16b79`, `src/bridge.rs` `8d1dfe367d44147eaa4343a971a57888d0ce9b9805844357b3257665f784e6f4`, `src/lib.rs` `e803c28c6f330bac8ad851da162aa6b106257c19294ee5072c2a540fc37cfd24`, `src/rebind.rs` `903e5040b3a921f9a6877a38851754b133b45116a9a3754f3421b562de05a11d`, `tests/contract.rs` `95bf075e56b53f64b9e585211a03cae36c01f14eb05faf4b032b6f80ec5abe64`. Nested `Cargo.lock` hashes are unchanged: builder `eeed52b1cf0f5b7aff7faa223ffda09f12c02bbdeb32e29a045e5b30a1fc27ca`, fixture `5f608fdc0b828127e7243229957a8e88e68fb403d3c35b408409c3dfabe0b0c4`. `testing/real-db/pg-builder` source hashes are unchanged.

## Withheld

A validated pass 2 aggregate. `shop.party` under the untrusted comment fixture stays `format_rejected` / `evidence_identity` and is not stored. A live miss on a second model id. Config accepts only `grok-4.7`. The live miss is the changed pass 2 comment. `shop.entry.amount` numeric oid 1700. `review.state` remains unset by the delivered pack, so column prose in the role file carries the `UNREVIEWED:` prefix. Live caveat and open-question arrays are empty. `lock_timeout` 1 second and a 300 second snapshot budget are not set by this crate. `parent_acceptance` is false. E47, SP07, and a release are not accepted by this subset. Independent review of this consumer commit has not been run.
