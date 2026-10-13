# Fixture DBA checklist

Use this only for the `oo-dl01w1-08` synthetic source. Do not run it against another database.

1. Confirm the image architecture is `arm64` and the id is `sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39`. Do not start the amd64 manifest.
2. Confirm no container, volume, or network already uses the names `oo-dl01w1-08-mariadb`, `oo-dl01w1-08-mariadb-data`, or `oo-dl01w1-08`.
3. Hold `/tmp/oo-dl01-wave1-resources.lock` for the admission snapshot and the `docker run`. Rust builders running must be below 10, and this session must have none.
4. Keep passwords in `/tmp/oo-dl01w1-08-secrets` only. Mode 700 on the directory and 600 on the files. Do not put them in argv, git, or logs.
5. Bootstrap `root` creates `oo_synth` and the two accounts. `oo_dba` then loads `sql/01-schema.sql` and `sql/02-data.sql`.
6. Reader grants are `SELECT` on `oo_synth.*` for `localhost` and `%`. DBA grants are `ALL` on that schema, `SELECT` on `mysql.general_log`, `PROCESS`, and `CONNECTION ADMIN`.
7. Leave `general_log` off until the accounts exist. Then set `log_output=TABLE`, truncate the log table, and turn the log on.
8. Reader sessions may `SELECT`, `SHOW`, `SET NAMES`, and `SET time_zone`. They do not get a trial `INSERT`, `UPDATE`, `DELETE`, or DDL statement.
9. Cancel the reader with `KILL QUERY` from `oo_dba`. The reader client needs `--comments` if the statement carries a marker.
10. Cleanup deletes only resources whose `oo.owner` label is `DL01-W1-08`. The pinned image may remain.
