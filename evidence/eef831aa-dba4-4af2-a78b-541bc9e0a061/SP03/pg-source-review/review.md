# V01-PG-01 review a061

Decision: **request_changes**.

This decision covers the PostgreSQL read-only source connect, AccessReport, and catalog in commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. It leaves SP03-W01, PostgreSQL 13 support, CDC, read chunks, the source SDK, P14, and the whole plan unsigned.

| Identity | Value |
|---|---|
| Commit | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| Tree | `68fbc1b863e4ae46695bd71b52bb4af396adcdfe` |
| Parent | `4213134d136ff50bd2f7d422f3f840e48e038b6d` |
| Subject | Add the PostgreSQL read-only source catalog |
| Source worktree | `/Users/redux/.codex/worktrees/product-pg/open-ontology` |
| Archive | exact `git archive` of that commit, 199 files, 0 blob mismatches |

The source worktree, index, and HEAD were clean on this commit before the run and after cleanup. `git status --porcelain` was empty both times. No source file, push, or pull request was made.

## Why this is request_changes

Three platform defects are reproduced against the committed crate. The official live test still passes, because its fixture never exercises them.

### 1. `registration_allowed` admits credentials that can change the source

`docs/03` I-SRC-3 refuses registration of a credential that can write business tables. REQ-SRC-02 and the PostgreSQL row in `docs/03` §5.2 require the check to cover inherited roles, ownership, schema `CREATE`, superuser, `BYPASSRLS`, and write-capable functions. Unknown facts must block.

The implementation checks table DML, column `INSERT`/`UPDATE`, and schema `CREATE` only for tables in the caller’s selection, and it loads `SECURITY DEFINER` functions only from `public` or those selected schemas (`sql.rs` `DEFINERS`). Measured on PostgreSQL 17.10, with `has_*_privilege` from the local superuser as the independent oracle:

| Login | Oracle | `registration_allowed` | Report |
|---|---|---|---|
| `oo_src_side` | `INSERT` on unselected `app.side_write` | true | `write` `read_only`, no findings |
| `oo_src_hiddenfn` | `EXECUTE` on `hidden.boom()`, `SECURITY DEFINER`, body inserts | true | `read_only`, definer list empty |
| `oo_src_schemacreate` | `CREATE` on schema `side`, which holds no selected table | true | `read_only` |
| `oo_src_dbcreate` | `CREATE` on database `source_lab` | true | `read_only` |

The same build blocks the cases the official fixture covers: inherit and inherit-only (`INHERIT TRUE`, `SET FALSE`) as `inherited`, transitive inherit-only as `inherited`, `SET`-only as `set_role`, column-only `INSERT` as `COLUMN_WRITE`, and `SUPERUSER`, `BYPASSRLS`, `CREATEROLE`, and `CREATEDB` as `privileged_role`. A membership with `INHERIT FALSE` and `SET FALSE` stays allowed. Those controls are real. They do not make the four rows above safe to import.

Minimal repro: after the committed fixture, create a login with `INSERT` on one table outside `TableSelection`, call `inspect_access` on the published selection, and read `registration_allowed`. It is true. Repeat with `EXECUTE` on a `SECURITY DEFINER` function in a schema other than `public` and the selected schemas.

### 2. `PGOPTIONS=-c role=...` makes a write-capable login look read-only

`PgConnector::connect` starts from `PgConnectOptions::new()`, which copies `PGOPTIONS`, `PGSSLCERT`, and `PGSSLKEY` (`sqlx-postgres` 0.9.0 `options/mod.rs`). The connector then sets host, port, username, password, database, application name, `VerifyFull`, and the caller CA. It does not clear `options` or the client certificate fields (`session.rs`).

Repro, after dropping the later public definer so the reader role itself is clean:

- Login `oo_src_dual` inherits `oo_src_writer` with `INHERIT TRUE`, `SET TRUE`, and can `SET ROLE` to `oo_src_reader`.
- Without `PGOPTIONS`, `inspect_access` returns `registration_allowed` false, block `write_capable`.
- With `PGOPTIONS=-c role=oo_src_reader`, the same login returns `current_user` `oo_src_reader`, `session_user` `oo_src_dual`, `registration_allowed` true, `write` `read_only`, and no findings.

