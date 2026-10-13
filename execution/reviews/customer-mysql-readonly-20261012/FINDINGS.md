# DL01-W1-78 findings

## F1. Official probe exited 1 before a greeting

`oo-cmysql-ro-78` attempt 2, `started_unix_ms` 1791778611269, probe elapsed 6021 ms, process exit 1. `target_a` connect 3007 ms and `target_b` connect 3004 ms. Both `connect_diag` values are `{variant: io, kind: UnexpectedEof, os_errno: null}`. `failure_class` is `network`. `auth_ok` is false. `rows_read_ok` is false. `server_errno` is null. `both_data_planes_ok` is false.

## F2. DNS class is an intermediate resolver range

Both targets resolved one address in class `intermediate_198_18_15` (6 ms and 2 ms). `rds_ip_signoff` is false. The resolver answer is not the RDS server IP and is not a successful authentication.

## F3. A no-credential TCP check saw the same close

First check, 2 s recv cap: both aliases start-to-result 2003 ms, intermediate true, `recv_timeout`. Second check: both aliases `connect_ms` 2, eof at 3004 ms, 0 bytes. No MySQL greeting byte arrived. The named driver failure and this check describe one host route.

## F4. Identity and the read plane are unobserved

`VERSION`, `DATABASE()`, `CURRENT_USER()`, charset, time zone, `SHOW GRANTS`, one base-table metadata set, and `LIMIT 5` did not execute. Counts of rows, columns, cells, and type buckets are absent. Row hashes are absent. `performance_schema` history for this connection was not read. A complete source audit is not claimed.

## F5. Account admission is unobserved

Privilege verbs were not returned. The account is not recorded as read-only. `platform_admitted_reader` is false. The observer issued no source DML and no source DDL. A wider test account, if that is what the private grants later show, still does not become the platform admitted reader from this precheck.

## F6. TLS was not the observed failure

`tls_session` is `not_requested`. The target file contains no instance CA path. `danger_accept_invalid_certs` was not enabled. After a greeting exists, `VERIFY_CA` still needs the instance PEM. The product helper `tools/mysql-preview/src/customer.rs` `connect_parts` already requires that CA file and refuses disabled verification.

## F7. The data source is not marked unavailable

`source_unavailable_claimed` is false on the probe and on both aliases. The minimal missing condition is a host route that delivers a MySQL server greeting to this process. The user-confirmed VPN and the intermediate DNS answer did not produce that greeting on the measured path. No local fixture stands in for the customer libraries.

## F8. Attempt 1's protocol class is withdrawn

Attempt 1 exited 1 at 6014 ms and labeled both targets `protocol` by matching Display text. That label missed `UnexpectedEof`. Attempt 2 is the official class.

## F9. Scope boundary

Product source, root `Cargo.lock`, dispatch, and other source ledgers were not edited by this precheck. Source acceptance stays with the existing 0/10 full sign-off. G1–G13 stay unpromoted. The source denominator is unchanged. Model and SQL Server stay pending. Engineer DL01-W1-06 keeps the original MySQL card, including updates, self-build, YAML ontology, Knowledge Pack, Gateway, and Agent.
