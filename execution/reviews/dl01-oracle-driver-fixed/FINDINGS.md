# Findings

The fresh run met the W34 preflight exits. The failures below stay failures. A later success does not erase them.

## Review harness attempt 1

`raw/attempt-1` used `bash -lc` to start the author `build-inside.sh`. A login shell dropped the image `PATH`, so `rustc` was not found. `docker-build.exit` is 127. No Oracle case ran. The relaunch calls `bash -c` and keeps the image `PATH`. That attempt is retained.

`raw/oracle/docker-logs.txt` and `docker-inspect.json` from the successful run are empty. Container id `cf120c9f55117ae49e59009d6f556170aac47a8c795678e356c8388dbf56906f` is in `docker-run.stdout`. Database identity is the SQL*Plus ping and the driver banner, not those two files.

## Author attempts that are not this pass

Author attempt 5 called `DBMS_SESSION.SLEEP(20)` with a 3 second call timeout. The client returned at 6043 ms with `UnableToRecover` and a closed connection. The server session stayed `ACTIVE`. The harness then issued DBA `ALTER SYSTEM CANCEL SQL`. That file is `evidence/4264ed49-a669-48b2-a819-dd18a00b1f27/DL01/oracle-driver-preflight/raw/attempt-5/probe/cancel-output.json`. DBA cancel is cleanup, not driver cancellation.

Author attempt 6 replaced the statement with `CONNECT BY LEVEL <= 200000000`. The server returned `ORA-30009` in 294 ms, before the timeout. That result is not a deadline pass.

Author attempts 1 and 2 failed `AS OF` with `ORA-01466` because the chosen SCN mapped at or before DDL. Retrying one unchanged SCN does not move `SCN_TO_TIMESTAMP` past `LAST_DDL_TIME`.

## This run's anchor failures

The DBA anchor still recorded two failed SCNs before the one it handed to the probe. SCN 2110333 and SCN 2110481 mapped at `2026-10-09 18:47:37` while `T_BATCH` `LAST_DDL_TIME` was `2026-10-09 18:47:38`, code `-1466`. Try 3 committed a real `n1` change and produced SCN 2110487, mapped at `18:47:41`, delta 3 seconds, count 5. The probe's `scn-attempts.json` has one entry, try 1, count 5, on 2110487. The pass uses that post-DDL SCN. It does not claim that repeating a stale SCN succeeded.

`GET_SYSTEM_CHANGE_NUMBER` returned 2110786. The probe recorded it and did not read `AS OF` that value.

## Deadline versus a user-cancel method

The promoted statement is:

`SELECT /* oo-dl01w1-34-cancel */ /*+ LEADING(a) USE_NL(b) NO_PARALLEL */ SUM(MOD(a.object_id + b.object_id, 9)) FROM all_objects a, all_objects b WHERE ROWNUM <= 8000000`

`Connection::set_call_timeout(Some(3s))` returned `CallTimeoutExceeded` at 3010 ms. Samples 6 through 16 show the statement `ACTIVE`. The later status spool is `status|done`. No `dba-cancel.out` exists. Output `ready` is false and `rows_committed` is 0.

Inspection of `oracledb` 26.0.0-beta.4 `src/connection/mod.rs` shows `set_call_timeout`, `call_timeout`, and `close`. It does not show a public `cancel` or `break` method. W34's output line accepts a driver timeout that ends the active query and keeps the partial result non-ready, and it keeps DBA kill as observer cleanup. That exit is met. A separate explicit user-cancel API is not implemented by this SDK and is not claimed here. A later product card that needs a caller-issued cancel, distinct from a deadline, has to specify that method on its own.

## Lossy Arrow stays uncommitted

The row API kept unconstrained NUMBER digits and `TIMESTAMP(9)` nanoseconds with a signed zone. Arrow did not. The three lossy schemas were `N_FREE:Float64`, `TS:Timestamp(Microsecond,None)`, and `TSTZ:Timestamp(Microsecond,None)`. The driver returned those batches without an error string. The probe set `committed` false and did not leave a promoted file for them. `NUMBER(1)` stayed `Int8` / digit text. Native `BOOLEAN` is a labeled side file.

## Privileges and handshake

`INHERIT PRIVILEGES` from `SYS` to `OO34_READER` is visible in `user_tab_privs`. `dba-setup.sql` and `dba-grants.sql` do not grant it. It is recorded as the Oracle default, not as a platform grant.

`ALTER SESSION SET TIME_ZONE='+00:00'` return code 0 appears on every thin connect, including the negative user. Classify allows `ALTER SESSION`. It is the driver handshake, recorded in `dba-audit.out`, and it is not a business write.

Object-privilege denial on this banner is `ORA-41900` (audit 2004). `CREATE TABLE` denial is `ORA-01031` (audit 1031). `T_SECRET` is `ORA-00942` (audit 2004) because the reader was not granted the table. The negative statements are marked `operational_path` false.

The 0.25 second observer did not sample `OO34_NOFLASH`. Unified audit still has that session's logon, handshake, three denials, and logoff, after the business reader logoff. Business samples never showed two `OO34_READER` rows in one sample.

## What this approval leaves closed

This result certifies the tested pair only: thin `oracledb` 26.0.0-beta.4 on banner 23.26.3.0.0. It does not certify Oracle 19c, a marketing alias of "26", TLS, W04's key/value Arrow protocol, or a source path through PG, Builder, or Pack. `control_management_pack_access` was not queried. Supplemental logging, AWR, ASH, LogMiner, XStream, and CDC were not opened.