The version guard holds. `PGOPTIONS=-c server_version=13.14` fails the connection with `SourceErrorKind::Protocol` and SQLSTATE `55P02`. There is no caller field that supplies a fake server version. `registration_allowed` is still a public field on the returned struct, so an in-process caller can mutate it after the call. The server did not accept a forged version.

`PGSSLKEY` set alone, with no client certificate, fails closed as `SourceErrorKind::Config` / `driver_configuration`. The environment is enough to change the connect result. `PGSSLMODE=disable` does not turn TLS off: `pg_stat_ssl` for `application_name=oo-pg-source` showed `ssl=t`, `TLSv1.3`, while that process had `PGSSLMODE=disable`.

### 3. A caller CA is added to the Mozilla root set

`docs/17` requires enterprise CA and hostname verification. The locked handshake in `sqlx-core` 0.9.0 `src/net/tls/tls_rustls.rs` builds `cert_store` from `webpki_roots::TLS_SERVER_ROOTS` when feature `webpki-roots` is on (lines 211–214) and then adds the caller PEM (lines 140–151). The product enables `tls-rustls-ring-webpki`. `cargo tree --edges features` shows `sqlx-core` feature `webpki-roots` and crate `webpki-roots` 1.0.9. `TlsReport.trust` is the literal `configured-ca-plus-webpki-roots`.

Wrong CA and wrong hostname both return `SourceErrorKind::Tls` / `tls_handshake`. Those controls show that the private CA is required in addition to the public roots. They do not show a private-CA-only store. This review did not present a certificate signed by a public Mozilla root; the store composition is the locked code path above. There is no feature in this crate that selects the empty-root configuration the sqlx comment mentions and does not use.

## What held

Linux arm64, pinned image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8.

