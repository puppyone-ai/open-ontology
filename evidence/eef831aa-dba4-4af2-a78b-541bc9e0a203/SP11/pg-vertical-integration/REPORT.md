# a203 PostgreSQL vertical integration

Verdict: API slice completed. `parent_acceptance` is false. `query_cli` is `not_exercised`. The import run stays `awaiting_publish`.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a203`. Fixed source `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. The product tree, root manifest, root lock, and the ledger were left unchanged. Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a203/SP11/pg-vertical-integration/`.

The result is `live/outcome.json` status `api_slice_completed`. Harness container `oo-a203-harness` exited 0. There is no `live/blocker.json`.

## Approved API path

One private PostgreSQL 17.10. TLS mode `verify-full`, negotiated TLSv1.3, SAN DNS `oo-a203-pg`. Source database `oo_a203_src`. Target database `oo_a203_dst`. Source role `oo_a203_reader`: login, `superuser` false, `create_role` false, `create_db` false, `bypass_rls` false, `replication` false. `registration_allowed` true. `block_reasons` empty. Write state `read_only`. Server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. Record: `live/access-report.json`.

Captured catalog protocol `oo.source.pgsql/v1`, database `oo_a203_src`, source id `a203ent`, schema hash `42c7ceab75c670f1d8d165efe3eb69ba99dbce5b8e407c6e26f2f724f06de5eb`. Comments are `byte_len` plus `sha256`. The catalog JSON does not contain the comment text. `live/oracle.json` sets `catalog_contains_comment_body` false.

Fixture, two tables:

- `ent.party`: `party_id` int8 primary key, `display_name` text, `active` bool, `credit_limit` int8, `internal_note` text. Table comment digest `76245388c7bd896c3d40c874157b2dd28802d70c904a224777cef089d5e4aace` (18 bytes).
- `ent.invoice`: `invoice_id` int8 primary key, `party_id` int8, `memo` text, `posted` bool, `amount` int8, `clerk_note` text. Foreign key name `invoice_party_fk`. Table comment digest `6e9da0a86dd9152be0fb6944efd6bce1ea8b4effb0e8ab250e1329e00de92a9b` (12 bytes).

Source readback in `live/oracle.json`, three rows each. Party includes `华能机电`, a null display name, `李四`, credit limits `9223372036854775807`, `-9223372036854775808`, and `0`. Invoice includes `首张发票`, a null memo, `负数边界`, and the same int64 bounds. Oracle sha256 `23fc44cafec8ebfe4fd4f47c4368549bddcb82a36914634e7c8ca190d6f80c8a`.

`prepare_snapshot` used generation 1. Target `install` once returned `bootstrapped` (`install_calls` 1). `install_full_import` once returned `installed` (`full_import_calls` 1). `import_unpublished` run id `7dba3fda-8dba-4cbd-83a6-a7ac0382bda0`. Seal state `awaiting_publish`. `pg_catalog.pg_publication` count was 0. `shadow_marked_published` false. Config digest `3033ccada1eef250b5f550f8cdfde039b3e9897e7b3e0489779f3adec3490859`. Source generation 1. Target generation 1.

Mapped shadow tables, three rows each:

- `ent.party` → `_oo.pt_d07a54b9f66626e18bc0b460df32f874bfcf08ebaf552350c24e428cb24d`, canonical digest `1a033f73481a8099a16b72a6d2a8d7f57e82aca3b870fb2b1f99fbf1907ca025`.
- `ent.invoice` → `_oo.pt_d9e20ef093d36d42e808074ca6d002eabc15e78a810f3516e38df2c0e878`, canonical digest `e201aeb50cda3c3dd761f41924222ebb7fba93909cb34f641b33115106185138`.

The harness compared the source tables and the shadow user columns, including `internal_note` and `clerk_note`, with the fixture. `run.exit` is 0, so those comparisons matched.

`compile_captured_pg_catalog` produced protocol `oo.builder.structure-draft/v1`. `label_attempt` is `chinese_labels_accepted`. Effective datasets: `ent.party` label `交易方` description `企业主体名录`, primary key `party_id`; `ent.invoice` label `发票` description `发票单据名录`, primary key `invoice_id`. Relationship `invoice_party_fk` status `draft`. Column `pii`, `classification`, and `semantic_type` are null. `provenance.publication` is `unavailable`. `provenance.pg_catalog_integration` is `not_accepted`. `llm` is `not_used`.

Review queue, 37 items, every `publishable` flag false:

| kind | state | count |
| --- | --- | --- |
| structural_column | accepted | 11 |
| structural_primary_key | accepted | 2 |
| candidate_relationship | accepted | 1 |
| pii_unknown | pending | 11 |
| semantic_unknown | pending | 11 |
| curated_unbound | pending | 1 |

`review_note`: `one_review_per_item_id blocks override beside accept:relationship/ent.invoice/invoice_party_fk`. The relationship accept and a curated override share one item id, so the override stayed pending. Dataset labels were applied.

Column ACL used the existing policy API with this bridge, recorded in `live/column-acl.json`: `trusted administrative classification; ontology compiler left pii and classification unset`. Both effects `Installed`.

- `oo_r_party_read` on the party physical table, four physical columns. `internal_note` is absent.
- `oo_r_invoice_read` on the invoice physical table, five physical columns. `clerk_note` is absent.

`load_policy_catalog` ran twice, one role each. `policy_id` equals the config digest. Project `a203ent`. `estimated_rows` 0. `pending_unclassified` 2 on each relation (hidden note plus `_oo_pos`). Visible type OIDs: int8 20, text 25, bool 16. Physical schema `_oo`. Large-table threshold `1000000`.

`DedicatedSession::execute_trusted_select` for each role:

- `SELECT party_id, display_name, active, credit_limit FROM ent.party` returned 3 rows.
- `SELECT count(*) FROM ent.party` matched the fixture count 3.
- `SELECT * FROM ent.party` returned 3 rows.
- `SELECT invoice_id, party_id, memo, posted, amount FROM ent.invoice` returned 3 rows.
- `SELECT count(*) FROM ent.invoice` matched the fixture count 3.
- `SELECT * FROM ent.invoice` returned 3 rows.

Guard refusals, recorded as class `guard_refusal`:

- `SELECT internal_note FROM ent.party`: `column is not visible: ent.party.party_id, ent.party.display_name, ent.party.active, ent.party.credit_limit; 2 columns are pending classification`.
- `INSERT INTO ent.party ...`: `write statement is not allowed`.
- `SELECT clerk_note FROM ent.invoice`: the same pending-classification form for the invoice columns.
- `INSERT INTO ent.invoice ...`: `write statement is not allowed`.

Direct database SQLSTATE 42501 was required by the harness and held, because `run.exit` is 0. The SQLSTATE itself is not copied into `outcome.json`. The checks are: source reader `INSERT INTO ent.party` then a count that stayed 3; each target login `SET ROLE` then `SELECT` of the hidden physical column; each target login `INSERT` into the physical table.

Audit table `public.a203_audit`. `DbAudit::admit` inserts phase `admit` on a second TLS session before the executor reads rows. `DbAudit::complete` inserts phase `complete:{status}:{sqlstate}`. For each of the six successful statements the harness required the admit count to rise by one before the first row and the complete count to rise by one after drain, with status `succeeded` and `end_audit_ok`. Guard refusals required the audit row count to stay unchanged. Those checks passed inside the process. The totals were not written to a file.

## Recorded seams

`compile_role_pack` rejected three inputs. All three have `compiled` false in `live/rolepack-attempts.json`.

| input | code | path | message |
| --- | --- | --- | --- |
| structure_draft_json | `oo.builder.role-pack/v1/schema.rejected` | `/` | the effective document does not match the role-pack contract |
| compiler_datasets_and_relationships | `oo.builder.role-pack/v1/classification.unknown_pii` | `/datasets/0/columns/0` | unknown PII stays hidden and is not classified by this compiler |
| compiler_datasets_only | `oo.builder.role-pack/v1/classification.unknown_pii` | `/datasets/0/columns/0` | unknown PII stays hidden and is not classified by this compiler |

The structure draft is not an `oo.builder.effective-oos/v1` document. Feeding compiler datasets still fails on unknown PII. This run did not invent PII or classification and then call the pack compiler as a self-build. The column ACL values are the administrative bridge above.

Semantic publication stays `unavailable`. The fixed snapshot keeps `pg_catalog_integration` at `not_accepted`. The query CLI at a198 was not read and was not run. This slice does not close the M1 vertical goal or a parent acceptance.

The next owned compiler, when a later task takes the seam, is a builder path that emits `oo.builder.effective-oos/v1` only after a reviewed contract sets PII and classification. This integration did not add that compiler and did not change product code.

## Images, commands, exits

Pre-edit smoke, container `oo-a203-rust194` `27182e6c011d8d69696187960da94fbff5d03a9ca0ff24bbf08cea2a27270b45`, exit 0, OOM false, privileged false, network `oo-a203-net`, capability drop `ALL`, no host ports. Finished `2026-10-05T18:10:35.979252591Z`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, Debian 13, LLVM 21.1.8. `oo-pipeline` lib tests: 11 passed. `oo-gateway` lib tests: 17 passed. Phase exits under `linux/phases/` are 0 for fetch, libclang, libclang-load, test-pipeline, test-gateway, clippy-component, clippy-pipeline, and clippy-gateway. Archive SHA in `linux/archive-sha.txt` is the fixed base. Libclang root `/cargo-home/a203-clang-root/usr/lib/llvm-19/lib`. `linux/libclang-ldd.txt` has no `not found` line.

Harness container `oo-a203-harness` `480a6081957a1faa07190d7b23a8394edbb133067e3eace60355d69dfff08a74`, exit 0, OOM false, privileged false, network `oo-a203-net`, capability drop `ALL`, memory 8589934592, no host ports. Finished `2026-10-05T20:00:26.777289221Z`. Same Rust image. Resolve mode `standalone` (`linux/resolve-mode.txt`). `linux/lock-registry-added.txt` is empty. `linux/lock-registry-removed.txt` lists 56 registry triples that the frozen root lock contained and the harness lock does not. `lock-diff.exit` is 0. Build finished in 4.03s. Clippy `--offline --locked --all-targets -- -D warnings` finished in 0.63s and exited 0. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` lint expectations (`flush.rs:11`, `message/mod.rs:76`); the phase exit stayed 0. Run exit 0. Secret-scan exit 0. `linux/harness-run.log` is 0 bytes; the binary wrote `live/`.

