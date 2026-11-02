# Fixtures for the chunked-typed/v2 driver integration test (T9)

Owned, disposable fixtures. Everything is named `oo-cb-T9-*`. Nothing here touches another
task's containers, networks or volumes. Secrets and certificates live in `/tmp/oo-cb-T9/fixture`
(directory 0700, passwords 0600, no newline).

| Piece | What | Created by |
|---|---|---|
| `oo-cb-T9-mysql57` | MySQL 5.7.44 (`linux/amd64`, emulated), ROW binlog, GTID, TLS with a test CA | `up.sh` |
| `oo-cb-T9-pg17` | PostgreSQL 17 (`linux/arm64`) | `up.sh` |
| `oo_cb_t9` database | source tables `items` and `small` (created by the test) | the test |
| `oo_cb_t7_apply` database | full migration chain 0000–0042, installed by the T7 live test | `run-in-container.sh` step 3 |

## Bring-up

```sh
tools/job-worker/tests/fixtures/c1_v2_driver/up.sh      # idempotent
```

## Run

```sh
tools/job-worker/tests/fixtures/c1_v2_driver/run-in-container.sh   # host side: docker run of the rust:1.94.0 container
```

The container builds `mysql-preview` (producer) and `oo-pipeline` (loader), installs the
chain with the T7 live test (`crates/oo-pipeline/tests/native_c1_chunked_apply_live.rs`), and
then runs the ignored tests in `tools/job-worker/tests/c1_v2_driver_live.rs`.

Environment of the integration test (set by `run-in-container.sh`):

- `OO_CB_T9_MYSQL_HOST`, `OO_CB_T9_MYSQL_PORT`, `OO_CB_T9_MYSQL_CLI` (`mysql` client binary)
- `OO_CB_T9_MYSQL_ROOT_PW_FILE`, `OO_CB_T9_MYSQL_WRITER_PW_FILE`, `OO_CB_T9_MYSQL_CDC_PW_FILE` (0600 files)
- `OO_CB_T9_CA`, `OO_CB_T9_TLS_NAME` (certificate name override, see ONBOARDING-API §5)
- `OO_CB_T9_PG_HOST`, `OO_CB_T9_PG_PORT`, `OO_CB_T9_PG_DATABASE`, `OO_CB_T9_PG_SUPER_PW_FILE`, `OO_CB_T9_PG_RUNTIME_PW_FILE`
- `OO_CB_T9_PRODUCER_BIN`, `OO_CB_T9_LOADER_BIN`, `OO_CB_T9_WORK`

## Teardown

```sh
tools/job-worker/tests/fixtures/c1_v2_driver/down.sh
```

## What the runner does, and why it looks the way it does

1. **Network.** The loader accepts `ssl: disable` only for `127.0.0.1`. The rust container
   therefore joins the PostgreSQL container's network namespace (`--network container:oo-cb-T9-pg17`),
   so PostgreSQL is at `127.0.0.1:5432`. MySQL is reached over its bridge address, and its
   certificate is verified against the name `localhost` (the producer's `tls_server_name`).
2. **Chain install.** The migration chain 0000–0042 is installed by the T7 live test
   (`install_everything`, which runs before its scenarios). Its scenarios then read `.arrow`
   fixtures that are git-excluded and absent from a checkout, so that step is expected to
   report `T7_STEP_EXIT`. The runner prints it and continues.
3. **Fixture grants.** `native_source_instance_register` (0024) calls
   `pg_advisory_xact_lock(integer, integer)`. The installer grants `oo_target_owner` only the
   `bigint` overload, so the test grants the other overload to `oo_target_owner` (superuser,
   fixture only). This is finding F2 in the T9 report.
4. **Key type.** The fixture key is a signed `BIGINT`. A `BIGINT UNSIGNED` key maps to
   `numeric(20,0)`, and the loader's chunk path refuses decimal key cells (finding F3).
5. **Unique ids.** Each test run uses a fresh source name and task id (the loader keeps state
   per source and task).

## Results recorded with this runner

- `multi_table_baseline_with_concurrent_writes_matches_the_source`: pass.
- `a_killed_producer_resumes_and_no_loaded_chunk_is_read_again`: pass (chunks 1–7 loaded once each).
- `a_table_over_32_mib_loads_in_bounded_chunks`: pass (40,000 rows, 6 chunks).
