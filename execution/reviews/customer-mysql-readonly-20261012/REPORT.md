# DL01-W1-78 customer MySQL read-only precheck

Verdict: `route_blocked_before_greeting`.

Named probe `oo-cmysql-ro-78` exited 1 at 2026-10-12 12:16:51 CST. Both configured targets completed a TCP connect through an intermediate resolver address and closed the socket before any MySQL server greeting. Authentication, identity, grants, table metadata, and row reads did not run.

`source_unavailable_claimed` is false. `platform_admitted_reader` is false. `product_source_modified` is false. This precheck leaves the MySQL source card with DL01-W1-06. It does not sign off that source, does not promote G1–G13, and does not change the source denominator. Model credentials and SQL Server native amd64 stay pending.

## Named run

| field | value |
| --- | --- |
| probe | `oo-cmysql-ro-78` official attempt 2 |
| exit | 1 |
| client | host `mysql_async` 0.36.1, features `minimal`, `rustls-tls`, `ring` |
| runtime | rustc 1.98.1 (48a229cea 2026-09-01) (Homebrew) |
| product rust-version | `tools/mysql-preview` 1.94.0; this binary is not the product binary |
| binary sha256 | `4cf37f0385230593cca1081a2922950c803d0ab99b7e3f4a034ffcaf4103f8f6` |
| binary | Mach-O 64-bit executable arm64, 18388400 bytes |
| connect cap | 10000 ms, one connection per target, targets sequential |
| statement cap | 5000 ms |
| TLS | `not_requested`; target file has no instance CA; verification was not disabled |
| started_unix_ms | 1791778611269 |
| probe elapsed_ms | 6021 |
| `/usr/bin/time -l` | real 7.27 s, user 0.00, sys 0.00, max RSS 14024704, peak footprint 3310216, page reclaims 1122, page faults 0, block IO 0, messages sent 25, messages received 25, voluntary switches 11, involuntary switches 88, instructions 60604047, cycles 38378768 |

Attempt 1 is archived under the private run directory. It exited 1 with elapsed 6014 ms and a withdrawn `protocol` class. The official class is attempt 2.

## Targets

Hostnames, database names, account names, and the password stay in the private target file. Git records aliases only.

| alias | port | provider | dns class | dns ms | addresses | rds IP sign-off | connect ms | diag | class | auth | rows |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| target_a | 3306 | aliyun_rds | intermediate_198_18_15 | 6 | 1 | false | 3007 | io UnexpectedEof, os_errno null | network | false | false |
| target_b | 3306 | aliyun_rds | intermediate_198_18_15 | 2 | 1 | false | 3004 | io UnexpectedEof, os_errno null | network | false | false |

Both databases were configured. `server_errno` is null on both. The driver error is `Error::Io` during `Conn::new`, inside the 10 s cap.

Addresses in 198.18.0.0/15 are `intermediate_198_18_15`. They are not an RDS server IP sign-off. A DNS answer is not authentication and is not a data read.

## Route check with no credentials

Two host TCP checks sent no MySQL packet and no password. They are route evidence beside the named probe.

The first check used a 5 s connect cap and a 2 s recv cap. Both aliases returned `tcp_ok`, start-to-result 2003 ms, intermediate true, `recv_timeout`.

The second check used a 10 s connect cap and an 8 s recv cap. Both aliases returned `connect_ms` 2, `total_ms` 3004, intermediate true, `recv=eof`, `nbytes=0`.

Zero greeting bytes matches the driver `UnexpectedEof` at 3004 ms and 3007 ms. The intermediate peer can accept TCP and then close. That measurement does not establish that either RDS instance is down.

## Statements that did not run

The session never reached `SET SESSION max_execution_time = 5000`, `SET SESSION transaction_read_only = 1`, `START TRANSACTION READ ONLY`, `SELECT VERSION(), DATABASE(), CURRENT_USER()` plus charset and time zone, `SHOW GRANTS FOR CURRENT_USER()`, `information_schema` selection, `SELECT ... LIMIT 5`, or this connection's `performance_schema` history.

Server version, charset, time zone, grants, table alias, type histogram, row count, cell count, and row hashes are unobserved. An empty-table result would require a completed `SELECT`. This run has no such result. Source audit is unobservable.

No source DML or DDL. No negative write probe. No data creation. No audit or binlog change. No RDS restart. No schema install. No base-table `COUNT(*)`.

## Clients that were not used

Host `mysql`, `mariadb`, `pymysql`, and `MySQLdb` are absent. The approved image pin `mysql-8.4.10` `linux/arm64` `sha256:3f616910ccd923d5089d7889d491cefa65e24fcfa79d74db4084729f48c9cab1` in `testing/infra/images.pins` was not started. Docker Desktop 29.4.1 on this Darwin host does not carry the host VPN, so a container dial is not the customer path. No local fixture was substituted.

## Private paths

Target config: `/tmp/oo-customer-mysql-20261012/targets.json`, directory mode 0700, secret file mode 0600. Probe tree: `/tmp/oo-customer-mysql-readonly-20261012`, mode 0700. No grants file and no row file were written.

Sanitized probe stdout is `summary.json` in this packet.
