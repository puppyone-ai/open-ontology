# DL01-W1-08 MariaDB preparation

Synthetic MariaDB 11.4.13 source fixture for target preparation. The support matrix grade stays designed. This tree does not accept MariaDB as a source and does not infer behavior from MySQL.

## Pin

Official image `docker.io/library/mariadb:11.4.13`, native `linux/arm64/v8` manifest `sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39`. The amd64 manifest is recorded in `pin.json` and was not started. OCI license label `GPL-2.0`. Live package, client, and server hashes are in `inventory/manifest.json`.

Server flags: `--skip-log-bin`, `--default-time-zone=+00:00`, `--skip-name-resolve`, `--innodb-buffer-pool-size=128M`, and an explicit sql_mode that keeps the image default and adds `NO_ZERO_DATE` and `NO_ZERO_IN_DATE`.

## Identities

| Identity | Account | Privilege |
| --- | --- | --- |
| Bootstrap | `root` | Password file only. Creates the database and accounts, then reads `SHOW GRANTS`. |
| Fixture DBA | `oo_dba` | `ALL` on `oo_synth.*`, `SELECT` on `mysql.general_log`, `PROCESS`, `CONNECTION ADMIN`. |
| Reader | `oo_reader` | `SELECT` on `oo_synth.*` for `localhost` and `%`. |

Passwords are created under `/tmp/oo-dl01w1-08-secrets` and are not passed on argv. The directory is removed on a successful cleanup.

## Run

From the repository root:

```bash
bash testing/dl01-targets/mariadb/scripts/run-all.sh
```

The script runs labeled unit checks, then `prepare`, `preflight`, `type`, `coverage`, `cancel`, `audit`, and `cleanup`. Raw stdout, stderr, and exit files are written under the session evidence directory. A non-zero phase leaves the labeled container, volume, network, and secret directory in place.

Preparation does not compile Rust. The pinned Rust 1.94 image is not started.

## Schema

Database `oo_synth`, charset `utf8mb4`, collation `utf8mb4_bin`. Tables cover unsigned, decimal, null, JSON, temporal, and binary keys, plus an empty zero-date probe. JSON is stored as MariaDB's `LONGTEXT` alias.
