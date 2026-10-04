# Findings

Reviewed source `d3429726618a4e69ada7fb35131f9043a21e23e9`. Deciding run: `linux/probe-4/docker-run.exit` = 0 and `linux/probe-4/live/oracle.txt`. Verdict: **APPROVE LIMITED** for the public column-ACL primitive.

## F1. Fresh install on a different fixture

The harness calls `compile_column_acl` and `install_column_acl` on a trusted TLS client. The fixture is `clinic` / `ward` / `visit`, generations 4 and 11, with decoys at 99 (same source, other digest) and 50 (other source, same digest). The author fixture (`orders` / `logistics` / `shipment`, generations 3 and 7) was not reused, and `policy_pg_live` was not executed.

`generation_scoped=11` while `generation_source_only=99` and `generation_digest_only=50`. Generation 11 installed. Generation 4 returned `StaleGeneration { policy: 4, current: Some(11) }`. `current_generation` filters `source` and `config_digest` together (`policy.rs` lines 590–603). A missing filter would have reported 99 or 50 and the assertion would have failed.

Row filter and unknown PII returned before any grant. Their transaction footprint matched a no-SQL calibration (`commit=4 rollback=0`). Stale generation, missing login, and a superuser login matched one autocommit read plus one rollback (`commit=6 rollback=1`). `oo_r_filtered`, `oo_r_mrn_only`, `oo_r_old_reader`, `oo_r_gap`, and `oo_r_half` were absent. The catalog after the run lists only `oo_r_picker` and `oo_r_scribe`.

Picker received `SELECT` on `visit_id` and `wing`. Scribe received `SELECT` on `visit_id`. Shrink and expand returned `GrantMismatch` and left `attacl` unchanged. Role flags for both `oo_r_` roles are all false. The login role is `INHERIT`. Membership is inherit false, set true, admin false, grantor `oo_a173_grantor`. `relacl` names `oo_target_owner` and `oo_pipeline_runtime` insert. It does not name the reader roles, the login, or `PUBLIC`.

Picker read `8801` and `north`. Scribe read `8801` and got `42501` on `wing`. The same backend pid 237 remained after `ROLLBACK`, `RESET ROLE`, and `RESET ALL`. Without a role, `visit_id` was `42501`. `mrn`, `_oo_pos`, a column added after install, `INSERT`, `UPDATE`, `DELETE`, `_oo.physical_mappings`, `_oo.full_import_run`, and `lookup_physical_mapping` were `42501` while `transaction_read_only` was `off`. `SET LOCAL oo.claims = 'wing=north'` completed in the harness and `mrn` stayed `42501`. The library source has no `oo.claims` and no `set_config`.

`compile_column_acl` refuses a row filter at `policy.rs` lines 226–228. `create_role` uses `NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION` at lines 698–705. `grant_membership` rejects a missing or superuser login at lines 720–726.

## F2. TLS behavior and the public error text

`SslMode::Require` is line 324. `with_no_client_auth` is line 302. There is no `NoTls` path. The other CA and the name `oo-a173-other` were rejected. The oracle class is `handshake_reason_omitted` for both. `db_error` (lines 546–557) stores `err.to_string()`, which on this driver is `error performing TLS handshake` and does not carry `UnknownIssuer` or `NotValidForName`.

The connection with the fixture CA and server name `oo-a173-pg` observed `pg_stat_ssl.ssl = true` for the grantor and for reader pid 237. TCP without TLS was rejected by `pg_hba.conf`. The library takes a `Client` the caller already trusts. It does not authenticate a principal. This is an observation about error text, and it is not a reason to withhold the limited approval: both bad certificates failed closed, and the matching certificate was checked in the database.

## F3. Grantor display and replay membership

The tested grantor `oo_a173_grantor` is a superuser and is not `oo_target_owner`. `InstallOutcome.grantor` and `pg_auth_members.grantor` are `oo_a173_grantor`. Column `attacl` shows `/oo_target_owner`. That string is the table owner PostgreSQL records when a superuser grants. The library does not `SET ROLE`. A non-superuser grantor was not run. The caller must already be able to execute `_oo.lookup_physical_mapping` and to grant on the shadow.

`install_in_transaction` calls `grant_membership` after the existing-role branch (`policy.rs` around lines 389–400 and 708–734). Replay of picker left the member count at 1. A second login was not granted. Column shrink did not change `attacl`.

## F4. Supply and the adapter name

Twenty-four lock pairs were added and none were removed. Checksums match the lock, the author tsv, and crates.io. The eight named checksums in `supply/unchanged-checksums.tsv` are unchanged. No added crate is yanked. No added crate was created on or after 2026-09-28.

`ci/deny.toml` allows Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. The published strings `MIT/Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, and `BSL-1.0` were not passed through `cargo deny`. `cargo deny` and `cargo audit` were not run. This probe is not a supply-chain release approval.

`tokio-postgres-rustls` is outside `is_gateway_postgres_stack`. The three focused architecture tests passed on the pre-overlay archive. The gate was not edited, and a missing taxonomy name is not an approval to treat the adapter as classified.

## F5. Preserved failures

`pg/up.exit` is 2 because the first `pg_hba.conf` write was empty. The reload did not replace the running file. `pg/repair.exit` is 0, and the repaired file requires TLS for TCP. `linux/probe`, `linux/probe-2`, and `linux/probe-3` remain. Their notes describe harness assertions. The green log is `linux/probe-4`.
