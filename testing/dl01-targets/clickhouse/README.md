# DL01 ClickHouse target preparation

Plan parent: DBX-W12. This directory prepares one ClickHouse server and the synthetic fixture around it. The support matrix stays `designed`. This packet does not claim source-support acceptance, CDC, or the Arrow to PostgreSQL builder.

Run from the repo root:

```bash
python3 testing/dl01-targets/clickhouse/bin/run_prep.py
```

The runner creates password files under `/tmp/oo-dl01w1-11-secrets` (mode `0700`) and mounts a users file. Passwords are not arguments, environment variables, or committed files. The labeled unit check is:

```bash
python3 testing/dl01-targets/clickhouse/bin/run_prep.py --self-check-only
```

The live run holds `/tmp/oo-dl01-wave1-resources.lock` while it counts Rust processes and starts Docker. It admits only when host `rustc`/`cargo`/`rustdoc`/`rustup` plus running `oo.kind=rust-builder` containers stay under 10, and this owner has no Rust builder. One container is published on `127.0.0.1:18123` only. Native ports 9000 and 9009 are not published. The memory cap starts at `1536m` and retries once at `2g` if that container exits during start.

| Account | Where it is defined | Used for |
|---|---|---|
| `default` | image entrypoint, localhost only | lockdown check; host HTTP must fail |
| `oo_dl01w1_11_dba` | generated users file, `GRANT ALL ON *.* WITH GRANT OPTION` | fixture DDL, DML, checksum, `KILL QUERY`, rollback |
| `oo_dl01w1_11_reader` | generated users file, profile `oo_reader`, `SELECT` grants | catalog, batch, partitions, parts, cancel target, budget, audit |

ClickHouse 26.8 keeps `users.d` accounts in readonly `users_xml` storage, so the DBA cannot `GRANT` to the reader. The reader privileges are in the generated users file. `dba/01-grants.sql` records that constraint. `dba/CHECKLIST.md` is the operator checklist and is not a second runner.

Pins, the server license, and the uncompiled driver are in `PIN.md`. The missing runtime contract is `API-PROPOSAL.md`.
