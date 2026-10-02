# a074 PostgreSQL security correction review

Decision: **approve** the correction slice `4ecdbf714766baef3490a4f954701eaf19f245a0` only. This does not approve SP03-W01, PostgreSQL 13 or 14, bounded read, type conversion, CDC, or source integration. Integration stays with the coordinator after a073 releases the SDK root lock. The integration must keep the feature check below. The workspace sentence that these path crates "are not a driver fork" must not be copied into an SBOM as unmodified crates.io identity.

## Identity

| Item | Value |
| --- | --- |
| Reviewed commit | `4ecdbf714766baef3490a4f954701eaf19f245a0` |
| Tree | `da8b5d2cb5f4b2548bd1066c2b707bbaedf0d07d` |
| Parent | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| Stamp (name only) | `181df9b6ba030b68e3a48aa0f58e5c5f909bee55` |
| Stamp tree | `41aecc32fcacb47f141fef8dda4e3810a92bc3fd` |
| Stamp diff | `evidence/.../a066/SP03/pg-source-fix/REPORT.md` and `handoff.json` |

The worktree `/Users/redux/.codex/worktrees/product-pg/open-ontology` stayed at the stamp with an empty porcelain. The review used a git archive of `4ecdbf71`. No source file, index, or worktree was edited. The probe was a workspace member of a copy of that archive.

Parent a061 decision on `b2d3cbc` was `request_changes`. The three HIGH cases from that review are closed on this commit by the live oracle below.

## What ran

Linux arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. PostgreSQL fixture image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. No host port, no host network, no privileged mode, no user HOME mount. Ambient `PG*` values were applied only with `Command.env` inside child processes.

Focused smoke, 2026-10-02T00:19:43Z to 00:20:12Z, all exit 0: `cargo fetch --locked`, `cargo test -p oo-connectors --locked --offline --lib` (14 passed), doc tests (0), `live_pg --no-run`. `cargo tree -i webpki-roots` exit 101. The selected `sqlx-core` features are `_rt-tokio`, `_tls-rustls`, `_tls-rustls-ring-no-roots`, `rustls`, `migrate`, `crc`, `sha2`, `tokio`, `tokio-stream`, and `default`. `rustls-webpki` 0.103.15 and `ring` 0.17.14 remain. `webpki-roots` is absent.

The live probe then opened the fixture as each role. Raw exits and JSON are in `raw/`.

## Privilege oracle

The oracle ran as the fixture superuser (`PGAPPNAME=oo-pg-oracle`), then rolled back. The connector never issued that SQL.

Legal reader `oo_src_reader`: `has_column_privilege` SELECT is true for `app.customer.id` and false for `hidden_pii` and for INSERT. Table-level `has_table_privilege(..., 'SELECT')` is false because the grant is columnar. The catalog snapshot agrees: `id`, `display_name`, and `label` have `select: true` and `insert: false`; `hidden_pii` has `select: false`. Registration is allowed, `write_state` is `read_only`, and the seven-table schema hash computed on this database is `b0dbcffe78f8818af5c72f37c2db048c7058ccfaf8eb1eef4b17b83ac37d9692`. That figure was not taken from the old `hashme` probe. A second table, `app.hashprobe`, hashed to `56ac4e04e096d57cede37a53e004c01430fe4b760478307ff499a4b8c406cbf4` on two snapshots. After a comment change and `GRANT INSERT` the hash stayed the same. After `ALTER TABLE app.hashprobe ADD COLUMN extra text` it became `0237f5f27dac17b1d21724ee296b24041d95a69d67696f52f20b1254b7aef074`.

