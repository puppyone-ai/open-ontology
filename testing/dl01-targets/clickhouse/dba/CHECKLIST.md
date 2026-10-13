# ClickHouse fixture checklist

This checklist is the operator reading of the runner. It is not applied by hand in this session. The executable path is `python3 testing/dl01-targets/clickhouse/bin/run_prep.py`.

## Preconditions

- Docker can run `linux/arm64`.
- Image `clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa` is present. The tag is `26.8.21.10`.
- Port `127.0.0.1:18123` is free.
- Names `oo-dl01w1-11-clickhouse`, `oo-dl01w1-11-net`, and `oo-dl01w1-11-data` are absent, or they already carry label `oo.owner=oo-dl01w1-11`. A name without that owner is a hard stop.
- Global Rust processes plus running Rust-builder containers are under 10. This task starts none.
- `/tmp/oo-dl01-wave1-resources.lock` is taken around the census and `docker run`.

## Accounts and secrets

- Create `/tmp/oo-dl01w1-11-secrets` mode `0700`.
- Generate two passwords with a CSPRNG into mode `0600` files. Do not pass them on argv or in `CLICKHOUSE_PASSWORD`.
- Mount `users.d/oo-users.xml` with `password_sha256_hex` only.
- DBA `oo_dl01w1_11_dba`: profile `default`, `GRANT ALL ON *.* WITH GRANT OPTION`. ClickHouse 26.8 rejects `access_management` beside `<grants>` (attempt-02, code 36).
- Reader `oo_dl01w1_11_reader`: profile `oo_reader` (`readonly=1`, `max_execution_time=8`, JSON quoting for 64-bit integers and decimals, decimal trailing zeros, `function_sleep_max_microseconds_per_block=30000000`), quota `oo_reader`, and the `SELECT` / `SHOW DATABASES` grants. The server default sleep cap is 3 seconds and rejects `sleep(30)` with code 160 (attempt-04).
- Leave `CLICKHOUSE_USER` unset so the entrypoint still confines `default` to localhost.

## Apply

1. Start one container, memory `1536m`, swap `1536m`, cpus `1`, publish `127.0.0.1:18123:8123`, labels `oo.project=open-ontology`, `oo.owner=oo-dl01w1-11`, `oo.session=4e795a4e-818a-4f9b-8846-09b070d1278c`, `oo.kind=clickhouse-source`.
2. Wait until `GET /ping` returns `Ok`. A TCP reset during startup is not ready. An exited container is retried once at `2g`.
3. Confirm anonymous HTTP, the `default` user with an empty password, and a wrong reader password do not return 200.
4. As the DBA, run `dba/00-fixture.sql` one statement per HTTP request. That creates `oo_dl01`, stops merges, inserts the four reading versions, and issues one lightweight delete on `retirees`.
5. Do not run `GRANT` against the reader. Attempt-03 returned code 495, `ACCESS_STORAGE_READONLY`. The grants in the users file are the applied form. `dba/01-grants.sql` is that note.
6. As the reader, run the probes under `probes/` except `checksum.sql`. Compare parsed JSON to `expected/`. The reader guard rejects a first keyword of insert, delete, alter, create, drop, truncate, optimize, kill, grant, revoke, attach, detach, rename, exchange, system, update, or replace.
7. DBA checksum, then reader probes, then the same checksum. The two bodies must match.
8. Cancel: reader `SELECT sleep(30)` with query id `oo-dl01w1-11-cancel` on one connection; DBA `KILL QUERY ... SYNC` when `system.processes` shows that id. Pass on `QUERY_WAS_CANCELLED` with elapsed under 8 seconds.
9. Budget: the same sleep with query id `oo-dl01w1-11-budget` and no kill. Pass on `TIMEOUT_EXCEEDED` with elapsed from 5 to 20 seconds. Join the cancel thread before this query so only one reader business connection is open.
10. DBA `SYSTEM FLUSH LOGS`, then the reader reads `system.query_log`. Forbidden successful kinds are Insert, Delete, Alter, Create, Drop, Truncate, Optimize, KillQuery, Rename, Grant, Revoke, System, and Update.
11. DBA runs `dba/rollback.sql` (`DROP DATABASE IF EXISTS oo_dl01`). The reader select of `oo_dl01.readings` must fail.

## Verify

- `raw/attempt-NN/summary.json` has `"exit_code": 0` and `"failures": []`.
- `http/batch-final.body` matches `expected/batch-final.json`, including the Decimal256 text.
- `http/batch-nofinal.body` still contains reading 101 version 1.
- `delete-observation.json` records the mutation. A finished mutation with row 9 still visible is a failure. A finished mutation with only row 10 visible is an applied delete, not a lagged one.
- Cleanup removes the container, volume, and network, and leaves the image.

## If this checklist is not applied

The runner is the application. Starting the image without the generated users file leaves `default` localhost-only and creates neither fixture account. Probes then have no reader. Do not point the reader at `default`, and do not skip the lockdown checks.
