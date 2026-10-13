# DL01-W1-42 fixed review of the Oracle thin-driver preflight

Verdict: `approve_preflight`.

Reviewed commit `645d37badf7a56dc26c90dc8cd3b007376489180`. Session `db8e3f53-43d0-45a4-9866-48edda74f909`. The original gate is W34 (`execution/batches/dl01-wave1-20261009/DL01-W1-34-oracle-driver-preflight.md`). This review compiled that commit and ran it against a new Oracle Free container. Author code, root, ledgers, and frontend were not modified.

The approved scope is the W34 native thin-driver preflight: connect, least privilege, `AS OF SCN`, row values, Arrow IPC for the exact supported subset, rejected lossy Arrow mappings, and a driver deadline that ends the server statement with a non-ready result. It does not accept an Oracle source product, Builder, Pack, the W04 key/value bridge, a schema change, DBP1, or a matrix row. Counts 10 and 8 stay unchanged. AT-SRC-03, AT-SRC-04, AT-SRC-07, and AT-SRC-10 stay outside this preflight.

## Provenance

`git archive 645d37badf7a56dc26c90dc8cd3b007376489180 testing/dl01-driver-preflight/oracle` matched the author `source-inventory.txt` on all 33 files (`raw/provenance/source-inventory-compare.txt`). The worktree of that path matched the same commit. Linux `aarch64-unknown-linux-gnu` `rustc 1.94.0 (4a4ef493e 2026-03-02)` ran `cargo build --jobs 1` and then `cargo build --locked --jobs 1` inside `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, with `--pull never`, 6 GiB, and 1 CPU. The unlocked build finished in 1m 20s and compiled `oracledb v26.0.0-beta.4` and `arrow-ipc v59.3.0`. The locked build finished in 0.03s on that same target.

The downloaded crate file sha256 is `d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397`, the same digest as `Cargo.lock`. `LICENSE.txt` is `a90ccbcbaac3be7c243648cff1ea7be053eed2033b9443ade51fe926380db235`. `NOTICE.txt` is `9b1bab09f15ad426f8615de9fac824081dbaa795c7aa99356cdb807cafaa23f2`. The crate manifest says `license = "UPL-1.0 OR Apache-2.0"` and `rust-version = "1.89"`. Its own `arrow-array` and `arrow-schema` requirements are `59.1.0` (Cargo caret range). The workspace lock pins `=59.3.0`, and that is the version the compiler built. Mode is thin. Instant Client was not installed.

Oracle image `docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8`, new volume `oo-dl01w1-42-oradata`, 6 GiB, 1 CPU, `--pull never`. SQL*Plus ping: `pdb|FREEPDB1|READ WRITE`. Driver `conn.version()` is `23.26.3.0.0`. Banner:

`Oracle AI Database 26ai Free Release 23.26.3.0.0 - Develop, Learn, and Run for Free`

`certifies_19c`, `certifies_marketing_alias`, and `tls` are false. `tls-ca-ref` is the literal `absent`.

## Fresh fixture

DBA setup created locked external schema `OO34_FIX` and reader `OO34_READER`. `user_sys_privs` is `CREATE SESSION` only. Object grants are `SELECT` and `FLASHBACK` on `T_BATCH`, `T_WIDE`, `T_TS`, and `T_BOOL`, plus `EXECUTE` on `SYS.DBMS_FLASHBACK`. `user_tab_privs` also shows `INHERIT PRIVILEGES|SYS|OO34_READER`. The setup and grant scripts do not issue that grant. No privilege contains ` ANY` and `LOGMINING` is absent. `v$database` returns `ORA-00942` on `SYS.V_$DATABASE`.

Business observer samples of `OO34_READER` peaked at one session, sid 50 serial 1770. Unified audit shows that session log off, then the labeled negative session, then one later reader logon for `T_SECRET`. Those later sessions are outside the business read.

## Values

Row API `typed-batch.json` matches `fixtures/gold.json`: `NUMBER(16,6)` `3141592653.589793` and `-0.000001`, `NUMBER(1)` digit strings `0` and `1` with metadata `DB_TYPE_NUMBER` precision 1 scale 0, DATE values that include time, CHAR values that keep spaces (`ZQ    `, six spaces, `AB C  `), empty VARCHAR2 as JSON null, RAW `0001ff00`, `00`, `dead00be`, `00000000`, and null.

Unconstrained NUMBER stayed decimal text in the row API: 38 eights, the expanded form of `1E-130`, null, and `0` for stored `1E-131`. `TO_NUMBER('1E-131')` is `0`. `CAST(10000000000 AS NUMBER(16,6))` is `ORA-01438`. `POWER(10,124)` returned the 125-digit integer. `POWER(10,126)` and `TO_NUMBER('1E126')` are `ORA-01426`.

`TIMESTAMP(9)` nanos `123456789` and `1` survived. `TIMESTAMP WITH TIME ZONE` came back as UTC fields plus a signed offset. `-05:30` is hour `-5` and minute `-30`. Reconstructed local time matches the inserted local timestamps.

`oracle-arrow-decode` does not depend on `oracledb`. Exit 0. `arrow-decode.json` is ready, 5 rows, schema `ID:Int16`, `N16:Decimal128(16,6)`, `N1:Int8`, `D:Timestamp(Microsecond,None)`, `C6:Utf8`, `V2:Utf8`, `R:Binary`. `batch.arrow` sha256 is `6a7e497c602865062152b7d085648e9a7803b41229e595fcb46d9702207beec4`. `N1` is Int8. Native SQL `BOOLEAN` is only in `oracle_sql_boolean_not_number_1.arrow.partial` (`B:Boolean`), with `number_1_is_boolean` false and `operational_path` false.

`arrow-rejected.json` records three uncommitted batches: unconstrained NUMBER as `Float64`, `TIMESTAMP(9)` as `Timestamp(Microsecond,None)`, and time zone as `Timestamp(Microsecond,None)`. `driver_error` is null. `committed` is false. `lossy` is true. No lossy file was promoted.

## SCN

DBA anchor try 1 SCN 2110333 and try 2 SCN 2110481 mapped one second before `T_BATCH` `LAST_DDL_TIME` and raised `ORA-01466`. Try 3 committed a new row change, SCN 2110487, mapped three seconds after the DDL, count 5. The probe chose `MAX(ora_rowscn)` 2110487 and its first `AS OF` try returned count 5. `GET_SYSTEM_CHANGE_NUMBER` was 2110786 and was not the chosen SCN. `AS OF SCN 2110487` kept id 41 at `2026-10-10T07:11:42` after the DBA committed the current value `2026-10-10T19:44:15`.

## Deadline

`set_call_timeout(3s)` on the nested-loop `ALL_OBJECTS` statement returned `CallTimeoutExceeded` at 3010 ms. `cancel-output.json` has `ready` false and `rows_committed` 0. Observer samples 6 through 16 show that SQL `ACTIVE` for sid 50 serial 1770. `dba-session-status.out` is `status|done`. `dba-cancel.out` was not created.

The crate's public connection surface has `set_call_timeout`, `call_timeout`, and `close`. It has no `cancel` or `break` method. W34 asks for driver out-of-band cancel or timeout that actually ends the active query and leaves the output non-ready, with DBA kill kept as observer cleanup. This run is that timeout. It is not an explicit user-cancel API.

## Audit and cleanup

Reader audit actions are `LOGON`, `ALTER SESSION`, `SELECT`, and `LOGOFF`. Both reader `ALTER SESSION` rows, and the negative user's row, are `ALTER SESSION SET TIME_ZONE='+00:00'` with return code 0. That is the thin driver's connect handshake. Reader `SELECT` return 0 covers the business and dictionary reads. Reader `SELECT` return 2004 is `v$database` and, on the later session, `T_SECRET`. Negative session: insert `ORA-41900` audit 2004, `CREATE TABLE` `ORA-01031` audit 1031, `AS OF` `ORA-41900` audit 2004. No successful reader `INSERT`, `UPDATE`, `DELETE`, or `CREATE TABLE`. Audit action names do not include AWR, ASH, LogMiner, XStream, or CDC.

Cleanup printed `cleanup|done` after dropping the audit policy, observer table, anchor log, and the three users. Owned containers, network, oradata, secrets, and the cargo and target volumes are gone. Images remain. Foreign containers were not removed.

## Exits

`raw/build/docker-build.exit` 0. `raw/probe/business.exit` 0. `raw/probe/decode.exit` 0. `raw/probe/negative.exit` 0. `raw/promote.exit` 0. `raw/run-result.json` has `promoted` true, `reader_peak` 1, `cancel_client_timeout` true, `cancel_server_seen_active` true, `cancel_server_still_active` false, `dba_kill_used` false, empty `platform_dml_success`, empty `audit_unknown`.

Admission under `/tmp/oo-dl01-wave1-resources.lock`: build at `2026-10-09T18:46:01Z` saw global Rust 3 and CPUs 12 to 13, headroom 21019181056 bytes. Oracle at `18:47:23Z` saw CPUs 12 to 13. The probe container at `18:47:45Z` saw CPUs 13 to 14, headroom 19711229952 bytes. Prefix `oo-dl01w1-42`.