Focused smoke from the archive, no database: `cargo fetch --locked` 0, `cargo test -p oo-connectors --locked --offline --lib` 0, `--doc` 0, `--test live_pg --no-run` 0. Worst exit 0. Window `2026-10-01T22:36:31Z` to `2026-10-01T22:38:01Z`.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`. `server_version` `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` `170010`, `ssl` `on`. `pg_hba.conf` was `local all postgres trust` plus `hostssl all all all scram-sha-256`. No host port. Memory 512MiB, 1 CPU, pids limit 256.

Official gate, same cargo home, archive read-only: clippy component install 0, `cargo test -p oo-connectors --locked --offline` 0 (13 unit tests, `live_source_catalog` passed, 0 doc tests), `cargo clippy -p oo-connectors --locked --offline --all-targets -- -D warnings` 0, feature tree 0. Window `2026-10-01T22:48:36Z` to `2026-10-01T22:48:56Z`.

Catalog, reader, full selection, hash `b0dbcffe78f8818af5c72f37c2db048c7058ccfaf8eb1eef4b17b83ac37d9692`, stable across a second snapshot. Seven tables. `type_conversion` `unsupported`. Primary keys stay in key order: `customer(id)`, `order_line(order_id, line_no)`, `客户.订单(编号)`. `reading` has no primary key. `numeric` unconstrained is `UnboundedOrWideNumeric` with `typmod` `-1`. `numeric(80,10)` is `typmod` `5242894`, precision 80, scale 10, same reason. `numeric(10,2)` is `KnownBuiltinNotConverted`, precision 10, scale 2. Enum `order_status` labels `open`, `closed`, `EnumNotConverted`. Domain `sku_code` base oid 25, `DomainNotConverted`. Collation `C` is kept on `app.collated.note`. Quoted names map through `pgident/v1`: `客户` is `q_e5aea2e688b7`, `订单` is `q_e8aea2e58d95`. NFC-distinct columns on `app.collide` return `identifier_collision`. Adding `app.hashme.extra` changes the hash from `681782a9e0ed0df90c46b98e37909dcc46ab04e3d6630ffe931c89f79429d96f` to `b1fd5282c6aff896925915febe81b7a6c1e9ddb318bc6cf5a0887077630d40a2`.

RLS on `app.customer` leaves `hidden_pii` in the catalog with `select` false. The comment canary and the probe instruction string are absent from catalog JSON and Debug. The comment is present as sha256 `99a042fad752401594c711cdc674f22470412394789d6e485622b0de7f264d50`. Empty `TableSelection` and empty database `source_empty` both hash to `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`. A visible table with no grants, `oo_src_denied` selecting `app.customer`, returns one table and hash `55104e40a6f523047cc370e49ada75d0519523ac50cf239258aab418064e2666`, with every column `select` false. A missing selected table is `selected_table_missing`, for both inspect and catalog.

`PgSourceConfig` Debug in the probe rejected a password or a PEM certificate banner before connect. The official test’s closed-string assertions passed. Evidence copies were scanned for every generated password and for certificate markers; the scan found none.

SQL audit of `application_name=oo-pg-source` parsed the whole statement, not the first token. Official window, before adversarial DDL: 271 statements, `SELECT` 229, `WITH` 12, `START` 15, `ROLLBACK` 15, users reader, inherit, setonly, definer, databases `source_lab` and `source_empty`. Full review window, including the probe: 1158 statements, `SELECT` 988, `WITH` 62, `START` 54, `ROLLBACK` 54, across the fixture and probe roles. String literals were removed before the write-keyword scan so privilege names such as `'INSERT'` are not counted as writes. Flagged write words, `WITH` write CTEs, and side-effect calls (`nextval`, `lo_*`, `dblink`, `pg_write_file`, `pg_read_file`): 0. `set_config` appears 51 times inside `SELECT`, setting `statement_timeout`, `lock_timeout`, `idle_in_transaction_session_timeout`, and `default_transaction_read_only=on`. That is a session GUC. No `COMMIT`, `SET`, `INSERT`, `UPDATE`, `DELETE`, `COPY`, or DDL from this application name.

`sqlx` 0.9.0 depends on `sqlx-core` with feature `migrate` even when the facade’s default features are off. The feature is present in the tree. No migrate statement ran.

## Supply chain, separate from the product decision

Lock versus parent `4213134`: 248 packages, 112 added, 0 removed, all 112 from crates.io. Every added checksum matches the sparse index cache. None are yanked. Youngest publish time is `rand` 0.10.3 at `2026-09-20T06:37:47Z`, 277.8 hours before the author’s floor instant `2026-10-01T20:26:52Z`. No added crate is under 168 hours at that instant.

`cargo-audit` 0.22.2, temporary `CARGO_HOME`, exit 0. Advisory database commit `6de4455103aced2cba86e3b86e5c090b22827cf1`, updated `2026-10-01T22:25:27+02:00`, 1279 advisories, 248 dependencies, 0 vulnerabilities.

`cargo-deny` 0.20.2, `ci/deny.toml` unchanged, `--locked check`, exit 4. Advisories 0. Sources 0. Bans 12 warnings for duplicate versions (`base64`, `block-buffer`, `cpufeatures`, `crypto-common`, `digest`, `getrandom`, `hashbrown`, `r-efi`, `sha2`, `syn`, `synstructure`, `windows-sys`). License errors, which `deny.toml` does not allow:

| Crate | Expression | Why deny fails |
|---|---|---|
| `ring` 0.17.14 | `Apache-2.0 AND ISC` | `ISC` is required, and it is not in the allow list |
| `rustls-webpki` 0.103.15 | `ISC` | same |
| `untrusted` 0.9.0 | `ISC` | same |
| `webpki-roots` 1.0.9 | `CDLA-Permissive-2.0` | not in the allow list |

`ISC` is the standard ISC text (copied). It is an OSI permissive license, so the spec’s permissive-license rule and `ci/deny.toml` disagree. `CDLA-Permissive-2.0` lets a recipient use and share the CA data and requires the agreement text to travel with that data. It is not a GPL or a GPL-with-exception. The exception text that does appear, `wasi` 0.11.1 `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`, passes deny because Apache-2.0 and MIT are allowed. `whoami` 2.1.3 is `Apache-2.0 OR BSL-1.0 OR MIT`. Deny passes it through MIT. The BSL-1.0 text is copied and is not treated here as the selected license. `r-efi` 6.0.0 is `MIT OR Apache-2.0 OR LGPL-2.1-or-later` and passes deny through MIT or Apache-2.0. `docs/02` and `docs/10` keep AGPL, BSL, ELv2, and SSPL out of the default image. The BSL and LGPL sides are alternatives on those two crates, and the selected side for each can be MIT or Apache-2.0. `deny.toml` was not edited.

Build scripts in the added set: `ring`, `rustls`, `getrandom`, `crossbeam-utils`, `icu_normalizer_data`, `icu_properties_data`, and the `windows_*` 0.52.6 crates. The Windows crates are target-specific and were not the arm64 build that ran here.

Author raw logs and the author’s `audited: false` note stay unsigned. The exits above are from this run.

## API handoff for the next import consumer

Crate `oo-connectors`. Protocol `oo.source.pgsql/v1`. Calls: `PgSourceConfig::try_new`, `PgConnector::new`, `PgConnector::connect`, `PgSession::inspect_access`, `PgSession::catalog`.

Do not import a source on `registration_allowed == true` from this revision. That flag is true for a tested 17.10 session when the in-process block list is empty. It is false for the official reader only inside the fixture’s grants. It is true for a login that can `INSERT` outside the selection, `CREATE` outside the selected schemas, `CREATE` on the database, or `EXECUTE` a definer outside `public` and the selected schemas. It is also true when `PGOPTIONS` has already `SET ROLE` to a read-only role while `session_user` is still write-capable.

Use `schema_hash` (`pg-catalog-hash/v1`) as the structural change detector. It ignores comments, privilege booleans, and `captured_at`. Treat `UntrustedEvidence` as a hash and a length. Domain, enum, unconstrained numeric, and numeric precision above 38 stay explicit and `converted: false`. `numeric(10,2)` is also unconverted.

Capabilities `scan`, `bounded_read`, `cdc`, `type_conversion`, and `registry_api` are unsupported. `source_ddl` and `source_write` are forbidden. This crate must not send DDL or table DML. The compiled `migrate` feature does not change that observation, and it remains a reason to keep the session audit on the next revision.

TLS hostname verification is on and `sslmode` in the environment does not disable it. Trust is the caller CA plus the Mozilla webpki roots. A private-CA-only deployment cannot get that policy from this API.

Clear `PGOPTIONS`, `PGSSLCERT`, and `PGSSLKEY` in the process that calls `connect`, or wait for a build that clears them inside `connect`. Compare `session_user` and `current_user` before trusting the report. The tested version string is only `17.10` with `server_version_num` 170010. A startup `server_version` override is rejected by PostgreSQL.

## Resource cleanup

Own labels: `project=open-ontology`, `owner=a061`, `scope=pg-source-review`, `run=35f484e9-a478-4a67-b2c5-4961380387b4`.

| Resource | ID | End state |
|---|---|---|
| MSRV smoke | `e82b3c55f64b` | exit 0, removed before PostgreSQL started |
| Official gate | `80a478aa47fd` | exit 0, removed |
| PostgreSQL 17.10 | `aaa13010c335` | removed after the logs and probe JSON were copied |
| Role-forge probe | `195ccdb38330` | exit 0, removed |
| Network | `26cadb45f2e3` | removed |
| Secrets and server log | `/tmp/oo-a061-35f484e9-a478-4a67-b2c5-4961380387b4/{secrets,pglog}` | removed; the server log contained adversarial password literals and was not copied |

The first probe container and the extra-case container exited 0 and were removed before the final inventory. Their stdout and JSON are in this evidence directory. No foreign container was stopped. No prune, host network, docker socket, privileged mode, or home mount was used. At smoke start one foreign builder was already running, so this review started one Rust container. PostgreSQL started only after that smoke container had exited. The later Rust containers ran one at a time beside the one PostgreSQL container.

Evidence is local and untracked. It was not committed, because a commit on the shared `master` would move the ledger HEAD and the tree contains other untracked product files.