| Case | Oracle | Connector |
| --- | --- | --- |
| `oo_src_side` INSERT on `app.side_write` | true | blocked, finding `INSERT` `direct` |
| `oo_src_hiddenfn` EXECUTE `hidden.boom()` | true, schema USAGE true | blocked, definer `boom` |
| `oo_src_schemacreate` CREATE on schema `side` | true | blocked, `CREATE` `schema_create` |
| `oo_src_dbcreate` CREATE on database `source_lab` | true | blocked, `CREATE` `database` |
| inherit, set-only, inherit-only, transitive | INSERT true by inherit or set | blocked, `inherited` or `set_role` |
| column INSERT on `display_name` | true; table INSERT false | blocked, `COLUMN_WRITE` `column` |
| superuser, BYPASSRLS, CREATEROLE, CREATEDB | attribute true | blocked, `privileged_role` |
| `oo_src_definer` | EXECUTE on both definers | blocked, `dangerous_insert` and `public_definer` |
| operator role | EXECUTE `hidden.boom_op`, USAGE on `ops`, no USAGE on `hidden` | blocked, definer `boom_op` |
| cast role | EXECUTE `hidden.boom_cast`, USAGE on `app`, no USAGE on `hidden` | blocked, definer `boom_cast` |
| table owner of unselected `app.owned_outside` | owner | blocked, `owner` on that table |
| 17-edge SET chain | no direct INSERT | blocked, `essential_fact_unknown` / `membership_depth` |
| `max_rows=1` on the multi-table database | n/a | `Budget` `max_rows`, no report, not registered |
| `oo_src_noset` (INHERIT false, SET false) | no INSERT, no CREATE | allowed, `read_only` |

`app.collide` (U+00E9 versus U+0065 U+0301) returns `Identifier` `identifier_collision`. Empty selection and empty database both hash to `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`, which matches an independent SHA-256 of `{"tables":[]}`. Stable idents for `客户` and `订单` are `q_e5aea2e688b7` and `q_e8aea2e58d95`.

Comment canary SHA-256 is `99a042fad752401594c711cdc674f22470412394789d6e485622b0de7f264d50` (26 bytes). The canary text is absent from catalog JSON and Debug. The `sku` default is stored as length 14, SHA-256 `d618f2a8f65ee151a7bd97b72defd03e5b1b61e2970a94eb42b43cbd254b9d5f`, which is SHA-256 of `'static'::text`. The word `static` does not appear in the catalog JSON. `type_conversion` is `unsupported` and every sampled `converted` flag is false.

## ADMIN membership and the view

`oo_src_adminmem` holds `oo_src_writer` with INHERIT false, SET false, ADMIN true. Inside a transaction that was rolled back:

- `SET LOCAL ROLE oo_src_writer` failed, SQLSTATE 42501.
- `INSERT` into `app.side_write` failed, SQLSTATE 42501.
- `GRANT oo_src_writer TO oo_src_adminmem WITH INHERIT TRUE, SET TRUE` succeeded.

After `ROLLBACK`, the membership was again INHERIT false, SET false, ADMIN true. The connector refused registration (`write_capable`) and reported the writer's INSERT, UPDATE, DELETE, TRUNCATE, and schema CREATE through the `set_role` path. The reproduced ability is GRANT of that role. SET ROLE and direct INSERT were denied. The connector blocks registration, so this is not an admitted hole. The path label says `set_role` even though SET ROLE itself failed.

`oo_src_viewtrap` can SELECT `app.trap`. The view calls `app.invoker_insert()`, a SECURITY INVOKER function. `SELECT` as that role failed, SQLSTATE 42501, `permission denied for function invoker_insert`. `app.side_write` stayed at 0 through rollback. The connector allowed registration. No write was reproduced.

## Startup environment and TLS

`PgConnectOptions::from_explicit` in a child with `PGOPTIONS=-c role=oo_src_reader`, `PGSSLMODE=disable`, `PGSSLCERT`, `PGSSLKEY`, `PGSSLROOTCERT`, `PGUSER`, `PGDATABASE`, `PGAPPNAME`, and `HOME=/pghome` still built host `example.invalid`, username `smoke-user`, database `smoke`, application name `oo-pg-source`, `options` null, and `VerifyFull`. The child saw those variables. The parent process `PGOPTIONS` stayed unset.

Live children of the write-capable login `oo_src_dual`:

