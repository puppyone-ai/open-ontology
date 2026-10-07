# Real PostgreSQL end-to-end fixture

Status: frozen before automation code. This file is not a development unit and does not accept a parent goal.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a317`. Fixed product archive `10ecd7654fd7939b6655a0e50a84214a7fca316e`. That commit is an ancestor of the worktree. Product bytes are taken from `git archive` of that commit. Dirty worktree files, including later pipeline work, are not mounted into the build.

Owned paths:

- `testing/real-db/pg-e2e/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/**`

No other path is edited. The root manifest, root lock, and every product crate stay untouched. No new dependency. No `cargo update`. No `cargo generate` of the root lock. a313 remains the only root manifest and lock writer.

## What this line is

M1-I01 is a real source PostgreSQL, a full import and catalog, a reviewed draft and pack, column ACL, and a controlled query on that same data. M1-I02 is that chain consumed by the compiled CLI, with two roles, real exits, and audit, without a hand-made stand-in for the middle artifacts. Both stay `not_accepted`. A passing trusted-query fixture is not I01 or I02 sign-off.

The parent counts in `specs/open-ontology-acceptance-counting.md` name those nodes and leave them open. a274 accepted M1-D25, the two-phase onboarding tool, on a 6-row shop fixture whose operator generation is hardcoded to 48. That generation is not a row count. This line does not reuse that oracle and does not edit it.

This line loads a public synthetic source, runs the archived `pg-onboarding` CLI, and checks what the target and `query-pg` actually return. It does not insert business rows into the target, does not invent pack bytes, and does not treat a fixture label as a human approval.

## Non-claims

`parent_acceptance` is false. `development_unit` is false. `m1_i01` and `m1_i02` are `not_accepted`. Semantic publication, public authentication, natural language, the harness, and runsc are separate missing conditions. `caller` is `trusted_local_admin`, an operator config on this machine. The runtime membership grant is fixture SQL, not authentication. Passing this fixture does not close the product-wide goal.

## Resources

One new Rust builder, 6 GiB, 1 CPU. One private PostgreSQL 17.10, 1 GiB, 1 CPU. They do not run beside a second builder or a second database owned by this session. The global running rust-builder cap is 10. Host `cargo`, `rustc`, `rustdoc`, and `rustup` processes defer the launch.

Images, already local, no pull:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

`linux/arm64`. Toolchain text must start with `rustc 1.94.0 `. `--cap-drop ALL`. PostgreSQL adds only `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID`. No privileged mode, no host network, no published port, no Docker socket, no host home, no shared cargo cache, no auth environment, no password on argv. Private names:

| Use | Value |
| --- | --- |
| Owner label | `a317` |
| Session label | `eef831aa-dba4-4af2-a78b-541bc9e0a317` |
| Network | `oo-a317-eef831aa-net` |
| Host and alias | `oo-a317-pg` |
| Volumes | `oo-a317-eef831aa-cargo`, `oo-a317-eef831aa-target`, `oo-a317-eef831aa-pgdata` |
| Private tmp | `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317` |
| Databases | `oo_a317_src`, `oo_a317_dst` |
| Logins | `oo_a317_reader`, `oo_a317_writer`, `oo_a317_runtime`, `oo_a317_login` |
| Secret ref | `secret://pg/a317/reader` |
| Shard | `a317main` |
| Source id | `pubfix` |
| Versions | `ont-a317`, `cat-a317`, `pol-a317`, `know-a317` |

Cleanup removes only objects whose `oo.owner` is `a317` and whose `oo.session` is this UUID. It does not prune and does not remove a foreign name.

## Pre-edit smoke

Before any file under `testing/real-db/pg-e2e/` other than this contract, and before the chain, one builder runs the archived `tools/pg-onboarding` tests. `live_chain` stays ignored. The smoke fetches the locked tool crates into this session's cargo volume, then tests and Clippy run `--locked --offline`. Expected archived result: 24 passed, 0 failed, 1 ignored, Clippy clean, toolchain `1.94.0`. A different result is preserved and stops the chain.

## Source fixture

Schema `shop`. Three tables. 100000 rows in total. The operator source generation is 317. It is the cut id the import CLI must echo. It is not the row count.

`shop.party`, 2000 rows, index `i` from 0 inclusive to 2000 exclusive:

- `party_key text` primary key: `p` plus `i` in five digits (`p00000` through `p01999`)
- `display_name text` null: null when `i % 17 == 0`, otherwise `n` plus the same five digits
- `active boolean` not null: true when `i % 2 == 0`
- `phone text` not null: `tel-` plus the five digits. Personal. The wide role may read it. The narrow role may not.
- `national_id text` not null: `nid-` plus the five digits. No classification decision. Neither query role may read it.

`shop.account`, 8000 rows, index `j` from 0 inclusive to 8000 exclusive:

- `account_id int8` primary key: `j`
- `party_key text` not null, foreign key to `shop.party(party_key)`: the party key of `j % 2000`
- `opened boolean` not null: true when `j % 3 != 0`
- `label text` null: null when `j % 19 == 0`, otherwise `a` plus the decimal digits of `j`

`shop.entry`, 90000 rows, index `k` from 0 inclusive to 90000 exclusive:

- `entry_id int8` primary key: `k`
- `account_id int8` not null, foreign key to `shop.account(account_id)`: `k % 8000`
- `qty int8` not null: `k = 0` is `-9223372036854775808`, `k = 1` is `9223372036854775807`, `k = 2` is `0`, otherwise `k - 45000`
- `amount numeric(18,4)` null. `k = 3` is `1234567890.1234`. `k = 4` is `-1234567890.1234`. `k = 5` is `0.0001`. `k = 6` is `-0.0001`. Otherwise, when `k % 29 == 0`, null. Otherwise the minor units are `(k % 10000) - 5000` and the text is that integer divided by 10000 with exactly four digits after the decimal.
- `memo text` null: null when `k % 13 == 0`, otherwise `m` plus the decimal digits of `k`

`COMMENT ON TABLE shop.party` contains the marker `甲注评`. The marker must be absent from `catalog.json` and `captured-draft.json`.

The source login `oo_a317_reader` receives `USAGE` on `shop` and `SELECT` on the three tables, including the PII columns, so the snapshot can see them. It receives no `INSERT`, `UPDATE`, or `DELETE`. `oo_a317_writer` receives `INSERT` on `shop.party` only, as the control that the denial is the reader grant. Public has no privilege on `shop`. The target loader roles are `oo_fixture_admin`, `oo_a317_runtime`, and `oo_a317_login`. They are not the source reader.

The Rust program writes the source `COPY` text. The loader runs that copy into the source database. The target receives rows only from `init-pg` and `import-pg`.

Canonical comparison text is PostgreSQL `COPY` text, one row per line, tab separated, null as `\N`, boolean as `t` or `f`, ordered by the primary key ascending. `numeric(18,4)` text always has a sign only when negative and exactly four digits after the decimal. Values contain no tab, newline, or backslash.

## Chain

The driver calls the archived binaries. Config field names are the ones `pg-onboarding` already accepts.

1. `pg-onboarding inspect`. Success is `awaiting_review`. `source_cut` is 317. `mapping_run_generation` is 317. `target_fence` is not 317. The config identity is the SHA-256 of `oo-pg-config-identity/v1` over the non-secret fields and the import defaults `connect_ms=30000`, `statement_ms=60000`, `lock_ms=30000`, `max_rows=10000`, `max_bytes=8388608`, then the three tables in the order party, account, entry. Password and CA bytes are not inputs.
2. The driver binds declared decisions to `fact_fingerprint` values read from the captured draft. It accepts queue items whose kind is `structural_column`, `structural_primary_key`, or `candidate_relationship`, and skips ids that start with `pii/`, `semantic/`, `type/`, or `classification/`. Reviewer `by` is `public-fixture` and `at` is `2026-10-07`. `national_id` gets no classification decision. The other columns are decided as follows. `explicit_allow` is true only when `pii` is `personal` or `sensitive`.

| Dataset | Column | pii | semantic_type |
| --- | --- | --- | --- |
| `shop.party` | `party_key` | `none` | `identifier` |
| `shop.party` | `display_name` | `personal` | `person_name` |
| `shop.party` | `active` | `none` | `boolean_flag` |
| `shop.party` | `phone` | `personal` | `free_text` |
| `shop.account` | `account_id` | `none` | `identifier` |
| `shop.account` | `party_key` | `none` | `foreign_key` |
| `shop.account` | `opened` | `none` | `boolean_flag` |
| `shop.account` | `label` | `none` | `free_text` |
| `shop.entry` | `entry_id` | `none` | `identifier` |
| `shop.entry` | `account_id` | `none` | `foreign_key` |
| `shop.entry` | `qty` | `none` | `quantity` |
| `shop.entry` | `amount` | `none` | `quantity` |
| `shop.entry` | `memo` | `none` | `free_text` |
3. `pg-onboarding apply-reviewed` on the same inspect directory. Intended success is `ready_for_trusted_query`, `analyze_published` false, `estimate_is_exact` false, `database_rollback` false, `database_release` false, `database_state` `child_compilers_applied_no_release`. Both column-ACL effects are `installed`. Grantor is `oo_fixture_admin`. Wide provision is `installed` and narrow provision is `replay` when narrow is second. Hidden `shop.party.national_id` reason is `classification.absent`.

Wide role `oo_r_wide` sees every column except `national_id`. Narrow role `oo_r_narrow` sees `party_key`, `display_name`, `account_id`, `party_key`, `opened`, `entry_id`, and `account_id` only.

Embedded statements, and the exit the contract expects:

| Name | When | Exit | Stderr | SQL |
| --- | --- | --- | --- | --- |
| `wide-before` | before analyze | 5 | `query-pg: policy` | `SELECT party_key FROM shop.party ORDER BY party_key` |
| `wide-hidden` | after bundle | 6 | `query-pg: guard` | `SELECT national_id FROM shop.party` |
| `wide-insert` | after bundle | 6 | `query-pg: guard` | `INSERT INTO shop.party (party_key, display_name, active, phone, national_id) VALUES ('p99999', 'x', true, 'tel-x', 'nid-x')` |
| `wide-pos` | after bundle | 6 | `query-pg: guard` | `SELECT party_key FROM shop.party ORDER BY _oo_pos` |
| `wide-count` | after bundle | 0 | | `SELECT count(*) FROM shop.entry` |
| `wide-null` | after bundle | 0 | | `SELECT party_key, display_name, active FROM shop.party WHERE party_key = 'p00000'` |
| `wide-qty` | after bundle | 0 | | `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id` |
| `wide-fk` | after bundle | 0 | | `SELECT account_id, party_key FROM shop.account WHERE account_id = 0` |
| `wide-phone` | after bundle | 0 | | `SELECT phone FROM shop.party WHERE party_key = 'p00001'` |
| `narrow-party` | after bundle | 0 | | `SELECT party_key, display_name FROM shop.party WHERE party_key = 'p00001'` |
| `narrow-hidden` | after bundle | 6 | `query-pg: guard` | `SELECT national_id FROM shop.party` |
| `narrow-phone` | after bundle | 6 | `query-pg: guard` | `SELECT phone FROM shop.party` |
| `narrow-amount` | after bundle | 6 | `query-pg: guard` | `SELECT amount FROM shop.entry` |

`wide-count` returns `90000`. `wide-null` returns key `p00000`, a null display name, and active true. `wide-qty` returns the int8 minimum, the int8 maximum, and zero. `wide-fk` returns account `0` and party key `p00000`. `wide-phone` returns `tel-00001`. `narrow-party` returns `p00001` and `n00001`.

Every embedded statement uses `max_result_bytes` 8192 and `wall_clock_ms` 30000. `max_rows` is 20 on the four wide refusals and the three narrow refusals, 3 on `wide-qty`, and 1 on the other successful statements. Those limits sit inside the tool cap of 2000 rows and 262144 result bytes.

The archived `query-pg` cell set is null, bool, int, and text. `amount` is `numeric(18,4)`. It is not placed in a statement whose expected exit is 0. A separate fresh probe, `SELECT amount FROM shop.entry WHERE entry_id = 3`, records the real exit. A type refusal is a product seam, not a rewritten success.

## Independent checks

After a successful import, `import-result.json` row counts must be 2000, 8000, and 90000. The driver then asks the target administrator to `COPY` the physical `_oo` columns named by that result, in source column order, ordered by the primary-key column. The copy is compared, line for line, with the Rust canonical text. A mismatch names the first line number and stops. The driver does not update the expected file to match the target.

The product `canonical_digest` is recorded. This fixture does not reimplement the batch hasher and does not treat that hex as the gold.

A fresh `query-pg` process, with a new audit file, runs `SELECT count(*) FROM shop.party` as `oo_r_wide` after apply. The count is 2000. The audit has `admit` and `complete`, the SQL hash, and no password bytes.

After inspect has sealed the snapshot, one transaction as `oo_a317_writer` inserts a party key `p90000` and rolls back. One statement as `oo_a317_reader` inserts the same shape and must return SQLSTATE `42501`.

One cancellation: the reader starts `SELECT pg_sleep(60)`, the administrator cancels that backend, and the reader statement returns SQLSTATE `57014`. That is the only cancellation case.

One restart: the same PostgreSQL volume is restarted, then the fresh party count is repeated. The count stays 2000. The role-pack digest is unchanged.

Host epoch seconds around inspect and apply, and one `docker stats --no-stream` of the database after inspect, are written as measurements. The manifest `budget_defaults` are copied beside the actual row counts. Those numbers are not a budget certificate. The onboarding request does not pass a budget, so the import uses its built-in default of 10000 catalog rows and 8388608 bytes. This fixture does not edit that default.

## Failure

If inspect or apply exits non-zero, the run directory, child stdout, and stderr stay in the evidence tree. The driver does not change an expected exit, drop `numeric`, or shrink the row count to obtain a zero status. The report names the phase, the exit, and the static stderr detail. Minimum repro is the owned launch script plus this contract.

## Recipe origins

Launch and server wrappers start from the reviewed a274 chain, which itself records a204, a200, and a199. Hashes below were measured before this line's edits.

| Origin | Path | SHA-256 |
| --- | --- | --- |
| a204 preoverlay launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a200 smoke launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a199 manifest launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh` | `c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3` |
| a274 focused launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/linux/focused/launch.sh` | `e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910` |
| a274 focused inside | same review, `linux/focused/inside.sh` | `803a0608f4104f1147019e207a7e37bd5c23d0e37d470b1eb5f8cafd343629ac` |
| a274 pg-wrap | same review, `pg/pg-wrap.sh` | `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb` |
| a274 up | same review, `pg/up.sh` | `ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76` |
| a274 sha256 tool | same review, `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a274 manifest walker | same review, `tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a274 deb selector | same review, `tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| a274 libclang extract | same review, `linux/chain/extract-libclang.sh` | `982f18e430e6b8136a4cb30cb7e0711015318d0b40c0939475bfae8142e55ed4` |

Copied tools keep those digests. Shell changes are the UUID, the 6 GiB and 1 CPU limits, this network, and the fixture load. Server flags stay the a274 set.