PostgreSQL container `oo-a203-pg` `2fa76563fc845a20748c3e6a48c9fd693d33b841994017143d4af8b3e80e2bae`, running at inspect time, OOM false, privileged false, network `oo-a203-net`, memory 1073741824. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Capability drop `ALL`. Capability add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. `5432/tcp` host binding is null. `pg/up.exit` is 0. Certificate SAN `DNS:oo-a203-pg`.

Launch capacity files recorded host cargo 0, running Rust builders 0, script cap 6. This task kept one builder and one PostgreSQL.

## Attempts kept

Smoke attempt 1, `linux/attempt-1/`: pipeline lib test exit 101 because the archive omitted `contracts/`. Gateway lib tests exited 0. The rerun archives `contracts/` and is the exit-0 smoke above.

Harness attempt 1, container `2a1a2907677aa30aac27282c82cc23ef2828b7ac8fa1c246fc2300ca79a7d7c8`, exit 101, OOM false. Two `E0521` errors. `build.exit` 101, `build-overlay.exit` 101. `linux/harness-attempt-1/`.

Harness attempt 2, container `26de7d43d29de8c0205e863ed16ba6ffb36b2ce6caa0d20b7fabd1dc99b279e6`, exit 1, OOM false. `build.exit` 0, `lock-diff.exit` 0. Clippy log: `cargo-clippy` is not installed for toolchain `1.94.0-aarch64-unknown-linux-gnu`. `clippy.exit` 1. The `build-overlay.exit` value 101 in that directory is the stale file carried from attempt 1. `linux/harness-attempt-2/`.

Harness attempt 3, container `4fd76226e85fdd3a6d4b556df787be034c788ed9daf19da00e2764f7698bcf7c`, exit 101, OOM false. `build.exit` 0, `lock-diff.exit` 0, `clippy-component.exit` 0, `clippy.exit` 101. Clippy: `too_many_arguments` (8/7) on `policy_for`. `linux/harness-attempt-3/`.

The exit-0 harness is the fourth container. Product code stayed at the fixed archive across every attempt.

## Limits

CLI acceptance waits for a fixed a198 commit. Row filters, a P15 control mirror, and semantic publication were not part of this slice. The ontology draft is an agent-context document. No release is claimed.