- `PGOPTIONS=-c role=oo_src_reader` plus a `HOME` pgpass file containing `wrong-pgpass-secret`: session user and current user stayed `oo_src_dual`, version stayed 17.10, registration stayed blocked.
- `PGOPTIONS=-c server_version=13.14`: version stayed `17.10`, registration stayed blocked.
- Reader with `PGSSLMODE=disable`, forged user, password, database, and app name, and a pgpass home: session user `oo_src_reader`, database `source_lab`, TLS `TLSv1.3`, registration allowed.

Missing CA and a PEM that contains `PRIVATE KEY` fail in `PgTlsConfig::from_ca_pem` with `ca_pem`, before a socket. A bogus certificate PEM, the wrong CA, and host `oo-source-wrong` (certificate SAN is only `oo-source-pg`) each fail with `Tls` `tls_handshake`. The configured private CA connects.

An independent poll of `pg_stat_ssl` joined to `pg_stat_activity` during a reader connect recorded `true|TLSv1.3|oo_src_reader|source_lab|oo-pg-source`. The connector report has the same user, database, and `negotiated_version`.

Source inspection: `VerifyFull` sets `accept_invalid_certs` and `accept_invalid_hostnames` false and uses `ClientConfig::with_root_certificates`. A Mozilla-signed adversarial certificate was not presented. The dynamic evidence is wrong-CA rejection, wrong-hostname rejection, and private-CA success. The static evidence is the feature graph and the empty-store test below.

## Root store

`cargo test -p sqlx-core` cannot run the crate's `cfg(test)` because `sqlx-core` is a path patch and not a workspace member (exit 101). A temporary copy of the patched crate ran:

- Features `_tls-rustls-ring-no-roots` only: `a074_root_count=0`, `a074_webpki=false`, `a074_native=false`, and the author's `root_store_starts_empty` passed. Exit 0.
- Features `_tls-rustls-ring-no-roots` and `_tls-rustls-ring-webpki` together: `a074_root_count=119`, `a074_webpki=true`, `a074_native=false`. Exit 0. That standalone resolve downloaded `webpki-roots` 1.0.7 from the crate range `version = "1"`. It is not the workspace lock. Private-CA-only does not survive that combination.

sqlx 0.9.0 default features are `any`, `macros`, `migrate`, and `json`. They do not enable TLS. `tls-rustls` is an alias of `tls-rustls-ring-webpki`, which enables `webpki-roots`. Cargo unifies features, so any later workspace dependency that turns those on fills the store.

Required integration check, after a073's lock merge: `cargo tree -e features -i sqlx-core` must not show `webpki-roots` or `rustls-native-certs`. CI must fail if any workspace crate enables `tls-rustls`, `tls-rustls-ring`, `tls-rustls-ring-webpki`, `tls-rustls-ring-native-roots`, `tls-native-tls`, or the sqlx-core features `webpki-roots` or `rustls-native-certs`.

## Vendor patch and licenses

Official crates.io 0.9.0, yanked false, license `MIT OR Apache-2.0`, published 2026-05-21. Downloaded tarball checksums match the API and, for `sqlx`, the lock: sqlx-core `05b44e85bf579a8eeb4ceaa77a3a523baf2bf0e9bac7e40f405d537b5d2d5ccb`, sqlx-postgres `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e`, sqlx `378620ccc25c62c89d8be1c819e76a88d59bdcc3304733330788948e619bfd71`. Age is well past 168 hours on 2026-10-02.

Recursive diff against those tarballs reports only:

- `sqlx-core`: `Cargo.toml`, `Cargo.toml.orig`, `src/net/tls/tls_rustls.rs`
- `sqlx-postgres`: `src/options/mod.rs`

No extra or missing files. The unified patch is 104 lines, 3664 bytes, SHA-256 `f4b1aff1f2575a6b161a5cec37da26a4e097475aa1dcde3a9cec603766ac5f37`. It adds `_tls-rustls-ring-no-roots`, adjusts the ring and aws-lc cfgs, comments the existing empty `import_root_certs`, adds `root_store_starts_empty`, and adds `PgConnectOptions::from_explicit`. `new` and `new_without_pgpass` still read libpq environment. The product connect path calls `from_explicit` and rejects a leftover options string.

