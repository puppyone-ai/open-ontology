# init-pg command

`init-pg` reads one `oo.pipeline.init-pg/v1` document from stdin and writes one `oo.pipeline.init-pg-result/v1` document to stdout. The command takes no arguments. An operator runs it on one empty target database before `oo-pipeline import-pg`. It is not started by the server, it does not install on a connection the server opens for a caller, and it is not a public authentication or deployment interface.

The command calls `oo_pipeline::target_installer::install`, then `verify_readiness`, then `install_full_import`, then `verify_full_import_readiness`. It does not copy user rows, write an apply receipt, create a publication, or grant `oo_target_owner` to `oo_pipeline_runtime`. The installer does not add that membership. This command does not add it either. `awaiting_publish` is not a result of this command. A later `import-pg` is a separate process.

Exit 0 is success. The other exits are the table below. Failure writes no result document. Stderr is one static line, `init-pg: ` plus the fact in the table. That line does not include a password, a CA body, a private key, a path, a host, a driver message, a server message, a stored checksum, or a SQLSTATE.

| Exit | Stderr fact | When |
|---|---|---|
| 2 | `usage` | Any argument is present. Stdin is not read. |
| 3 | `invalid input` | The document, a field, the endpoint, or `connect_ms` is rejected before a file is opened and before a connection is attempted. |
| 3 | `invalid version` | `protocol` is not `oo.pipeline.init-pg/v1`. |
| 3 | `target password file` | The password file is missing, not a regular file, larger than 1026 bytes, not UTF-8, empty after one trailing newline is removed, longer than 1024 bytes, or contains NUL. |
| 3 | `target ca file` | The CA file is missing, not a regular file, larger than 1 MiB, or rejected by `PgTlsConfig::from_ca_pem`. A private key is rejected. |
| 4 | `target connect` | The endpoint and both files were accepted, and the verify-full connection did not open. |
| 4 | `target principal` | `CURRENT_USER` or `SESSION_USER` is not `administrator`. |
| 5 | `target administrator` | The session is not a superuser and is not a member of `oo_target_owner`, or `install` / `install_full_import` returned `NotAdministrator`. The role check uses `pg_roles` and `pg_has_role` before `install`, because a login without `USAGE` on `_oo` cannot complete the installer's catalog probe. |
| 6 | `target catalog refused` | The installer refused the catalog, including a second empty database on a cluster that already has the product roles, a missing segment relation (`OO002`), or `Refused`. |
| 7 | `migration checksum` | A stored checksum differs (`OO001` or `ChecksumMismatch`). The stored digest is not printed. |
| 1 | `target database` | PostgreSQL failed in some other way. The driver text is dropped. |
| 1 | `target schema is not ready` | Readiness is not the compiled production set, or the full-import seal is not the compiled `0005` digest. |
| 1 | `init input` | Stdin could not be read. |
| 1 | `init output` | The install transaction already committed and stdout could not be written. A later run is the installer's replay. |

## Request

The document is at most 1_048_576 bytes. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. `serde` rejects unknown fields and JSON null in `connect_ms`. There is no password field, no SQL field, and no CA body. A field named `password` is `invalid input`. Its value is not written to stderr.

`protocol` is `oo.pipeline.init-pg/v1`.

`target.host` is checked with `PgEndpoint::new`: at most 253 characters, no scheme, colon, slash, or whitespace. `target.port` is 1 through 65535. `target.database` and `target.administrator` are 1 to 63 characters of ASCII letters, digits, and underscore. `administrator` is the login that calls the installer. On an empty cluster database that login has to be a superuser. A member of `oo_pipeline_runtime` is not that administrator. The placeholder passed to `PgCredentials` while checking the login name is not sent to PostgreSQL.

`target.password_file` and `target.ca_file` are paths of regular files. Symlinks are rejected. The password file is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 bytes and is not written to the output. The CA file is at most 1 MiB and must satisfy `PgTlsConfig::from_ca_pem`.

`connect_ms`, when omitted, is 30000. The accepted range is 1 to 120000. It bounds the connection attempt and the principal query. The installer then sets its own lock and statement timeouts inside its transaction. `connect_ms` is checked before either file is opened.

`PGPASSWORD`, `DATABASE_URL`, and `PGSSLROOTCERT` are not read. The connection is `PgConnectOptions::from_explicit`, `PgSslMode::VerifyFull`, and `ssl_root_cert_from_pem` of the caller CA. The sqlx features are the workspace pins, including `_tls-rustls-ring-no-roots`. WebPKI roots and native roots are not enabled. Statement logging is disabled.

## Result

Stdout is one canonical JSON object, keys sorted, with a trailing newline. The fields are copied from the installer values after the readiness checks agree with `compiled_digests` and `compiled_full_import_digest`:

| Field | Source |
|---|---|
| `protocol` | `oo.pipeline.init-pg-result/v1` |
| `install` | `InstallOutcome` display: `bootstrapped`, `upgraded_from_v1`, `upgraded_from_v2`, `installed_mapping`, or `replay` |
| `historical_v2` | `VerifiedReadiness::retains_historical_v2` |
| `seals.bootstrap` | compiled digest of `0000_bootstrap_roles.sql` |
| `seals.fence` | compiled digest of `0001_target_fence_expand.sql` |
| `seals.preflight` | compiled digest of `0003_target_segment_preflight_expand.sql` |
| `seals.mapping` | compiled digest of `0004_physical_mapping.sql` |
| `full_import` | `FullImportInstallOutcome` display: `installed` or `replay` |
| `full_import_digest` | compiled digest of `0005_full_import_expand.sql` |

A fresh empty database on a cluster without `oo_target_owner` and `oo_pipeline_runtime` returns `install` `bootstrapped` and `full_import` `installed`. The same document run again returns `replay` and `replay`. `applied_at` stays. A second empty database on that cluster is not installed. `0000_bootstrap_roles.sql` is not edited to skip `CREATE ROLE`. The refusal is exit 6, and the first database is unchanged.

A wrong CA fails at exit 4 and does not change migration rows. A login that is not the administrator fails at exit 5 and does not change migration rows. A partial catalog that is not a fresh install or a forward migration fails at exit 6 or exit 7. Existing rows and stored migration checksums stay. This command does not publish.
