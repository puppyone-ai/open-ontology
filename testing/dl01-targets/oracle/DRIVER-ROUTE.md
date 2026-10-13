# Oracle driver route (preparation, not an implementation)

## Selected route

Oracle maintains `oracledb` (repository `oracle/rust-oracledb`). The crates.io release observed for this task is `26.0.0-beta.4`, license `UPL-1.0 OR Apache-2.0`, `rust-version` 1.89. The project describes it as a pure Rust thin driver: no Oracle Client libraries, SQL and PL/SQL, and an optional Arrow feature. That matches the P2 worker rule that native or JVM libraries stay out of Gateway and Loader, and it matches the Arrow handoff in the database-support design.

This task does not compile the crate, does not vendor it, and does not add it to a manifest. `26.0.0-beta.4` is a beta. DBP1's driver-maturity question stays open until a Rust consumer runs it against this pinned database. The README's supported database list names 12, 18, 19, 21, and 26. It does not name 23 as a separate line. The free image tag is `23.26.3`. The live banner was `Oracle AI Database 26ai Free Release 23.26.3.0.0 - Develop, Learn, and Run for Free`, with `ORACLE_HOME=/opt/oracle/product/26ai/dbhomeFree` and SQL*Plus `23.26.3.0.0`. A later consumer must treat a banner the driver does not claim as unverified, not as inherited support.

## Rejected routes

| Route | Why it is not the preparation default |
|---|---|
| `oracle` crate (ODPI-C) | Needs Oracle Client at runtime. Instant Client is under its own license (https://www.oracle.com/downloads/licenses/instant-client-lic.html), not the Database Free terms. The project default is not to redistribute it. |
| JDBC / SQLcl | Official, but a JVM or a separate CLI is not the Rust worker. SQLcl's license is not the database license. |
| sqlplus inside the image | Official client for this probe only. It is not a driver the product can link. |
| Third-party pure Rust TNS crates | The design text says they are early and are not the default. Oracle now ships its own thin driver. |

## Bridge

Commit `577160c22de63adb9b38e0147f3bc2da893c4d9d` has no `tools/source-driver-worker`. `oo_connectors::model::AccessReport` in that commit is `oo.source.pgsql/v1` and carries PostgreSQL facts (`in_recovery`, publications, replication slots). It is not an Oracle contract. DL01-W1-04 owns the worker interface and its SHA was not delivered to this task. This preparation does not invent a substitute protocol and does not mark the bridge present.

When that immutable SHA exists, the Oracle consumer should use it unchanged. The bridge has to be able to carry at least:

- `target_id` for this free container, distinct from `oracle-19c-selfmanaged` and from `oracle-xstream`
- driver identity `oracledb` / `26.0.0-beta.4` / mode `thin`, plus the crate checksum the consumer actually built
- `secret_ref` only, never a password
- connect host, port, and PDB service (`FREEPDB1` on this image), not a CDB shortcut
- a batch read and an `AS OF SCN` read, with the SCN and `undo_retention` / undo retention guarantee in the access report
- semantic flags the catalog already names: `empty_string_is_null`, `DATE` includes time, unconstrained `NUMBER` kept off the binary64 path, `TIMESTAMP(9)` as `precision_loss` if the destination is `timestamp(6)`
- explicit rejection of AWR, LogMiner, XStream, supplemental-log DDL, and any `ANY` system privilege

Missing driver or digest stays `DRIVER_MISSING`. No fallback to a generic driver. CDC stays excluded until a separate DBP1 ADR. That proposal is not an implemented API.