`PATCH.md` describes those hunks and says the directories are the crates.io sources plus the hunks. That part is accurate. The workspace `Cargo.toml` says the path patches "are not a driver fork." That sentence is the wrong identity for an SBOM. These crates are modified upstream artifacts at version 0.9.0.

Both tarballs and the vendor tree ship `LICENSE-MIT` and `LICENSE-APACHE` as dangling one-line pointers (`../LICENSE-MIT`, `../LICENSE-APACHE`). The patch did not remove a license body the crate contained. The full texts live at VCS `003b698e99e024f3621b8043a2426fde5b741171`: MIT 1109 bytes SHA-256 `5abbdd842310c8f1650fcde3a5a51a3e09412b9e5a6eea87519e0db9e32b339d` (Copyright SQLx Contributors and LaunchBadge), Apache-2.0 10297 bytes SHA-256 `c8f5453612253e8ea8bad241617fe74c4a6d5d592ba64ff881668c211b50ac99`. The vendored tree does not contain those bodies. `webpki-roots` and its CDLA-Permissive-2.0 data are gone from this lock. `ring` remains `Apache-2.0 AND ISC`, and `rustls-webpki` and `untrusted` remain ISC. Those ISC obligations are still in the binary.

## Statements and supply

`log_statement=all` captured 957 statements with `application_name=oo-pg-source` across connect, inspect, catalog, and reconnect. After string literals were removed, the first keywords were SELECT 815, WITH 58, START 42, ROLLBACK 42. Flagged write or DDL statements: 0. Empty `app=` lines are server startup. Three `[unknown]` lines are rejected SSL handshakes, not SQL. Fixture DDL used `oo-pg-fixture`.

Correction `Cargo.lock` SHA-256 `ee9c4ad02b69686b9132dba3cc8bcedb809d42787ce770441fdaa045e44fc8a7`. It has no `webpki-roots` package. The author's lock delta, rechecked here, is 248 packages to 247, zero new registry crates, `webpki-roots` 1.0.9 removed, and the two sqlx crates moved from registry checksums to path. The build copy's lock gained only the probe package `oo-a074-probe`; that copy was not the deny input.

`cargo-deny` 0.20.2 (musl tarball SHA-256 `995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d`) on the unmodified archive and `ci/deny.toml`: exit 4. Advisories ok, bans ok, sources ok, licenses FAILED for `ring` 0.17.14 `Apache-2.0 AND ISC`, `rustls-webpki` 0.103.15 `ISC`, and `untrusted` 0.9.0 `ISC`. The same command with main candidate `b14c7821031990aafd713f23068658d41042ee2d`, whose only license change is allowing ISC: exit 0, `advisories ok, bans ok, licenses ok, sources ok`. ISC allow is not the TLS fix. `ci/deny.toml` in this slice is unchanged.

`cargo-audit` 0.22.2, database fetched from the RustSec clone whose local HEAD is `6de4455103aced2cba86e3b86e5c090b22827cf1`, loaded 1279 advisories and scanned 247 dependencies. Exit 0. No vulnerability records were printed. A first attempt exited 1 because the database mount was read-only (`packed-refs.lock`). That stderr is in `raw/audit-ro-attempt.txt`. A later copy of the successful scan had replaced that file; the original text was put back from the captured read of `out/audit.txt` before cleanup. Deny was configured with `yanked = "deny"` and reported advisories ok.

## Out of scope

PostgreSQL 13 and 14 were not started. Bounded read, type conversion, CDC, and registry APIs are reported unsupported. One server version was tested: 17.10. This review does not sign SP03-W01.

## Coordinator

Source-only integration of `4ecdbf71` is permitted by this security review after a073 releases the root lock, with the feature check above enforced on the merged graph. Do not describe the path crates as byte-identical crates.io 0.9.0. Ship the MIT and Apache-2.0 bodies with the patched crates. Land the ISC allow from `b14c782` as its own supply change.
