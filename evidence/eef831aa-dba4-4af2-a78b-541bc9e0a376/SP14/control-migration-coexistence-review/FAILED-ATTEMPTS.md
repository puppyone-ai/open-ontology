# Failed attempts

These are harness failures. The green cargo exits were not repeated. Product files were not edited.

## run-proof.sh dropped stdin

`linux/run-proof.sh` called `docker exec` without `-i` for the deny `DO` blocks and for `CREATE DATABASE`. `psql` saw EOF, exited 0, and ran no SQL.

Kept evidence:

- `proof/facts-deny-oo_a365_cal_first.log` and `proof/facts-deny-oo_a365_enum_first.log` are empty.
- `proof/raw-create.log` is empty and `proof/raw-create.log.exit` is 0. That 0 is a false success. The databases did not exist.
- `proof/raw-tx-calendar.log` is `FATAL: database "oo_a376_raw_tx" does not exist`. No `.exit` file was written. The script aborted at `raw-psql` before clippy and the oracle.

`linux/continue-proof.sh` repeated those probes with `docker exec -i` into the same `oo-a376-pg`. Retry logs use the `-retry` suffix. `proof/facts-deny.txt` and `proof/facts-raw.txt` come from the retry.

## Image id string

The first `continue-proof.sh` launch compared `docker inspect` image id `sha256:…` with the reference `docker.io/library/rust@sha256:…`. The strings differ. The process exited 1 before `continue-start` and before any SQL. The check now compares `docker image inspect` id values.

## Oracle did not compile

`proof/oracle-compile-attempt.log` is rustc `E0277` at `line[prefix.len()..] == *body` (`str` versus `&str`). Exit 1. The comparison is now `strip_prefix`. `proof/oracle.log` is the successful `oracle_ok` run. The product crates were not compiled by that `rustc` command.

## SQLSTATE text

The retry parser looked for `SQLSTATE: `. PostgreSQL 17 verbose output is `ERROR:  42P06: schema "oo_dict" already exists`. `proof/facts-raw.txt` was set to `raw_tx|second|42P06` from `proof/raw-tx-enum-retry.log`. The catalog counts were already `oo_calendar_control|1`, `oo_enum_control|0`, `oo_dict|1`.

## dual view expectation

The first oracle required `raw_auto|dual|0`. Autocommit created one view. `raw_auto|dual|1` is the measured count: `oo_dict.calendar` owned by `oo_calendar_admin`, no `enums` view, and `oo_enum_control` left behind. The oracle now requires that count, the partial schema count 1, the two view owners, and warning `25P01`. Requiring `dual|0` would have hidden the real unsupported outcome.

## Builder count

A later `continue-proof.sh` launch exited 1 because three Rust builders were running: `oo-a376-proof`, `oo-a375-proof`, and `oo-fe01-rust-m6`. No SQL ran. The script now requires one a376 builder and a global count at or under 10, and it still does not start a container. `proof/continue-capacity.txt` records `own_builders=1 global_builders=3` for the oracle rerun. The foreign builders were not removed.
