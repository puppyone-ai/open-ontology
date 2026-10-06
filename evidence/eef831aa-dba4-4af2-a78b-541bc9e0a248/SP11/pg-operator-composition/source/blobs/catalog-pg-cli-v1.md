# catalog-pg command

`oo-pipeline catalog-pg` reads one `oo.pipeline.catalog-pg/v1` document from stdin and writes one `oo.source.pgsql/v1` catalog to stdout. The command takes no arguments. It does not accept a target, open a target connection, install a schema, run a migration, import rows, grant, certify, or publish.

Stdout is the serde JSON of the `SourceCatalog` value returned by `PgSession::catalog`. The bytes are `serde_json::to_vec` of that value. There is no trailing newline, no command envelope, and no reconstructed catalog. `oo-builder captured-pg-draft` reads those bytes as its stdin document. `oo-builder review-pg-draft` reads the same value as the `catalog` object. This command does not call the builder and the pipeline crate does not depend on it.

Exit 0 is success. Exit 2 is usage: the command word is missing, it is not `catalog-pg` or `import-pg`, or another argument is present. Usage does not start the runtime and does not read stdin. Stderr is one line, `oo-pipeline: usage`. The argument text is not echoed. Exit 1 is invalid input, a source failure, or an output failure. Failure writes no stdout. Diagnostics are static. They name a field or a source fact. They do not include a password, a CA body, a path, a driver message, a server message, a comment body, or a URL that contains a password.

## Request

The document is at most 1_048_576 bytes. A longer stream is not read further. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. `serde` then rejects unknown fields and JSON null in the optional budget. `protocol` is `oo.pipeline.catalog-pg/v1`. Any other string is an invalid version.

The `source` object is the import command's source object. `source.source_id` matches `^[a-z][a-z0-9_]{0,62}$`. `source.host` is a DNS name of at most 253 characters: ASCII letters, digits, `.`, `_`, and `-`, without a scheme, a colon, a slash, or whitespace. `source.port` is 1 through 65535. `source.database` and `source.username` are 1 to 63 characters of ASCII letters, digits, and underscore. `source.secret_ref` is `secret://` plus 1 to 200 characters of ASCII letters, digits, and `.`, `_`, `/`, `-`. `source.password_file` and `source.ca_file` are paths of regular files. A symlink is not a regular file. The password file is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 bytes, contains no NUL, and is not written to the output. The CA file is at most 1 MiB and must be a certificate bundle. A private key is rejected. `source.tables` is a non-empty list of `{schema, table}` catalog names, each 1 to 63 bytes and without NUL. Duplicate pairs are rejected.

`source.budget`, when omitted, is connect 30000 ms, statement 60000 ms, lock 30000 ms, `max_rows` 10000, and `max_bytes` 8388608. Each timeout is 1 to 120000 ms. `max_rows` is 1 to 100000. `max_bytes` is 1 to 33554432. A present budget object requires every field. There is no SQL field, no target object, no shard, no owner, and no generation.

A rejected source value uses the same static source diagnostic as the import command: `oo-pipeline: source <kind> <fact>` when the fact is a short lowercase token, otherwise `oo-pipeline: source <kind>`.

## Session

`PgConnector::connect` opens one TLS session from that configuration. TLS is verify-full with the caller CA and an empty root store. The command does not disable TLS and does not use native or webpki roots. The connector's statements are the existing read-only catalog statements. This command does not submit caller SQL.

`PgSession::inspect_access` runs first. `registration_allowed` must be true. A false report is `oo-pipeline: registration refused`. The access report is not written. Block reasons, role names, and the secret reference are not written. The catalog is not read after a refusal.

When registration is allowed, `PgSession::catalog` reads the selected tables. The session is dropped before stdout is written. Dropping it closes the source socket. There is no `COMMIT`.

## Result

Stdout is one JSON object, the `SourceCatalog` serde document, with no trailing newline. Its `protocol` is `oo.source.pgsql/v1`. Comments, defaults, policy expressions, and domain checks are `{byte_len, sha256}`. A comment body is not a field. The document is refused, and nothing is written, when it is not that protocol, it does not parse as the same `SourceCatalog`, or it is longer than 1_048_576 bytes. That is the scanner ceiling `captured-pg-draft` and `review-pg-draft` apply to one document.

The builder's draft still leaves PII unknown, treats a captured foreign key as a tentative candidate, and records `row_import` `not_proven` and `publication` `unavailable`. Those are the builder's statements. This command does not make them.
