# Oracle Database Free — DL01-W1-10 preparation

Plan parent DBX-W09. This is batch-route preparation for a legally usable free target, not source-support acceptance and not a support-matrix promotion.

Pinned image: `docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8` (`23.26.3-faststart-arm64`). License and driver choice: `PIN.json`, `NOTICE.md`, `DRIVER-ROUTE.md`.

## What a run proves

A private container, prefix `oo-dl01w1-10`, memory 6g, 2 CPUs. SYS (OS authentication, `sqlplus / as sysdba`) is the DBA identity and creates the synthetic schema `OO_FIX`. `OO_READER` is a separate least-privilege user: `CREATE SESSION`, table `SELECT`, object `FLASHBACK`, and `EXECUTE` on `SYS.DBMS_FLASHBACK`. `OO_NOFLASH` has `CREATE SESSION` and `SELECT` only. Passwords stay in `/tmp/oo-dl01w1-10-secrets` and are not written to Git, argv, or the evidence spool.

The reader journey checks unconstrained `NUMBER` against decimal literals (including values that are not binary64), `DATE` values that include a time, `''` stored as NULL, and `TIMESTAMP(9)`. It then reads `AS OF SCN` inside the undo window, a stale SCN, and a missing `FLASHBACK` grant. Unified Auditing is the zero-write record. One in-flight reader statement is cancelled. AWR views are not opened. LogMiner is not started.

## Run

From the repository root, after the wave resource lock is free:

```bash
bash testing/dl01-targets/oracle/scripts/run-all.sh
```

The script refuses to start a container when the admission snapshot shows 10 or more Rust builders, or one owned by this task. It does not compile Rust. Linux/MSRV applicability of the future consumer stays pending until a Rust task is assigned and the DBX-W04 bridge SHA is fixed.

Evidence from the assigned session is `evidence/18da73a5-ad34-477a-8edf-0dc51122c1dd/DL01/oracle/`.

## Not in this task

- No edit to `tools/source-driver-worker` or any other worker's tree.
- No 19c target (that edition is not the free program).
- No CDC, supplemental logging change, or R1w flush table.
- No load into Agent DB. The Arrow consumer waits on the frozen bridge.
