# SP13-W04 E50-D1/1 generated-only S6–S8 suggestions

Session `eef831aa-dba4-4af2-a78b-541bc9e0a355`. Fixed base `7a6f7eac8b8041eae215c087d1a80930437871ce`. Product crate `tools/selfbuild-suggestions`. Evidence is only this directory.

This delivery produces S6 entity and domain candidates, S7 metric hypotheses, S8 question and SQL-template candidates, and an S9 review-queue input. Semantic publication, metric certification, entity merge, curated writes, grants, and acceptance of SP13 as a whole are outside this slice. No new review count is opened here.

## Producer

`selfbuild-suggestions produce` reads one JSON command from stdin, at most 2_097_152 bytes. `produce --config PATH` is the only path that can spawn a process. Without a config, `produce` does not spawn and does not open a socket.

Exit 0 writes the outcome. Exit 1 is malformed: empty stdout, stderr `selfbuild-suggestions: malformed <reason>`. Exit 2 is refused: one outcome on stdout, stderr `selfbuild-suggestions: refused <reason>`. Refused tokens are `seal`, `authorization`, `scope`, `catalog`, `profile`, and `probe`. Malformed tokens are `usage`, `empty`, `stdin_too_large`, `json`, `trailing`, `duplicate_key`, `unknown_field`, `type`, `budget`, `config`, and `io`.

The command carries a versioned scope (`project`, `role`, `evidence_sha256`, `source_id`, `schema_version` `oos/v1`, `model_scope` `none`), an authorized catalog seal, confirmed relationships, a typed profile (`sample_policy` `none`), bounded feedback, negative pairs, curated override hashes, and prior candidate ids. Seals are SHA-256 of canonical JSON. `evidence_sha256` must equal `catalog_sha256` and the computed catalog hash. Duplicate keys are rejected by the crate parser. Depth above 40 or more than 200_000 nodes is a budget refusal.

Candidate ids are `cand_` plus the first 32 hex characters of SHA-256 over `oo.selfbuild-suggestions.candidate/v1\n<project>\n<kind>\n<key>\n`. The checked oracles are canonical `{"a":1,"b":"x"}` = `ecf9e98ec0641e23113ff3ce8bdc78d0ddd249886517fd4a7f68cc83d4e65667` and `candidate_id("public","metric","logistics.shipment.amount")` = `cand_5544a04dbfef7e7def73d6ac050b29d3`.

## S6, S7, S8, S9

S6 builds undirected components from confirmed declared foreign keys that are not negative pairs. Members are sorted by `schema.table`. The primary member is the one with no outgoing edge and the greatest incoming degree, then the earlier name. If every member has an outgoing edge, the earlier name is primary. A one-table component has provenance `isolated` and confidence `lower`. A connected component has provenance `declared_fk` and confidence `graph`. Domain labels are `commerce` for `shop`, `transport` for `logistics` or `tms`, `finance` for `finance` or `erp`, `warehouse` for `wms` or `warehouse`, otherwise the schema name. More than one schema yields `cross_source`. `strong_key_status` is `weak` when a member has a phone or person-name column, `validated_unmerged` when no weak column is present and a profile column is `validated`, otherwise `unknown`. `auto_merge` is false. That status does not merge anything.

S7 emits one hypothesis for each money or quantity column. `certified` is false, `status` is `draft`, `owner_needed` is true, and `control_total_needed` is true. A time column on the same table sets grain `month`, `time_dimension` to the earliest time column, `time_anchor` `prev_month_start`, `time_anchor_closed` false, and `time_anchor_needed` false. Otherwise grain is `row`, the time fields are null, and `time_anchor_needed` is true. The template is `SELECT sum(<column>) FROM <schema>.<table>`. Metric SQL is `not_applicable` and is never sent to the gateway.

S8 emits one example per remaining graph edge, ordered by constraint name, then source table, then column. The question is `Which values of <schema>.<table>.<column> does <constraint> relate?`. The template is `SELECT <column> FROM <schema>.<table> WHERE <column> IS NOT NULL LIMIT 1`. Without a config, validation is `not_run` and `gateway_admitted` is false. With a config, the probe runs first. The producer records `probe=refused` and `probe_exit=6` only when that child started, did not time out, exited 6, and wrote empty stdout. Any other probe result refuses the command with token `probe`, admits nothing, and leaves examples at `not_run`. After a refused probe, at most `max_validated` examples (1 through 4) are sent. Exit 0 with a summary `status=succeeded` and `returned_rows >= 1` is `admitted`. Exit 0 with zero rows is `empty`. Exit 6 is `guard_refused`. Anything else is `error`. `knowledge_verified` is written false on every candidate and on the outcome. Admission does not verify knowledge.

