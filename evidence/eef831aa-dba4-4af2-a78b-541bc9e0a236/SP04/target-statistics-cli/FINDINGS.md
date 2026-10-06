# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a236`. Product commit `6ac2216505723b51ae116b1938aca47846ade35f`. The deciding proof is `linux/proof/`. Attempts 1 and 2 are unchanged under `linux/proof/attempt-1/` and `linux/proof/attempt-2/`.

## F1. The command is an autobin over the existing maintenance call

`analyze-pg` reads one `oo.pipeline.analyze-pg/v1` document and calls `oo_pipeline::target_statistics::analyze_mapped_shadows`. Stdout is `oo.pipeline.analyze-pg-result/v1` with the library receipt fields. `published` and `estimate_is_exact` are copied. A true value exits 1 `target statistics` and is not rewritten to false. The process opens its own verify-full connection and exits after the call. `disposition` is the library value. This run recorded `reusable`.

`oo-pipeline.rs`, `target_statistics.rs`, `import_cli.rs`, `init-pg.rs`, and both manifests were left at the fixed archive. Smoke hashed them. Proof `base-match.exit` is 0 and `Cargo.lock` did not change.

## F2. Closed checks run before either secret file is opened

Attempt-free unit tests cover extra arguments (exit 2 `usage`), truncated JSON, a bad protocol (`invalid version`), a secret field, a null or float where an integer is required, project mismatch, a bad digest, generation 0 (`identity`), an empty list, a duplicate, nine tables, a timeout outside 1..=30000, a closed logical name (`bound`), `connect_ms` 0, a bad host, and a bad operator (`invalid input`). Password and CA failures use a missing file, a directory, a symlink, an empty file, a NUL, 1025 bytes, a private-key PEM, and a non-certificate. The marker `s3cret-a236-marker` and the decoy environment values stay out of stdout and stderr.

## F3. Import registration refuses a security-definer audit function

Attempt 2 reached compiled `init-pg` and then `import-pg` exited 1: `oo-pipeline: source privilege registration_refused`. `PgConnector::prepare_snapshot` refuses registration when `inspect_access` sees a `SECURITY DEFINER` function. The a219 library fixture used that function attribute. The reviewed `import-pg` fixture in `import_pg_cli_live.rs` uses an ordinary `plpgsql` trigger.

The deciding fixture follows that import command: `app.note_write` is not `SECURITY DEFINER`. The two setup inserts still write `app.write_audit`. After import, the refusals, and `ANALYZE`, `source_audit_delta=0`, `source_reader_writes=0`, and source `app.party` `reltuples` stayed `-1`. The runtime login also received `CONNECT` on `oo_a236_dst`, matching the import command fixture. `oo_pipeline_runtime` already has that grant from migration `0000`.

## F4. The live chain stayed unpublished and inexact

Order on `oo_a236_dst`: `init-pg` bootstrapped and installed full import, `import-pg` returned `awaiting_publish` with 2 rows and generation 11, `reltuples` was the `-1` sentinel, then `analyze-pg` as `oo_a236_admin` returned a finite estimate. Bits `1073741824` are `2.0f32`. `estimated_rows` was 2 and `count(*)` was 2. `estimate_is_exact` stayed false. A second call stayed unpublished with the same bits. `public.sentinel` stayed `-1`.

Refusals, each with `reltuples` still `-1` afterward:

- wrong CA: exit 4 `target connect`
- `oo_a236_runtime`, member of `oo_pipeline_runtime`: exit 5 `runtime caller`
- `oo_a236_plain`, `CONNECT` and `USAGE` on `_oo`, no `MAINTAIN`: exit 5 `target role`
- generation 12: exit 6 `stale generation`
- logical table `absent`: exit 6 `mapping absent`

## F5. An open transaction is not a command input

The command dials a new connection. A caller cannot pass an open transaction on stdin. `StatisticsError::OpenTransaction` maps to exit 6 `open transaction`. The proof does not `BEGIN` and sleep. Lock timeout, statement timeout, forged relation, and a read-only caller stay mapped in `contracts/pipeline/analyze-pg-cli-v1.md` from the existing library errors. This run did not add those campaigns.

## F6. The catalog query is an evidence harness

`oo-gateway` is not an `oo-pipeline` dev-dependency, and this task does not edit a manifest. `overlay/catalog_proof.rs` is copied into the proof archive only. It calls the existing `compile_column_acl`, `install_column_acl`, `load_policy_catalog` with `UnknownRows::Refuse`, and `DedicatedSession::execute_trusted_select` for `SELECT count(*) FROM app.party`. The query returned 2. `statistics_published=false`. The harness is not in the product commit. Gateway sources in the product commit are untouched.

## F7. Vendor clippy noise is pre-existing

`sqlx-postgres` prints two unfulfilled `#[expect(dead_code)]` warnings during the proof compile. `cargo clippy -p oo-pipeline --bin analyze-pg --test analyze_pg_cli -- -D warnings` exited 0 on the deciding run. The vendor tree was not edited.

## F8. Scope this commit leaves open

M1-D06A is already accepted elsewhere. This evidence does not accept M1-D06B, does not edit the plan ledger, and does not close M1-I01, M1-I02, or a parent goal. There is no publish command and no new dependency.