Feedback stays untrusted, bounded, and review-only. It is a separate queue channel from metrics. `trusted_metric` is false. Unknown keys, weak keys, and negative entity pairs do not auto-merge, grant, certify, overwrite curated bytes, or publish. A curated hash that differs from the generated fact emits `stale_override` and leaves the generated candidate in place. `curated_written` stays false.

The S9 queue scores impact times uncertainty, sorts by score descending and `queue_id` ascending, and keeps 128 items. Scores used by the public fixture are stale 7200, metric 4200, example 3600 (or 1600 when admitted), negative 3000, feedback 2000, edged entity 1600, and isolated entity 1500.

## Recorded crate tests

Linux arm64, Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. The repo `rust-toolchain.toml` was not mounted. Dependencies stayed on the committed lock. No `cargo update` and no `cargo generate-lockfile`.

Smoke before product edits is `smoke-before/rustc.txt`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Focused suite, `build/test.exit` 0 and `build/test.txt`:

- library: 3 passed, 0 failed
- `tests/producer.rs`: 7 passed, 0 failed, 1 ignored

`build/clippy.exit` is 0 and `build/clippy.txt` is empty (`-D warnings`). `build/rustfmt.exit` and `build/rustfmt-check.exit` are 0.

The seven producer tests cover candidate gold, provenance, dedup, stale override, cross-project drops, closed effects, CLI exits, parser budget, graph rules (cycle, in-degree tie, weak versus validated, multi-column and type refusals), and a fake runner for admission. The ignored test is the live gateway run below. The default suite does not need Postgres.

The public fixture is `logistics` (`customer`, `shipment`, isolated `warehouse`, negative pair suppressing `alias`). Domain `transport`. One month-grain metric on `shipment.amount`. One example left `not_run`. Queue scores `[7200, 4200, 3600, 3000, 2000, 1600, 1500, 1500]`. Two `produce` calls match. `INJECT_GRANT_TOKEN` is absent from SQL and questions. `CROSS_PROJECT_TOKEN` is absent from the outcome.

The sealed 100k input is the existing catalog `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/live/run/catalog.json` (schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`, source `pubfix`). It was not a row load. The profile tables list is empty, so column classes come from names. `reltuples` were not a fresh scan. Tables are `shop.party`, `shop.account` (`account_party_fk`), and `shop.entry` (`entry_account_fk`). The connected entity primary is `shop.party`. Domain is `commerce`. `display_name` and `phone` make `strong_key_status` `weak`. `national_id` stays unknown. Metrics are `amount` then `qty`, grain `row`, `time_anchor_needed` true. Tampering a column name refuses on `seal` before catalog consistency.

## Live gateway record

One private PostgreSQL 17.10 was already up. The ignored test was already finished. This pass collected that result and did not start another builder or another database.

Container `oo-a355-sbs-live` id `f1ec86f0bf5569e6ac8ef17c99d55f68dc27d642c8ede0c274d4c72c11aeeaa6`, exit 0, OOMKilled false. Image rust digest above, linux arm64, 6442450944 bytes, 1 CPU, pids 4096, cap-drop ALL, no-new-privileges, privileged false, network `oo-a355-sbs-net`. Launch capacity was `own=0 global=0 host_cargo=0`. `live/rustc.exit` 0. `live/test.exit` 0. `live/Cargo.lock.frozen` matches `tools/selfbuild-suggestions/Cargo.lock`.

`live/test.txt` verbatim result for the gateway test:

```
running 1 test
test gateway_probe_is_refused_before_one_admitted_select ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.20s
```

The test pointed `ProcessRunner` at the compiled `query-pg` binary and `pg/request.json` (password file path only; the password value is not in the document). The probe SQL was `UPDATE shop.account SET label = label`. The one validated example SQL was `SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1`. The second example, `entry_account_fk`, stayed `not_run` because `max_validated` is 1.

`run_gateway` records `probe=refused` and `probe_exit=6` only when that first child started, did not time out, exited 6, and stdout was empty. The test asserts those outcome fields, then `gateway.admitted=1`, `examples[0].validation=admitted`, `examples[0].gateway_admitted=true`, and `examples[1].validation=not_run`. An example becomes `admitted` only when its child exits 0 and the NDJSON summary has `status=succeeded` and `returned_rows >= 1`. The test also asserts `knowledge_verified` is false on every object and that every effect flag stays closed, including `knowledge_verified_count` 0. Metric SQL containing `sum(` is not among the calls.

The query-pg NDJSON bytes were not saved. `live/record.txt` is the transcript of `live/test.txt` plus those predicates. This report does not invent a row payload.

`knowledge_verified` remains false. The admitted select is gateway admission of one guard-checked statement. It is not knowledge verification and not certification.

## query-pg binary

Built from `git archive` of `7a6f7eac8b8041eae215c087d1a80930437871ce` paths `Cargo.toml`, `Cargo.lock`, `crates`, and `tools/gateway-token-store`. The dirty worktree was not copied over the archive. `rust-toolchain.toml` was absent from the archive. `querypg/build.exit` 0. `querypg/fetch.exit` 0. `querypg/libclang.exit` 0. Binary SHA-256 `68ddb8ecdb031cff8db5b0764f3916c0593aad43eda2158257b1c5667a365c30`. Source `query-pg.rs` SHA-256 `b6cd2f4e5a5af9d895a6c2ab9020ca6956afc4f61dd0b799a9cc297f54c13a22`. Workspace lock SHA-256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. The binary is on the private target volume and is not committed.

Libclang extraction reused `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` and the same directory's `tools/sha256.rs` and `tools/select-debs.rs`. Hashes are `querypg/origin-sha256.txt`.

Token-store blobs at that commit match the hashes pinned in the archived `crates/oo-gateway/build.rs` (13 files, 0 mismatches), which is why the third build could compile.

## Private Postgres

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux arm64. Container `oo-a355-pg` id `05e25c9e78cc8d0e80481b9aa5e3f7efaa90452e4fc8cf8a6096b112d58a38a0`. 1073741824 bytes, 1 CPU, pids 256, privileged false, cap-drop ALL, cap-add CHOWN DAC_OVERRIDE FOWNER SETGID SETUID, no-new-privileges, network `oo-a355-sbs-net`, alias `oo-a355-pg`, no host port. `pg/up.exit` 0. `pg/apply.exit` 0. `pg/apply.txt` ends at `INSERT 0 1`, `ANALYZE`, two `GRANT`s, and `shadow ready`.

Database `oo_a355`. Login role `oo_a355_login`. Approved role `oo_r_reviewer` (NOLOGIN, flags required by the gateway all false). Mapping is only `shop.account.party_key` (text) under project `pubfix`, generation 355. One shadow row was inserted and analyzed so `unknown_rows=refuse` could admit the select. The 100k catalog rows were not loaded. Server certificate SAN is `DNS:oo-a355-pg` (`pg/server-san.txt`). Passwords and private keys stayed in `/tmp/oo-a355-sbs-secrets` and are not in this evidence tree. A scan of this directory found no password values and no PEM private keys.

## Pins

Crate lock is a hand subset, lock version 4. `serde` `=1.0.229` with derive, `serde_json` `=1.0.151`, `sha2` `=0.10.9`. Edition 2024, `rust-version` 1.94.0, `forbid(unsafe_code)`, resolver 3, its own `[workspace]`. Root manifests were not edited.

## Handle observation

One 18-minute wait ended `2026-10-07T20:30:15Z`. One observation was written at `2026-10-07T20:49:24Z` in `observation/handles.txt`. It was not repeated.

## Limits

S6–S8 candidate production and an S9 queue input. The outcome does not publish a semantic model, certify a metric, merge an entity, write curated storage, issue a grant, change a role, call a model, or retain a sample. `knowledge_verified_count` is 0. The admitted select is one gateway guard result. Query-pg stdout bytes are not in the evidence. `reltuples` on the sealed catalog were not a fresh scan. This report does not accept the parent plan or the SP13 goal.
